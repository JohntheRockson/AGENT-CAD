//! Geometry execution engine.
//!
//! The `Engine` is a zero-cost dispatch wrapper. By default (no `occt` feature)
//! it uses the pure-Rust mock backend, which is fast and allows `cargo test`
//! to pass without any native dependencies. Enable the `occt` feature (done
//! automatically by `crates/server`) to get real OpenCASCADE B-Rep geometry.

use crate::ir::{CadDocument, CadProgram, Feature, Profile};
use thiserror::Error;

// ── Output types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MeshData {
    /// Flat vertex positions: [x0, y0, z0, x1, y1, z1, …]
    pub positions: Vec<f32>,
    /// Per-vertex normals in the same order as `positions`.
    pub normals: Vec<f32>,
    /// Triangle indices. Empty means non-indexed (every 3 vertices = 1 triangle).
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct MetricsData {
    pub volume: f64,
    /// [xmin, ymin, zmin, xmax, ymax, zmax] in model units.
    pub bbox: [f64; 6],
    pub surface_area: f64,
    pub is_solid: bool,
}

#[derive(Debug, Clone)]
pub struct BodyOutput {
    pub body_id: String,
    pub name: String,
    pub visible: bool,
    pub suppressed: bool,
    pub mesh: MeshData,
    pub metrics: MetricsData,
}

#[derive(Debug, Clone)]
pub struct DocumentOutput {
    pub bodies: Vec<BodyOutput>,
    /// Combined metrics of visible bodies.
    pub metrics: MetricsData,
}

impl DocumentOutput {
    pub fn primary_mesh(&self) -> Option<&MeshData> {
        self.bodies
            .iter()
            .find(|b| b.visible && !b.suppressed)
            .or_else(|| self.bodies.first())
            .map(|b| &b.mesh)
    }

    pub fn into_model_output(self) -> Result<ModelOutput, KernelError> {
        let mesh = combine_meshes(
            &self
                .bodies
                .iter()
                .filter(|b| b.visible && !b.suppressed)
                .map(|b| &b.mesh)
                .collect::<Vec<_>>(),
        );
        if mesh.positions.is_empty() {
            return Err(KernelError::InvalidState(
                "Document produced no visible solid".into(),
            ));
        }
        Ok(ModelOutput {
            mesh,
            metrics: self.metrics,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ModelOutput {
    pub mesh: MeshData,
    pub metrics: MetricsData,
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("Validation: {0}")]
    Validation(#[from] crate::ir::ValidationError),
    #[error("OCCT kernel error: {0}")]
    Occt(String),
    #[error("Invalid program state: {0}")]
    InvalidState(String),
}

// ── Engine ────────────────────────────────────────────────────────────────────

/// Lightweight, `Copy`-able dispatch handle. Stores no OCCT state (the kernel
/// lives in a thread-local). Safe to put in Axum `AppState`.
#[derive(Clone, Copy, Debug)]
pub struct Engine {
    use_occt: bool,
}

impl Engine {
    /// In a build with `feature = "occt"` this uses the real kernel.
    /// In all other builds it uses the mock.
    pub fn new() -> Self {
        #[cfg(feature = "occt")]
        return Engine { use_occt: true };
        #[cfg(not(feature = "occt"))]
        Engine { use_occt: false }
    }

    /// Always uses the mock backend – the right choice for unit tests.
    pub fn mock() -> Self {
        Engine { use_occt: false }
    }

    pub fn uses_occt(&self) -> bool {
        self.use_occt
    }

    /// Compile the OCCT WASM module (once per process) and instantiate a kernel
    /// on this thread. Call at server startup so the first `/api/run` is not
    /// blocked on Cranelift.
    pub fn warmup(&self) -> Result<(), KernelError> {
        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::warmup();
            #[cfg(not(feature = "occt"))]
            let _ = self.use_occt;
        }
        Ok(())
    }

    pub fn execute(&self, program: &CadProgram) -> Result<ModelOutput, KernelError> {
        program.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::execute_with_occt(program);
            #[cfg(not(feature = "occt"))]
            let _ = self.use_occt;
        }
        mock_backend::execute_with_mock(program)
    }

    pub fn execute_document(&self, document: &CadDocument) -> Result<DocumentOutput, KernelError> {
        let mut document = document.clone();
        crate::params::bind_independent_bolt_dims(&mut document);
        document.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::execute_document_with_occt(&document);
            #[cfg(not(feature = "occt"))]
            let _ = self.use_occt;
        }
        mock_backend::execute_document_with_mock(&document)
    }

    /// Export the current program to bytes in the requested format.
    pub fn export(
        &self,
        program: &CadProgram,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        program.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::export_with_occt(program, format);
            #[cfg(not(feature = "occt"))]
            let _ = self.use_occt;
        }
        mock_backend::export_with_mock(program, format)
    }

    pub fn export_document(
        &self,
        document: &CadDocument,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        let mut document = document.clone();
        crate::params::bind_independent_bolt_dims(&mut document);
        document.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::export_document_with_occt(&document, format);
        }
        mock_backend::export_document_with_mock(&document, format)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::mock()
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Step,
    Stl,
    #[serde(alias = "glb")]
    Gltf,
    Obj,
    Brep,
}

impl ExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Step => "step",
            ExportFormat::Stl => "stl",
            ExportFormat::Gltf => "glb",
            ExportFormat::Obj => "obj",
            ExportFormat::Brep => "brep",
        }
    }
    pub fn mime(&self) -> &'static str {
        match self {
            ExportFormat::Step => "application/step",
            ExportFormat::Stl => "model/stl",
            ExportFormat::Gltf => "model/gltf-binary",
            ExportFormat::Obj => "text/plain",
            ExportFormat::Brep => "application/octet-stream",
        }
    }
}

// ── Mock backend ──────────────────────────────────────────────────────────────
//
// Generates analytically correct geometry for sketch+extrude programs. The
// geometry is a flat-shaded box (no OCCT required) which is enough for tests
// and for iterating on the UI without the heavy kernel build.

pub(crate) mod mock_backend {
    use super::*;
    use crate::export;

    pub fn execute_with_mock(program: &CadProgram) -> Result<ModelOutput, KernelError> {
        let mut w = 0.0_f64;
        let mut h = 0.0_f64;
        let mut depth = 0.0_f64;

        for feat in &program.features {
            match feat {
                Feature::Sketch(op) => match &op.profile {
                    Profile::Rect(r) => {
                        w = r.w;
                        h = r.h;
                    }
                    Profile::Circle(c) => {
                        w = c.d;
                        h = c.d;
                    }
                    Profile::Hex(hx) => {
                        w = hx.across_flats;
                        h = hx.across_flats;
                    }
                    _ => {}
                },
                Feature::Extrude(op) => depth = op.depth,
                Feature::Box(op) => {
                    w = op.size[0];
                    h = op.size[1];
                    depth = op.size[2];
                }
                Feature::Cylinder(op) => {
                    w = op.diameter;
                    h = op.diameter;
                    depth = op.height;
                }
                Feature::Ellipsoid(op) => {
                    w = op.radii[0] * 2.0;
                    h = op.radii[1] * 2.0;
                    depth = op.radii[2] * 2.0;
                }
                Feature::Helix(op) => {
                    w = (op.radius + op.diameter) * 2.0;
                    h = w;
                    depth = op.height;
                }
                Feature::Thread(op) => {
                    let d = op.diameter.unwrap_or(8.0);
                    w = d;
                    h = d;
                    depth = if op.length > 0.0 { op.length } else { d * 2.0 };
                }
                Feature::Sweep(_) => {
                    if w <= 0.0 {
                        w = 10.0;
                        h = 10.0;
                    }
                    if depth <= 0.0 {
                        depth = 10.0;
                    }
                }
                _ => {}
            }
        }

        if w <= 0.0 || h <= 0.0 || depth <= 0.0 {
            return Err(KernelError::InvalidState(
                "Program must contain a sketch with positive dimensions followed by an extrude"
                    .into(),
            ));
        }

        let (positions, normals) = box_mesh_flat(w as f32, h as f32, depth as f32);
        let volume = w * h * depth;
        let bbox = [0.0, 0.0, 0.0, w, h, depth];
        let surface_area = 2.0 * (w * h + w * depth + h * depth);

        Ok(ModelOutput {
            mesh: MeshData {
                positions,
                normals,
                indices: vec![],
            },
            metrics: MetricsData {
                volume,
                bbox,
                surface_area,
                is_solid: true,
            },
        })
    }

    pub fn export_with_mock(
        program: &CadProgram,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        match format {
            ExportFormat::Stl => {
                let out = execute_with_mock(program)?;
                Ok(export::to_stl(&out.mesh))
            }
            ExportFormat::Obj => {
                let out = execute_with_mock(program)?;
                Ok(export::to_obj(&out.mesh).into_bytes())
            }
            ExportFormat::Step | ExportFormat::Gltf | ExportFormat::Brep => {
                Err(KernelError::InvalidState(
                    "STEP, glTF and BREP export require the `occt` feature".into(),
                ))
            }
        }
    }

    pub fn execute_document_with_mock(
        document: &CadDocument,
    ) -> Result<DocumentOutput, KernelError> {
        let mut bodies = Vec::new();
        for body in &document.bodies {
            if body.suppressed {
                continue;
            }
            let prog = CadProgram {
                units: document.units.clone(),
                features: body.features.clone(),
            };
            let mut out = execute_with_mock(&prog)?;
            offset_mesh(&mut out.mesh, body.transform.position);
            out.metrics.bbox = crate::engine::bbox_from_positions(&out.mesh.positions);
            bodies.push(BodyOutput {
                body_id: body.body_id.clone(),
                name: body.display_name().to_string(),
                visible: body.visible,
                suppressed: false,
                mesh: out.mesh,
                metrics: out.metrics,
            });
        }
        Ok(document_output_from_bodies(bodies))
    }

    pub fn export_document_with_mock(
        document: &CadDocument,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        let out = execute_document_with_mock(document)?;
        let model = out.into_model_output()?;
        match format {
            ExportFormat::Stl => Ok(export::to_stl(&model.mesh)),
            ExportFormat::Obj => Ok(export::to_obj(&model.mesh).into_bytes()),
            ExportFormat::Step | ExportFormat::Gltf | ExportFormat::Brep => {
                Err(KernelError::InvalidState(
                    "STEP, glTF and BREP export require the `occt` feature".into(),
                ))
            }
        }
    }

    fn offset_mesh(mesh: &mut MeshData, pos: [f64; 3]) {
        let (dx, dy, dz) = (pos[0] as f32, pos[1] as f32, pos[2] as f32);
        if dx == 0.0 && dy == 0.0 && dz == 0.0 {
            return;
        }
        for chunk in mesh.positions.chunks_mut(3) {
            if chunk.len() == 3 {
                chunk[0] += dx;
                chunk[1] += dy;
                chunk[2] += dz;
            }
        }
    }

    /// Flat-shaded box mesh (non-indexed). Suitable for both rendering and STL.
    /// 6 faces × 2 triangles × 3 vertices = 36 vertices.
    fn box_mesh_flat(w: f32, h: f32, d: f32) -> (Vec<f32>, Vec<f32>) {
        let verts: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0], // 0
            [w, 0.0, 0.0],   // 1
            [w, h, 0.0],     // 2
            [0.0, h, 0.0],   // 3
            [0.0, 0.0, d],   // 4
            [w, 0.0, d],     // 5
            [w, h, d],       // 6
            [0.0, h, d],     // 7
        ];

        // Each quad: corner indices and outward face normal
        let faces: [([usize; 4], [f32; 3]); 6] = [
            ([0, 1, 2, 3], [0.0, 0.0, -1.0]), // -Z front
            ([5, 4, 7, 6], [0.0, 0.0, 1.0]),  // +Z back
            ([4, 0, 3, 7], [-1.0, 0.0, 0.0]), // -X left
            ([1, 5, 6, 2], [1.0, 0.0, 0.0]),  // +X right
            ([4, 5, 1, 0], [0.0, -1.0, 0.0]), // -Y bottom
            ([3, 2, 6, 7], [0.0, 1.0, 0.0]),  // +Y top
        ];

        let mut positions = Vec::with_capacity(36 * 3);
        let mut normals = Vec::with_capacity(36 * 3);

        for ([a, b, c, e], n) in &faces {
            // Two triangles per quad: ABC and ACE
            for &vi in &[*a, *b, *c, *a, *c, *e] {
                positions.extend_from_slice(&verts[vi]);
                normals.extend_from_slice(n);
            }
        }

        (positions, normals)
    }
}

// ── OCCT backend (optional) ───────────────────────────────────────────────────
//
// Wraps the `occt-wasm` crate. The kernel lives in a thread-local so it is
// never sent across threads; callers should dispatch via `tokio::task::spawn_blocking`.

#[cfg(feature = "occt")]
pub(crate) mod occt_backend {
    use super::*;
    use crate::ir::*;
    use std::cell::{Cell, RefCell};
    use std::collections::hash_map::DefaultHasher;
    use std::collections::{HashMap, HashSet};
    use std::f64::consts::PI;
    use std::hash::{Hash, Hasher};

    // Maximum number of cached shape states kept per thread.
    // Each entry is tiny (2 Option<u32> + an enum), but the WASM arena holding
    // the actual shapes grows with each unique result — cap to bound memory.
    const CACHE_LIMIT: usize = 64;

    /// Snapshot of execution state after one feature step, keyed by cumulative hash.
    #[derive(Clone)]
    struct StepEntry {
        face: Option<u32>, // raw arena ID, not ShapeHandle (avoids Send/Sync issues)
        solid: Option<u32>,
        plane: SketchPlane,
        last_tool: Option<u32>,
        last_boolean: Option<LastBoolean>,
        face_normal: Option<[f64; 3]>,
        base_before_face_sketch: Option<u32>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum LastBoolean {
        Cut,
        Fuse,
    }

    /// Peak WASM budget: an ~8-turn M8 V-groove tessellates; 20+ turns trap.
    const MAX_INLINE_THREAD_TURNS: f64 = 8.0;

    #[derive(Clone, Copy, Debug)]
    struct ThreadPreview {
        major: f64,
        pitch: f64,
        length: f64,
        at: [f64; 3],
        left: bool,
    }

    thread_local! {
        static KERNEL: RefCell<Option<occt_wasm::OcctKernel>> = const { RefCell::new(None) };
        /// Maps (cumulative feature hash) → state after that feature.
        static STEP_CACHE: RefCell<HashMap<u64, StepEntry>> = RefCell::new(HashMap::new());
        /// After a tessellate wasm trap, rebuild with a coarser absolute mesh.
        static COARSE_LEVEL: Cell<u8> = const { Cell::new(0) };
    }

    fn is_fatal_occt(err: &KernelError) -> bool {
        let s = err.to_string().to_lowercase();
        s.contains("out of bounds")
            || s.contains("wasm trap")
            || s.contains("wasm runtime")
            || s.contains("error while executing")
            || s.contains("memory fault")
            || s.contains("unreachable")
            || s.contains("internal cad kernel crash")
            || s.contains("kernel task panicked")
    }

    fn sanitize_occt_message(s: &str) -> String {
        let lower = s.to_lowercase();
        if lower.contains("out of bounds")
            || lower.contains("wasm trap")
            || lower.contains("wasm runtime")
            || lower.contains("error while executing")
            || lower.contains("memory fault")
            || lower.contains("unreachable")
        {
            let op = s.split(':').next().unwrap_or("operation").trim();
            return format!(
                "{op}: internal CAD kernel crash (wasm memory). Body rotation and \
                 cylinders on X/Y are valid — do not drop those ops. Start each body \
                 with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude."
            );
        }
        if let Some(i) = lower.find("wasm backtrace") {
            return s[..i].trim().trim_end_matches(':').trim().to_string();
        }
        s.to_string()
    }

    fn occt_err(msg: impl std::fmt::Display) -> KernelError {
        KernelError::Occt(sanitize_occt_message(&msg.to_string()))
    }

    fn discard_kernel() {
        KERNEL.with(|cell| {
            *cell.borrow_mut() = None;
        });
        STEP_CACHE.with(|c| c.borrow_mut().clear());
    }

    pub(crate) fn warmup() -> Result<(), KernelError> {
        occt_wasm::OcctKernel::precompile()
            .map_err(|e| KernelError::Occt(format!("OCCT precompile: {e}")))?;
        with_kernel(|_| Ok(()))
    }

    fn with_kernel<F, T>(f: F) -> Result<T, KernelError>
    where
        F: FnOnce(&mut occt_wasm::OcctKernel) -> Result<T, KernelError>,
    {
        let result = KERNEL.with(|cell| {
            let mut guard = cell.borrow_mut();
            if guard.is_none() {
                *guard = Some(
                    occt_wasm::OcctKernel::new()
                        .map_err(|e| KernelError::Occt(format!("OCCT init: {e}")))?,
                );
            }
            f(guard.as_mut().unwrap())
        });
        if result.as_ref().err().is_some_and(is_fatal_occt) {
            discard_kernel();
        }
        result
    }

    /// After a wasm trap the instance is dead. Rebuild on a fresh kernel with
    /// successively coarser absolute tessellation (relative meshing OOMs on
    /// long helical threads).
    fn with_kernel_retry<T>(f: impl Fn() -> Result<T, KernelError>) -> Result<T, KernelError> {
        let mut last = KernelError::Occt("kernel retry exhausted".into());
        for level in 0..=2u8 {
            COARSE_LEVEL.with(|c| c.set(level));
            match f() {
                Ok(v) => {
                    COARSE_LEVEL.with(|c| c.set(0));
                    return Ok(v);
                }
                Err(e) if is_fatal_occt(&e) && level < 2 => {
                    last = e;
                }
                Err(e) => {
                    COARSE_LEVEL.with(|c| c.set(0));
                    return Err(e);
                }
            }
        }
        COARSE_LEVEL.with(|c| c.set(0));
        Err(last)
    }

    // ── Handle conversion helpers ─────────────────────────────────────────────
    //
    // get_sub_shapes returns Vec<u32> raw IDs, but fillet/chamfer need &[ShapeHandle].
    // ShapeHandle is a newtype struct ShapeHandle(pub(crate) u32) — identical memory layout.
    // Safety: single-field newtypes have the same size, alignment, and bit-validity as
    // their inner type; every u32 is a valid ShapeHandle.

    fn ids_to_handles(ids: Vec<u32>) -> Vec<occt_wasm::ShapeHandle> {
        unsafe { std::mem::transmute::<Vec<u32>, Vec<occt_wasm::ShapeHandle>>(ids) }
    }

    fn id_to_handle(id: u32) -> occt_wasm::ShapeHandle {
        unsafe { std::mem::transmute::<u32, occt_wasm::ShapeHandle>(id) }
    }

    fn handle_to_id(h: occt_wasm::ShapeHandle) -> u32 {
        unsafe { std::mem::transmute::<occt_wasm::ShapeHandle, u32>(h) }
    }

    /// BRepAlgoAPI_Cut/Fuse always return a TopoDS_Compound, not a TopoDS_Solid.
    /// BRepFilletAPI_MakeFillet (and sub-shape queries) require an actual Solid.
    /// Unwrap to the first solid sub-shape; fall back to the original shape if not found.
    fn unwrap_to_solid(
        k: &mut occt_wasm::OcctKernel,
        shape: occt_wasm::ShapeHandle,
    ) -> occt_wasm::ShapeHandle {
        k.get_sub_shapes(shape, "solid")
            .ok()
            .and_then(|ids| ids.into_iter().next())
            .map(id_to_handle)
            .unwrap_or(shape)
    }

    /// Keep a single healed solid when the boolean produced one; otherwise keep
    /// the compound so disconnected bosses/fasteners are not dropped.
    fn drawable_shape(
        k: &mut occt_wasm::OcctKernel,
        shape: occt_wasm::ShapeHandle,
    ) -> occt_wasm::ShapeHandle {
        match k.get_sub_shapes(shape, "solid") {
            Ok(ids) if ids.len() == 1 => heal_shape(k, id_to_handle(ids[0])),
            Ok(ids) if ids.len() > 1 => shape,
            _ => unwrap_to_solid(k, shape),
        }
    }

    /// Merge same-domain faces after booleans so tessellation doesn't emit
    /// coplanar sliver faces (those show up as hatched ghost rectangles).
    fn heal_shape(
        k: &mut occt_wasm::OcctKernel,
        shape: occt_wasm::ShapeHandle,
    ) -> occt_wasm::ShapeHandle {
        let s = unwrap_to_solid(k, shape);
        let s = k.unify_same_domain(s).unwrap_or(s);
        k.fix_shape(s).unwrap_or(s)
    }

    /// Keep only edges whose underlying curve is a straight line.
    /// Circular/seam edges from hole cylinders cause OCCT fillet to fail.
    fn filter_to_line_edges(k: &mut occt_wasm::OcctKernel, ids: Vec<u32>) -> Vec<u32> {
        ids.into_iter()
            .filter(|&id| {
                k.curve_type(id_to_handle(id))
                    .ok()
                    .map_or(false, |t| t.eq_ignore_ascii_case("line"))
            })
            .collect()
    }

    struct EdgeInfo {
        id: u32,
        length: f64,
        dir: usize,
        mid: [f64; 3],
        is_thickness: bool,
        is_top: bool,
    }

    fn aabb_extents(b: &occt_wasm::BoundingBox) -> [f64; 3] {
        [
            (b.max.x - b.min.x).abs(),
            (b.max.y - b.min.y).abs(),
            (b.max.z - b.min.z).abs(),
        ]
    }

    fn aabb_min(b: &occt_wasm::BoundingBox, axis: usize) -> f64 {
        match axis {
            0 => b.min.x,
            1 => b.min.y,
            _ => b.min.z,
        }
    }

    fn aabb_max(b: &occt_wasm::BoundingBox, axis: usize) -> f64 {
        match axis {
            0 => b.max.x,
            1 => b.max.y,
            _ => b.max.z,
        }
    }

    fn argmin3(v: [f64; 3]) -> usize {
        let mut i = 0;
        if v[1] < v[i] {
            i = 1;
        }
        if v[2] < v[i] {
            i = 2;
        }
        i
    }

    fn argmax3(v: [f64; 3]) -> usize {
        let mut i = 0;
        if v[1] > v[i] {
            i = 1;
        }
        if v[2] > v[i] {
            i = 2;
        }
        i
    }

    fn classify_line_edges(
        k: &mut occt_wasm::OcctKernel,
        solid_bb: &occt_wasm::BoundingBox,
        ids: &[u32],
    ) -> Vec<EdgeInfo> {
        let extents = aabb_extents(solid_bb);
        let thin = argmin3(extents);
        let thickness = extents[thin].max(1e-9);
        ids.iter()
            .filter_map(|&id| {
                let h = id_to_handle(id);
                let length = k.get_length(h).unwrap_or(0.0);
                let eb = k.get_bounding_box(h, false).ok()?;
                let spans = aabb_extents(&eb);
                let dir = argmax3(spans);
                let is_thickness = dir == thin && spans[thin] > 0.55 * thickness;
                let mid = [
                    0.5 * (eb.min.x + eb.max.x),
                    0.5 * (eb.min.y + eb.max.y),
                    0.5 * (eb.min.z + eb.max.z),
                ];
                let is_top = (aabb_max(solid_bb, thin) - mid[thin]).abs()
                    <= (mid[thin] - aabb_min(solid_bb, thin)).abs() + 1e-6;
                Some(EdgeInfo {
                    id,
                    length,
                    dir,
                    mid,
                    is_thickness,
                    is_top,
                })
            })
            .collect()
    }

    /// Pick a set of edges OCCT can actually fillet/chamfer.
    ///
    /// `"all"` on a thin lid used to include the 6 mm verticals *and* both the
    /// top and bottom perimeters. A 5 mm radius on 6 mm stock then self-intersects
    /// at the corners and tessellates into spikes.
    fn select_blend_edges(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        ids: Vec<u32>,
        radius: f64,
    ) -> Vec<u32> {
        let Ok(bb) = k.get_bounding_box(solid, false) else {
            return ids;
        };
        let extents = aabb_extents(&bb);
        let thin = argmin3(extents);
        let thickness = extents[thin];
        let mut edges = classify_line_edges(k, &bb, &ids);
        if edges.is_empty() {
            return ids;
        }

        // Through-thickness edges of a plate cannot take a large radius.
        if thickness < 2.15 * radius {
            edges.retain(|e| !e.is_thickness);
            // Top and bottom both at `radius` would overlap through the stock.
            edges.retain(|e| e.is_top);
        } else {
            edges.retain(|e| !e.is_thickness || e.length > 2.15 * radius);
        }

        edges.retain(|e| e.length > 0.25 * radius);

        // Inner window vs outer perimeter: drop the shorter of two parallels
        // closer than 2r (the remaining wall is too thin for both).
        let min_sep = 2.0 * radius + 0.35;
        edges.sort_by(|a, b| {
            b.length
                .partial_cmp(&a.length)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut kept: Vec<EdgeInfo> = Vec::new();
        for e in edges {
            let clashes = kept.iter().any(|o| {
                if o.dir != e.dir {
                    return false;
                }
                let perp = (0..3).find(|&a| a != e.dir && a != thin).unwrap_or(0);
                (o.mid[perp] - e.mid[perp]).abs() < min_sep
                    && (o.mid[thin] - e.mid[thin]).abs() < 0.35 * thickness.max(1.0)
            });
            if !clashes {
                kept.push(e);
            }
        }
        let out: Vec<u32> = kept.into_iter().map(|e| e.id).collect();
        if out.is_empty() {
            ids
        } else {
            out
        }
    }

    fn aabb_exploded(
        before: &occt_wasm::BoundingBox,
        after: &occt_wasm::BoundingBox,
        radius: f64,
    ) -> bool {
        let pad = radius * 2.0 + 1.0;
        after.max.x > before.max.x + pad
            || after.max.y > before.max.y + pad
            || after.max.z > before.max.z + pad
            || after.min.x < before.min.x - pad
            || after.min.y < before.min.y - pad
            || after.min.z < before.min.z - pad
    }

    fn try_fillet(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        ids: &[u32],
        radius: f64,
    ) -> Option<Handle> {
        if ids.is_empty() || radius <= 0.0 {
            return None;
        }
        let before = k.get_bounding_box(solid, false).ok();
        let handles = ids_to_handles(ids.to_vec());
        let result = k.fillet(solid, &handles, radius).ok()?;
        let result = unwrap_to_solid(k, result);
        if let Some(ref b0) = before {
            if let Ok(b1) = k.get_bounding_box(result, false) {
                if aabb_exploded(b0, &b1, radius) {
                    return None;
                }
            }
        }
        Some(heal_shape(k, result))
    }

    fn try_chamfer(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        ids: &[u32],
        distance: f64,
        angle: Option<f64>,
    ) -> Option<Handle> {
        if ids.is_empty() || distance <= 0.0 {
            return None;
        }
        let before = k.get_bounding_box(solid, false).ok();
        let handles = ids_to_handles(ids.to_vec());
        let result = if let Some(a) = angle {
            k.chamfer_dist_angle(solid, &handles, distance, a).ok()?
        } else {
            k.chamfer(solid, &handles, distance).ok()?
        };
        let result = unwrap_to_solid(k, result);
        if let Some(ref b0) = before {
            if let Ok(b1) = k.get_bounding_box(result, false) {
                if aabb_exploded(b0, &b1, distance) {
                    return None;
                }
            }
        }
        Some(heal_shape(k, result))
    }

    fn longest_edges(k: &mut occt_wasm::OcctKernel, ids: &[u32], take: usize) -> Vec<u32> {
        let mut scored: Vec<(u32, f64)> = ids
            .iter()
            .map(|&id| (id, k.get_length(id_to_handle(id)).unwrap_or(0.0)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(take).map(|(id, _)| id).collect()
    }

    /// Drop triangles with a vertex far outside the B-Rep box (fillet "spikes").
    fn strip_spike_triangles(mesh: &mut occt_wasm::Mesh, bb: &occt_wasm::BoundingBox) {
        if mesh.indices.is_empty() || mesh.positions.len() < 9 {
            return;
        }
        let ext = aabb_extents(bb);
        let diag = (ext[0] * ext[0] + ext[1] * ext[1] + ext[2] * ext[2])
            .sqrt()
            .max(1.0);
        let m = (diag * 0.08).max(1.0);
        let (xmin, ymin, zmin) = (bb.min.x - m, bb.min.y - m, bb.min.z - m);
        let (xmax, ymax, zmax) = (bb.max.x + m, bb.max.y + m, bb.max.z + m);
        let inside = |idx: u32| {
            let i = idx as usize;
            let p = &mesh.positions;
            if i * 3 + 2 >= p.len() {
                return false;
            }
            let (x, y, z) = (p[i * 3] as f64, p[i * 3 + 1] as f64, p[i * 3 + 2] as f64);
            x >= xmin && x <= xmax && y >= ymin && y <= ymax && z >= zmin && z <= zmax
        };
        let mut kept = Vec::with_capacity(mesh.indices.len());
        for tri in mesh.indices.chunks(3) {
            if tri.len() == 3 && inside(tri[0]) && inside(tri[1]) && inside(tri[2]) {
                let i0 = tri[0] as usize * 3;
                let i1 = tri[1] as usize * 3;
                let i2 = tri[2] as usize * 3;
                let ax = mesh.positions[i1] - mesh.positions[i0];
                let ay = mesh.positions[i1 + 1] - mesh.positions[i0 + 1];
                let az = mesh.positions[i1 + 2] - mesh.positions[i0 + 2];
                let bx = mesh.positions[i2] - mesh.positions[i0];
                let by = mesh.positions[i2 + 1] - mesh.positions[i0 + 1];
                let bz = mesh.positions[i2 + 2] - mesh.positions[i0 + 2];
                let cx = ay * bz - az * by;
                let cy = az * bx - ax * bz;
                let cz = ax * by - ay * bx;
                if cx * cx + cy * cy + cz * cz > 1e-16 {
                    kept.extend_from_slice(tri);
                }
            }
        }
        if !kept.is_empty() && kept.len() < mesh.indices.len() {
            mesh.indices = kept;
        }
    }

    // ── Step-hash cache helpers ───────────────────────────────────────────────

    /// Bump when handler semantics change so in-memory step cache cannot
    /// replay solids built with a previous (wrong) revolve/plane mapping.
    const KERNEL_SEMANTICS: u64 = 0xA6E1_CAD0_0000_0008;

    fn body_cache_ns(body_id: &str) -> u64 {
        let mut h = DefaultHasher::new();
        body_id.hash(&mut h);
        h.finish()
    }

    /// Compute the cumulative hash for the first `n` features.
    /// Each feature is serialised to JSON and mixed into a running hash so
    /// any change to any feature invalidates all steps from that point on.
    fn step_hashes(features: &[Feature], namespace: u64) -> Vec<u64> {
        let mut acc: u64 = 0xcbf29ce484222325 ^ KERNEL_SEMANTICS ^ namespace;
        features
            .iter()
            .map(|feat| {
                let json = serde_json::to_string(feat).unwrap_or_default();
                let mut h = DefaultHasher::new();
                acc.hash(&mut h);
                json.hash(&mut h);
                acc = h.finish();
                acc
            })
            .collect()
    }

    // ── State machine ─────────────────────────────────────────────────────────

    type Handle = occt_wasm::ShapeHandle;

    struct ExecState {
        current_face: Option<Handle>,
        current_solid: Option<Handle>,
        active_plane: SketchPlane,
        /// Tool shape from the last cut/fuse/hole (for feature-scope patterns).
        last_tool: Option<Handle>,
        last_boolean: Option<LastBoolean>,
        /// When sketching on a face, extrude along this world normal.
        face_normal: Option<[f64; 3]>,
        /// Solid that existed before a face-based sketch (fused after extrude).
        base_before_face_sketch: Option<Handle>,
    }

    impl Default for ExecState {
        fn default() -> Self {
            ExecState {
                current_face: None,
                current_solid: None,
                active_plane: SketchPlane::XY,
                last_tool: None,
                last_boolean: None,
                face_normal: None,
                base_before_face_sketch: None,
            }
        }
    }

    pub fn execute_with_occt(program: &CadProgram) -> Result<ModelOutput, KernelError> {
        with_kernel_retry(|| {
            with_kernel(|k| {
                let solid =
                    STEP_CACHE.with(|c| execute_in_kernel(k, program, &mut c.borrow_mut(), 0))?;
                tessellate_solid(
                    k,
                    solid,
                    thread_pitch_hint(&program.features, &program.units),
                    long_thread_preview(&program.features, &program.units),
                )
            })
        })
    }

    fn thread_pitch_hint(features: &[Feature], units: &Units) -> Option<f64> {
        let mut pitch = None;
        for feat in features {
            if let Feature::Thread(op) = feat {
                if let Ok((_, p)) = thread_dims(op, units) {
                    pitch = Some(pitch.map(|q: f64| q.min(p)).unwrap_or(p));
                }
            }
        }
        pitch
    }

    fn external_thread_length(op: &ThreadOp, major: f64, pitch: f64) -> f64 {
        if op.length > 0.0 {
            op.length
        } else {
            (major * 2.0).max(pitch * 4.0)
        }
    }

    /// Long Z external threads cannot be meshed (or even cut) as one WASM helix.
    fn preview_from_thread(op: &ThreadOp, units: &Units) -> Option<ThreadPreview> {
        if !matches!(op.kind, ThreadKind::External) {
            return None;
        }
        if !matches!(op.axis, RevolveAxis::Z) {
            return None;
        }
        let (major, pitch) = thread_dims(op, units).ok()?;
        let length = external_thread_length(op, major, pitch);
        if length / pitch.max(1e-9) <= MAX_INLINE_THREAD_TURNS {
            return None;
        }
        Some(ThreadPreview {
            major,
            pitch,
            length,
            at: op.at,
            left: matches!(op.hand, ThreadHand::Left),
        })
    }

    fn long_thread_preview(features: &[Feature], units: &Units) -> Option<ThreadPreview> {
        features.iter().rev().find_map(|f| match f {
            Feature::Thread(op) => preview_from_thread(op, units),
            _ => None,
        })
    }

    fn preview_with_translation(tp: ThreadPreview, transform: &crate::ir::BodyTransform) -> Option<ThreadPreview> {
        let [rx, ry, rz] = transform.rotation;
        if rx.abs() + ry.abs() + rz.abs() > 1e-9 {
            // Segment instancing is Z-axis in feature space.
            return None;
        }
        let [dx, dy, dz] = transform.position;
        Some(ThreadPreview {
            at: [tp.at[0] + dx, tp.at[1] + dy, tp.at[2] + dz],
            ..tp
        })
    }

    fn drawable_solids(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        pitch: Option<f64>,
    ) -> Vec<Handle> {
        let ids = k.get_sub_shapes(shape, "solid").unwrap_or_default();
        if ids.len() > 1 {
            ids.into_iter().map(id_to_handle).collect()
        } else if ids.len() == 1 {
            let h = id_to_handle(ids[0]);
            // Unify-same-domain on a helical groove can wreck the thread or
            // explode the face count; skip heal when a thread is present.
            if pitch.is_some() {
                vec![h]
            } else {
                let healed = heal_shape(k, h);
                vec![if shape_has_extent(k, healed) {
                    healed
                } else {
                    h
                }]
            }
        } else {
            vec![shape]
        }
    }

    fn tessellate_solid(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        pitch: Option<f64>,
        preview: Option<ThreadPreview>,
    ) -> Result<ModelOutput, KernelError> {
        let solids = drawable_solids(k, shape, pitch);
        let mut meshes = Vec::with_capacity(solids.len());
        let mut metrics = Vec::with_capacity(solids.len());
        for solid in &solids {
            if !shape_has_extent(k, *solid) {
                continue;
            }
            reject_if_planar(k, *solid)?;
            let bbox = k
                .get_bounding_box(*solid, false)
                .ok()
                .map(|b| [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z])
                .unwrap_or([0.0; 6]);
            metrics.push(MetricsData {
                volume: k.get_volume(*solid).unwrap_or(0.0),
                bbox,
                surface_area: k.get_surface_area(*solid).unwrap_or(0.0),
                is_solid: true,
            });
            meshes.push(*solid);
        }
        if meshes.is_empty() {
            return Err(occt_err("tessellate: shape has no drawable solid"));
        }
        let metric_refs: Vec<&MetricsData> = metrics.iter().collect();
        let mut combined = super::combine_metrics(&metric_refs);

        if let Some(tp) = preview {
            if tp.length / tp.pitch.max(1e-9) > MAX_INLINE_THREAD_TURNS {
                match tessellate_thread_segments(k, &meshes, tp) {
                    Ok(mesh) if !mesh.positions.is_empty() => {
                        combined.bbox = bbox_from_positions(&mesh.positions);
                        return Ok(ModelOutput {
                            mesh,
                            metrics: combined,
                        });
                    }
                    Err(e) if is_fatal_occt(&e) => return Err(e),
                    _ => {}
                }
            }
        }

        let mut tess_meshes = Vec::with_capacity(meshes.len());
        for solid in meshes {
            tess_meshes.push(tessellate_shape_mesh(k, solid, pitch)?);
        }
        let mesh_refs: Vec<&MeshData> = tess_meshes.iter().collect();
        Ok(ModelOutput {
            mesh: combine_meshes(&mesh_refs),
            metrics: combined,
        })
    }

    fn bbox_diag(k: &mut occt_wasm::OcctKernel, solid: Handle) -> f64 {
        k.get_bounding_box(solid, false)
            .ok()
            .map(|b| {
                let dx = b.max.x - b.min.x;
                let dy = b.max.y - b.min.y;
                let dz = b.max.z - b.min.z;
                (dx * dx + dy * dy + dz * dz).sqrt()
            })
            .unwrap_or(10.0)
    }

    fn shape_has_extent(k: &mut occt_wasm::OcctKernel, shape: Handle) -> bool {
        k.get_bounding_box(shape, false)
            .ok()
            .map(|b| {
                let dx = (b.max.x - b.min.x).abs();
                let dy = (b.max.y - b.min.y).abs();
                let dz = (b.max.z - b.min.z).abs();
                dx.max(dy).max(dz) > 1e-6
            })
            .unwrap_or(false)
    }

    /// Absolute deflection. Never call tessellate_relative on a long helix.
    fn tessellate_shape_mesh(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        pitch: Option<f64>,
    ) -> Result<MeshData, KernelError> {
        let level = COARSE_LEVEL.with(|c| c.get());
        let diag = bbox_diag(k, solid);
        let linear = preview_linear(pitch, level, diag);
        let angular = if level == 0 { 0.32 } else { 0.55 };
        tessellate_once(k, solid, linear, angular)
    }

    fn preview_linear(pitch: Option<f64>, level: u8, diag: f64) -> f64 {
        match (pitch, level) {
            // ~pitch/12: resolve the 60° V flank without tessellate_relative.
            (Some(p), 0) => (p * 0.08).clamp(0.07, 0.12),
            (Some(p), 1) => (p * 0.14).clamp(0.10, 0.18),
            (Some(_), _) => 0.35,
            (None, 0) => (diag * 0.008).clamp(0.06, 0.25),
            (None, 1) => 0.4,
            (None, _) => 0.8,
        }
    }

    fn tessellate_once(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        linear: f64,
        angular: f64,
    ) -> Result<MeshData, KernelError> {
        let mut mesh = k
            .tessellate(solid, linear, angular)
            .map_err(|e| occt_err(format!("tessellate: {:?}", e)))?;
        if let Ok(bb) = k.get_bounding_box(solid, false) {
            strip_spike_triangles(&mut mesh, &bb);
        }
        Ok(MeshData {
            positions: mesh.positions,
            normals: mesh.normals,
            indices: mesh.indices,
        })
    }

    /// Viewport path for long Z bolts: do not tessellate a 30-turn B-Rep.
    /// Mesh the uncut head+shank, strip the smooth cylinder, instance an
    /// 8-turn V-groove rod (the size we know WASM can mesh) along Z.
    fn tessellate_thread_segments(
        k: &mut occt_wasm::OcctKernel,
        solids: &[Handle],
        tp: ThreadPreview,
    ) -> Result<MeshData, KernelError> {
        let level = COARSE_LEVEL.with(|c| c.get());
        let angular = if level == 0 { 0.32 } else { 0.55 };
        let mut body_parts: Vec<MeshData> = Vec::new();
        for &solid in solids {
            if !shape_has_extent(k, solid) {
                continue;
            }
            let diag = bbox_diag(k, solid);
            let linear = preview_linear(None, level, diag);
            match tessellate_once(k, solid, linear, angular) {
                Ok(mesh) => body_parts.push(mesh),
                Err(e) if is_fatal_occt(&e) => return Err(e),
                Err(_) => {}
            }
        }
        let body = if body_parts.is_empty() {
            MeshData {
                positions: vec![],
                normals: vec![],
                indices: vec![],
            }
        } else {
            let refs: Vec<&MeshData> = body_parts.iter().collect();
            strip_thread_envelope(&combine_meshes(&refs), &tp)
        };

        let seg = (tp.pitch * 6.4).min(tp.length).max(tp.pitch * 2.0);
        let n_full = (tp.length / seg).floor() as i32;
        let rem = tp.length - n_full as f64 * seg;
        let helix_linear = preview_linear(Some(tp.pitch), level, tp.major.max(10.0));

        let proto = threaded_rod(k, tp.major, tp.pitch, seg)?;
        let proto_mesh = tessellate_once(k, proto, helix_linear, angular)?;
        let (pz0, pz1) = mesh_z_range(&proto_mesh);

        let mut parts: Vec<MeshData> = Vec::new();
        if !body.positions.is_empty() {
            parts.push(body);
        }
        let has_tail = rem > tp.pitch * 0.45;
        for i in 0..n_full {
            let z = i as f64 * seg;
            let strip_bottom = i > 0;
            let strip_top = i + 1 < n_full || has_tail;
            let mesh = strip_z_caps(&proto_mesh, pz0, pz1, strip_bottom, strip_top);
            parts.push(place_thread_mesh(&mesh, &tp, z));
        }
        if has_tail {
            let tail = threaded_rod(k, tp.major, tp.pitch, rem)?;
            let tail_mesh = tessellate_once(k, tail, helix_linear, angular)?;
            let (tz0, tz1) = mesh_z_range(&tail_mesh);
            let mesh = strip_z_caps(&tail_mesh, tz0, tz1, n_full > 0, false);
            parts.push(place_thread_mesh(&mesh, &tp, n_full as f64 * seg));
        } else if n_full == 0 {
            return Err(occt_err("thread preview produced no helical segments"));
        }
        if parts.is_empty() {
            return Err(occt_err("thread preview produced no mesh"));
        }
        let refs: Vec<&MeshData> = parts.iter().collect();
        Ok(combine_meshes(&refs))
    }

    fn mesh_z_range(mesh: &MeshData) -> (f32, f32) {
        let mut z0 = f32::MAX;
        let mut z1 = f32::MIN;
        for chunk in mesh.positions.chunks(3) {
            if chunk.len() == 3 {
                z0 = z0.min(chunk[2]);
                z1 = z1.max(chunk[2]);
            }
        }
        if z0 > z1 {
            (0.0, 1.0)
        } else {
            (z0, z1)
        }
    }

    fn strip_thread_envelope(mesh: &MeshData, tp: &ThreadPreview) -> MeshData {
        let r_max = tp.major * 0.5 + tp.pitch * 0.15;
        let r2 = (r_max * r_max) as f32;
        let z0 = tp.at[2] as f32;
        let z1 = (tp.at[2] + tp.length) as f32;
        let cx = tp.at[0] as f32;
        let cy = tp.at[1] as f32;
        let in_env = |x: f32, y: f32, z: f32| {
            z >= z0 - 0.04 && z <= z1 + 0.04 && {
                let dx = x - cx;
                let dy = y - cy;
                dx * dx + dy * dy <= r2
            }
        };
        filter_triangles(mesh, |mesh, a, b, c| {
            let ax = mesh.positions[a * 3];
            let ay = mesh.positions[a * 3 + 1];
            let az = mesh.positions[a * 3 + 2];
            let bx = mesh.positions[b * 3];
            let by = mesh.positions[b * 3 + 1];
            let bz = mesh.positions[b * 3 + 2];
            let cx_ = mesh.positions[c * 3];
            let cy_ = mesh.positions[c * 3 + 1];
            let cz = mesh.positions[c * 3 + 2];
            !in_env((ax + bx + cx_) / 3.0, (ay + by + cy_) / 3.0, (az + bz + cz) / 3.0)
        })
    }

    fn strip_z_caps(
        mesh: &MeshData,
        z0: f32,
        z1: f32,
        strip_bottom: bool,
        strip_top: bool,
    ) -> MeshData {
        if !strip_bottom && !strip_top {
            return mesh.clone();
        }
        let eps = ((z1 - z0).abs() * 0.03).clamp(0.03, 0.12);
        filter_triangles(mesh, |mesh, a, b, c| {
            let za = mesh.positions[a * 3 + 2];
            let zb = mesh.positions[b * 3 + 2];
            let zc = mesh.positions[c * 3 + 2];
            let near_lo = strip_bottom
                && (za - z0).abs() <= eps
                && (zb - z0).abs() <= eps
                && (zc - z0).abs() <= eps;
            let near_hi = strip_top
                && (za - z1).abs() <= eps
                && (zb - z1).abs() <= eps
                && (zc - z1).abs() <= eps;
            if !near_lo && !near_hi {
                return true;
            }
            // Only drop end-cap disks (normal along Z), not helical groove walls.
            let p = |i: usize| {
                [
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ]
            };
            let pa = p(a);
            let pb = p(b);
            let pc = p(c);
            let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
            let nz = e1[0] * e2[1] - e1[1] * e2[0];
            let nx = e1[1] * e2[2] - e1[2] * e2[1];
            let ny = e1[2] * e2[0] - e1[0] * e2[2];
            let len = (nx * nx + ny * ny + nz * nz).sqrt();
            if len < 1e-12 {
                return true;
            }
            (nz / len).abs() < 0.85
        })
    }

    fn filter_triangles(
        mesh: &MeshData,
        keep: impl Fn(&MeshData, usize, usize, usize) -> bool,
    ) -> MeshData {
        let tris: Vec<[u32; 3]> = if mesh.indices.is_empty() {
            (0..mesh.positions.len() / 9)
                .map(|t| {
                    let i = (t * 3) as u32;
                    [i, i + 1, i + 2]
                })
                .collect()
        } else {
            mesh.indices
                .chunks(3)
                .filter_map(|c| {
                    if c.len() == 3 {
                        Some([c[0], c[1], c[2]])
                    } else {
                        None
                    }
                })
                .collect()
        };
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for [a, b, c] in tris {
            let (ai, bi, ci) = (a as usize, b as usize, c as usize);
            if !keep(mesh, ai, bi, ci) {
                continue;
            }
            let base = (positions.len() / 3) as u32;
            for vi in [ai, bi, ci] {
                positions.extend_from_slice(&mesh.positions[vi * 3..vi * 3 + 3]);
                if mesh.normals.len() >= (vi + 1) * 3 {
                    normals.extend_from_slice(&mesh.normals[vi * 3..vi * 3 + 3]);
                } else {
                    normals.extend_from_slice(&[0.0, 0.0, 1.0]);
                }
            }
            indices.extend_from_slice(&[base, base + 1, base + 2]);
        }
        MeshData {
            positions,
            normals,
            indices,
        }
    }

    fn place_thread_mesh(mesh: &MeshData, tp: &ThreadPreview, z_along: f64) -> MeshData {
        let theta = 2.0 * PI * (z_along / tp.pitch.max(1e-9));
        let (ct, st) = (theta.cos() as f32, theta.sin() as f32);
        let (dx, dy, dz) = (tp.at[0] as f32, tp.at[1] as f32, (tp.at[2] + z_along) as f32);
        let mut out = mesh.clone();
        let n = out.positions.len() / 3;
        for i in 0..n {
            let x = out.positions[i * 3];
            let y = out.positions[i * 3 + 1];
            let z = out.positions[i * 3 + 2];
            let x2 = x * ct - y * st;
            let mut y2 = x * st + y * ct;
            if tp.left {
                y2 = -y2;
            }
            out.positions[i * 3] = x2 + dx;
            out.positions[i * 3 + 1] = y2 + dy;
            out.positions[i * 3 + 2] = z + dz;
            if out.normals.len() >= (i + 1) * 3 {
                let nx = out.normals[i * 3];
                let ny = out.normals[i * 3 + 1];
                let nz = out.normals[i * 3 + 2];
                let nx2 = nx * ct - ny * st;
                let mut ny2 = nx * st + ny * ct;
                if tp.left {
                    ny2 = -ny2;
                }
                out.normals[i * 3] = nx2;
                out.normals[i * 3 + 1] = ny2;
                out.normals[i * 3 + 2] = nz;
            }
        }
        out
    }

    fn apply_body_transform(
        k: &mut occt_wasm::OcctKernel,
        mut solid: Handle,
        transform: &crate::ir::BodyTransform,
    ) -> Result<Handle, KernelError> {
        let [rx, ry, rz] = transform.rotation;
        if rx.abs() > 1e-9 {
            solid = rotate_shape(
                k,
                solid,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                rx * PI / 180.0,
                "body rotate X",
            )?;
        }
        if ry.abs() > 1e-9 {
            solid = rotate_shape(
                k,
                solid,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                ry * PI / 180.0,
                "body rotate Y",
            )?;
        }
        if rz.abs() > 1e-9 {
            solid = rotate_shape(
                k,
                solid,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                rz * PI / 180.0,
                "body rotate Z",
            )?;
        }
        translate_if_needed(k, solid, transform.position)
    }

    pub fn execute_document_with_occt(
        document: &CadDocument,
    ) -> Result<DocumentOutput, KernelError> {
        with_kernel_retry(|| execute_document_inner(document))
    }

    fn execute_document_inner(document: &CadDocument) -> Result<DocumentOutput, KernelError> {
        with_kernel(|k| {
            STEP_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let mut solids: HashMap<String, Handle> = HashMap::new();
                let mut consumed: HashSet<String> = HashSet::new();

                for body in &document.bodies {
                    if body.suppressed {
                        continue;
                    }
                    let prog = CadProgram {
                        units: document.units.clone(),
                        features: body.features.clone(),
                    };
                    let solid =
                        execute_in_kernel(k, &prog, &mut cache, body_cache_ns(&body.body_id))?;
                    let solid = drawable_shape(k, solid);
                    let solid = apply_body_transform(k, solid, &body.transform)?;
                    solids.insert(body.body_id.clone(), solid);
                }

                for body in &document.bodies {
                    if body.suppressed {
                        continue;
                    }
                    let Some(tool) = solids.get(&body.body_id).copied() else {
                        continue;
                    };
                    for r in &body.references {
                        let target = solids.get_mut(&r.target).ok_or_else(|| {
                            KernelError::InvalidState(format!(
                                "body '{}' references unknown target '{}'",
                                body.body_id, r.target
                            ))
                        })?;
                        let raw = match r.op {
                            BodyRefOp::Cut => k
                                .cut(*target, tool)
                                .map_err(|e| occt_err(format!("body cut: {:?}", e)))?,
                            BodyRefOp::Fuse => fuse_robust(k, *target, tool, "body fuse")?,
                        };
                        *target = match r.op {
                            BodyRefOp::Cut => drawable_shape(k, raw),
                            BodyRefOp::Fuse => raw,
                        };
                        if r.consume {
                            consumed.insert(body.body_id.clone());
                        }
                    }
                }

                let mut bodies = Vec::new();
                for body in &document.bodies {
                    if body.suppressed {
                        continue;
                    }
                    if consumed.contains(&body.body_id) {
                        continue;
                    }
                    let Some(&solid) = solids.get(&body.body_id) else {
                        continue;
                    };
                    let pitch = thread_pitch_hint(&body.features, &document.units);
                    let preview = long_thread_preview(&body.features, &document.units)
                        .and_then(|tp| preview_with_translation(tp, &body.transform));
                    let out = tessellate_solid(k, solid, pitch, preview)?;
                    bodies.push(BodyOutput {
                        body_id: body.body_id.clone(),
                        name: body.display_name().to_string(),
                        visible: body.visible,
                        suppressed: false,
                        mesh: out.mesh,
                        metrics: out.metrics,
                    });
                }
                Ok(document_output_from_bodies(bodies))
            })
        })
    }

    pub fn export_document_with_occt(
        document: &CadDocument,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        with_kernel_retry(|| export_document_inner(document, format))
    }

    fn export_document_inner(
        document: &CadDocument,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        with_kernel(|k| {
            STEP_CACHE.with(|c| {
                let mut cache = c.borrow_mut();
                let mut combined: Option<Handle> = None;
                for body in &document.bodies {
                    if body.suppressed || !body.visible {
                        continue;
                    }
                    let prog = CadProgram {
                        units: document.units.clone(),
                        features: body.features.clone(),
                    };
                    let solid =
                        execute_in_kernel(k, &prog, &mut cache, body_cache_ns(&body.body_id))?;
                    let solid = drawable_shape(k, solid);
                    let solid = apply_body_transform(k, solid, &body.transform)?;
                    combined = Some(match combined {
                        None => solid,
                        Some(acc) => fuse_robust(k, acc, solid, "export fuse")?,
                    });
                }
                let solid = combined
                    .ok_or_else(|| KernelError::InvalidState("nothing visible to export".into()))?;
                let pitch = document
                    .bodies
                    .iter()
                    .filter_map(|b| thread_pitch_hint(&b.features, &document.units))
                    .fold(None, |acc, p| Some(acc.map(|a: f64| a.min(p)).unwrap_or(p)));
                let visible: Vec<_> = document
                    .bodies
                    .iter()
                    .filter(|b| !b.suppressed && b.visible)
                    .collect();
                let preview = if visible.len() == 1 {
                    long_thread_preview(&visible[0].features, &document.units)
                        .and_then(|tp| preview_with_translation(tp, &visible[0].transform))
                } else {
                    None
                };
                match format {
                    ExportFormat::Step => {
                        let s = heal_shape(k, solid);
                        k.export_step(s)
                            .map(|s| s.into_bytes())
                            .map_err(|e| occt_err(format!("export_step: {:?}", e)))
                    }
                    ExportFormat::Stl => {
                        let out = tessellate_solid(k, solid, pitch, preview)?;
                        Ok(crate::export::to_stl(&out.mesh))
                    }
                    ExportFormat::Gltf => {
                        let out = tessellate_solid(k, solid, pitch, preview)?;
                        Ok(mesh_to_glb(&mesh_data_as_occt(&out.mesh)))
                    }
                    ExportFormat::Obj => {
                        let out = tessellate_solid(k, solid, pitch, preview)?;
                        Ok(crate::export::to_obj(&out.mesh).into_bytes())
                    }
                    ExportFormat::Brep => {
                        let s = heal_shape(k, solid);
                        k.to_brep(s)
                            .map(|s| s.into_bytes())
                            .map_err(|e| occt_err(format!("to_brep: {:?}", e)))
                    }
                }
            })
        })
    }

    pub fn export_with_occt(
        program: &CadProgram,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        with_kernel_retry(|| export_program_inner(program, format))
    }

    fn export_program_inner(
        program: &CadProgram,
        format: &ExportFormat,
    ) -> Result<Vec<u8>, KernelError> {
        with_kernel(|k| {
            let solid =
                STEP_CACHE.with(|c| execute_in_kernel(k, program, &mut c.borrow_mut(), 0))?;
            let pitch = thread_pitch_hint(&program.features, &program.units);
            let preview = long_thread_preview(&program.features, &program.units);
            match format {
                ExportFormat::Step => {
                    let s = heal_shape(k, solid);
                    k.export_step(s)
                        .map(|s| s.into_bytes())
                        .map_err(|e| occt_err(format!("export_step: {:?}", e)))
                }
                ExportFormat::Stl => {
                    let out = tessellate_solid(k, solid, pitch, preview)?;
                    Ok(crate::export::to_stl(&out.mesh))
                }
                ExportFormat::Gltf => {
                    let out = tessellate_solid(k, solid, pitch, preview)?;
                    Ok(mesh_to_glb(&mesh_data_as_occt(&out.mesh)))
                }
                ExportFormat::Obj => {
                    let out = tessellate_solid(k, solid, pitch, preview)?;
                    Ok(crate::export::to_obj(&out.mesh).into_bytes())
                }
                ExportFormat::Brep => {
                    let s = heal_shape(k, solid);
                    k.to_brep(s)
                        .map(|s| s.into_bytes())
                        .map_err(|e| occt_err(format!("to_brep: {:?}", e)))
                }
            }
        })
    }

    fn mesh_data_as_occt(mesh: &MeshData) -> occt_wasm::Mesh {
        occt_wasm::Mesh {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            indices: mesh.indices.clone(),
            face_groups: Vec::new(),
        }
    }

    /// Build a minimal binary glTF (GLB 2.0) from a tessellated mesh.
    fn mesh_to_glb(mesh: &occt_wasm::Mesh) -> Vec<u8> {
        let vertex_count = mesh.positions.len() / 3;
        let index_count = mesh.indices.len();

        let pos_bytes: Vec<u8> = mesh
            .positions
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let nrm_bytes: Vec<u8> = mesh.normals.iter().flat_map(|f| f.to_le_bytes()).collect();
        let idx_bytes: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();

        let align4 = |mut v: Vec<u8>| {
            let pad = (4 - v.len() % 4) % 4;
            v.extend(std::iter::repeat(0u8).take(pad));
            v
        };
        let pos_bytes = align4(pos_bytes);
        let nrm_bytes = align4(nrm_bytes);
        let idx_bytes = align4(idx_bytes);
        let pos_len = pos_bytes.len();
        let nrm_len = nrm_bytes.len();
        let idx_len = idx_bytes.len();
        let nrm_offset = pos_len;
        let idx_offset = pos_len + nrm_len;
        let bin_len = pos_len + nrm_len + idx_len;

        let (mut mn_x, mut mn_y, mut mn_z) = (f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let (mut mx_x, mut mx_y, mut mx_z) =
            (f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for chunk in mesh.positions.chunks(3) {
            if chunk.len() == 3 {
                mn_x = mn_x.min(chunk[0]);
                mx_x = mx_x.max(chunk[0]);
                mn_y = mn_y.min(chunk[1]);
                mx_y = mx_y.max(chunk[1]);
                mn_z = mn_z.min(chunk[2]);
                mx_z = mx_z.max(chunk[2]);
            }
        }

        let json = format!(
            concat!(
                r#"{{"asset":{{"version":"2.0","generator":"AgentCAD"}},"scene":0,"scenes":[{{"nodes":[0]}}],"#,
                r#""nodes":[{{"mesh":0}}],"meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1}},"indices":2,"mode":4}}]}}],"#,
                r#""accessors":["#,
                r#"{{"bufferView":0,"componentType":5126,"count":{vc},"type":"VEC3","min":[{mn_x},{mn_y},{mn_z}],"max":[{mx_x},{mx_y},{mx_z}]}},"#,
                r#"{{"bufferView":1,"componentType":5126,"count":{vc},"type":"VEC3"}},"#,
                r#"{{"bufferView":2,"componentType":5125,"count":{ic},"type":"SCALAR"}}"#,
                r#"],"bufferViews":["#,
                r#"{{"buffer":0,"byteOffset":0,"byteLength":{pl}}},"#,
                r#"{{"buffer":0,"byteOffset":{no},"byteLength":{nl}}},"#,
                r#"{{"buffer":0,"byteOffset":{io},"byteLength":{il}}}"#,
                r#"],"buffers":[{{"byteLength":{bl}}}]}}"#,
            ),
            vc = vertex_count,
            ic = index_count,
            mn_x = mn_x,
            mn_y = mn_y,
            mn_z = mn_z,
            mx_x = mx_x,
            mx_y = mx_y,
            mx_z = mx_z,
            pl = pos_len,
            no = nrm_offset,
            nl = nrm_len,
            io = idx_offset,
            il = idx_len,
            bl = bin_len,
        );

        let json_bytes = json.into_bytes();
        let json_pad = (4 - json_bytes.len() % 4) % 4;
        let mut json_chunk = json_bytes;
        json_chunk.extend(std::iter::repeat(b' ').take(json_pad));

        let total_len = 12 + 8 + json_chunk.len() + 8 + bin_len;
        let mut glb = Vec::with_capacity(total_len);
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(total_len as u32).to_le_bytes());
        glb.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json_chunk);
        glb.extend_from_slice(&(bin_len as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&pos_bytes);
        glb.extend_from_slice(&nrm_bytes);
        glb.extend_from_slice(&idx_bytes);
        glb
    }

    /// Runs the feature list with incremental caching and returns the final solid handle.
    ///
    /// The OCCT WASM arena is persistent (thread-local). Each step's result is stored
    /// under a cumulative feature hash. On the next call, unchanged leading steps are
    /// skipped by restoring the cached state — only the first changed step and everything
    /// after it is re-executed. For a fillet radius tweak this means ≈1 OCCT op instead
    /// of the whole sequence.
    fn execute_in_kernel(
        k: &mut occt_wasm::OcctKernel,
        program: &CadProgram,
        cache: &mut HashMap<u64, StepEntry>,
        cache_ns: u64,
    ) -> Result<Handle, KernelError> {
        let hashes = step_hashes(&program.features, cache_ns);

        // Find the last consecutive cache hit from the beginning.
        let mut state = ExecState::default();
        let mut resume_from = 0usize;
        for (i, &h) in hashes.iter().enumerate() {
            if let Some(e) = cache.get(&h) {
                state.current_face = e.face.map(id_to_handle);
                state.current_solid = e.solid.map(id_to_handle);
                state.active_plane = e.plane.clone();
                state.last_tool = e.last_tool.map(id_to_handle);
                state.last_boolean = e.last_boolean;
                state.face_normal = e.face_normal;
                state.base_before_face_sketch = e.base_before_face_sketch.map(id_to_handle);
                resume_from = i + 1;
            } else {
                break;
            }
        }

        // Execute only the uncached suffix.
        for i in resume_from..program.features.len() {
            match &program.features[i] {
                Feature::Sketch(op) => handle_sketch(k, &mut state, op)?,
                Feature::Extrude(op) => handle_extrude(k, &mut state, op)?,
                Feature::Revolve(op) => handle_revolve(k, &mut state, op)?,
                Feature::Cut(op) => handle_cut(k, &mut state, op)?,
                Feature::Fuse(op) => handle_fuse(k, &mut state, op)?,
                Feature::Common(op) => handle_common(k, &mut state, op)?,
                Feature::Hole(op) => handle_hole(k, &mut state, op)?,
                Feature::Fillet(op) => handle_fillet(k, &mut state, op)?,
                Feature::Chamfer(op) => handle_chamfer(k, &mut state, op)?,
                Feature::Transform(op) => handle_transform(k, &mut state, op)?,
                Feature::Box(op) => handle_box(k, &mut state, op)?,
                Feature::Cylinder(op) => handle_cylinder(k, &mut state, op)?,
                Feature::Sphere(op) => handle_sphere(k, &mut state, op)?,
                Feature::Cone(op) => handle_cone(k, &mut state, op)?,
                Feature::Torus(op) => handle_torus(k, &mut state, op)?,
                Feature::Loft(op) => handle_loft(k, &mut state, op)?,
                Feature::Mirror(op) => handle_mirror(k, &mut state, op)?,
                Feature::Pattern(op) => handle_pattern(k, &mut state, op)?,
                Feature::Shell(op) => handle_shell(k, &mut state, op)?,
                Feature::DraftExtrude(op) => handle_draft_extrude(k, &mut state, op)?,
                Feature::Thread(op) => handle_thread(k, &mut state, op, &program.units)?,
                Feature::Sweep(op) => handle_sweep(k, &mut state, op)?,
                Feature::Pipe(op) => handle_pipe(k, &mut state, op)?,
                Feature::Helix(op) => handle_helix(k, &mut state, op)?,
                Feature::Offset(op) => handle_offset(k, &mut state, op)?,
                Feature::Thicken(op) => handle_thicken(k, &mut state, op)?,
                Feature::Ellipsoid(op) => handle_ellipsoid(k, &mut state, op)?,
                Feature::Draft(op) => handle_draft(k, &mut state, op)?,
            }

            // Evict oldest entry when the cache is full.
            if cache.len() >= CACHE_LIMIT {
                if let Some(&oldest) = cache.keys().next() {
                    cache.remove(&oldest);
                }
            }
            cache.insert(
                hashes[i],
                StepEntry {
                    face: state.current_face.map(handle_to_id),
                    solid: state.current_solid.map(handle_to_id),
                    plane: state.active_plane.clone(),
                    last_tool: state.last_tool.map(handle_to_id),
                    last_boolean: state.last_boolean,
                    face_normal: state.face_normal,
                    base_before_face_sketch: state.base_before_face_sketch.map(handle_to_id),
                },
            );
        }

        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("Program produced no solid".into()))?;
        let solid = coalesce_solids(k, solid)?;
        reject_if_planar(k, solid)?;
        Ok(solid)
    }

    // ── Feature handlers ──────────────────────────────────────────────────────

    fn handle_sketch(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &SketchOp,
    ) -> Result<(), KernelError> {
        let face = make_profile_face(k, &op.profile, op.origin)?;

        if let Some(ref face_ref) = op.face {
            let solid = state.current_solid.ok_or_else(|| {
                KernelError::InvalidState("sketch on face requires an existing solid".into())
            })?;
            let frame = resolve_face_frame(k, solid, face_ref)?;
            let placed = place_xy_shape_on_frame(k, face, &frame)?;
            state.base_before_face_sketch = Some(solid);
            state.face_normal = Some(frame.normal);
            state.current_face = Some(placed);
            state.active_plane = dominant_plane(&frame.normal);
        } else {
            // Always build the profile on XY. The plane is applied after the 3-D
            // operation (extrude/revolve/draft) so OCCT never has to prism a
            // face along +Y/+X — that path produced perpendicular "cross" solids.
            state.current_face = Some(face);
            state.active_plane = op.plane.clone();
            state.face_normal = None;
            state.base_before_face_sketch = None;
        }
        Ok(())
    }

    fn handle_extrude(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ExtrudeOp,
    ) -> Result<(), KernelError> {
        let face = state.current_face.ok_or_else(|| {
            KernelError::InvalidState("extrude requires a preceding sketch".into())
        })?;

        let solid = if let Some(n) = state.face_normal {
            let (dx, dy, dz) = if op.symmetric {
                (n[0] * op.depth, n[1] * op.depth, n[2] * op.depth)
            } else {
                (n[0] * op.depth, n[1] * op.depth, n[2] * op.depth)
            };
            let mut solid = k
                .extrude(face, dx, dy, dz)
                .map_err(|e| occt_err(format!("extrude on face: {:?}", e)))?;
            if op.symmetric {
                solid = k
                    .translate(
                        solid,
                        -n[0] * op.depth / 2.0,
                        -n[1] * op.depth / 2.0,
                        -n[2] * op.depth / 2.0,
                    )
                    .map_err(|e| occt_err(format!("extrude symmetric translate: {:?}", e)))?;
            }
            if let Some(base) = state.base_before_face_sketch {
                fuse_robust(k, base, solid, "fuse face extrude")?
            } else {
                solid
            }
        } else {
            // Prism along +Z (the plane the face actually lives on), then rotate
            // the solid onto the requested construction plane.
            let solid = k
                .extrude(face, 0.0, 0.0, op.depth)
                .map_err(|e| occt_err(format!("extrude: {:?}", e)))?;

            let solid = if op.symmetric {
                k.translate(solid, 0.0, 0.0, -op.depth / 2.0)
                    .map_err(|e| occt_err(format!("extrude symmetric translate: {:?}", e)))?
            } else {
                solid
            };

            rotate_to_plane(k, solid, &state.active_plane)?
        };

        if state.base_before_face_sketch.take().is_some() {
            // Face-sketch extrude already fused with the prior solid.
            set_solid(state, solid);
        } else {
            join_or_set(k, state, solid)?;
        }
        state.current_face = None;
        state.face_normal = None;
        Ok(())
    }

    fn handle_revolve(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &RevolveOp,
    ) -> Result<(), KernelError> {
        let face = state.current_face.ok_or_else(|| {
            KernelError::InvalidState("revolve requires a preceding sketch".into())
        })?;

        let (dx, dy, dz) = axis_dir(&op.axis);
        let n = plane_normal(&state.active_plane);
        if (dx * n[0] + dy * n[1] + dz * n[2]).abs() > 0.99 {
            return Err(KernelError::InvalidState(format!(
                "revolve axis {:?} is perpendicular to sketch plane {:?} — that spins the \
                 profile in its own plane and produces a flat disk. For a tube/vase/venturi \
                 standing on XY, sketch the half-section on XZ as (radius, height) and revolve \
                 around Z (an axis that lies IN the sketch plane).",
                op.axis, state.active_plane
            )));
        }

        // Sketches are stored on XY. Put the face onto the construction plane
        // (so UV → world XZ/YZ correctly), THEN revolve around the world axis.
        // Do NOT revolve on XY and rotate_to_plane afterwards: revolving an XY
        // face around Z is exactly the degenerate disk the user saw.
        let face = place_sketch_on_plane(k, face, &state.active_plane)?;

        let angle_rad = op.angle * PI / 180.0;
        let [px, py, pz] = op.origin;

        let solid = k
            .revolve(face, px, py, pz, dx, dy, dz, angle_rad)
            .map_err(|e| occt_err(format!("revolve: {:?}", e)))?;

        join_or_set(k, state, solid)
    }

    fn handle_cut(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &CutOp,
    ) -> Result<(), KernelError> {
        let base = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("cut requires an existing solid".into()))?;

        let tool = if let Some(ref face_ref) = op.face {
            make_tool_on_face(k, base, face_ref, &op.profile, op.depth, op.at, op.through)?
        } else {
            make_tool_solid(
                k,
                &op.profile,
                op.depth,
                op.at,
                &op.plane,
                op.through,
                Some(base),
            )?
        };
        state.last_tool = Some(tool);
        state.last_boolean = Some(LastBoolean::Cut);
        let raw = k
            .cut(base, tool)
            .map_err(|e| occt_err(format!("cut: {:?}", e)))?;
        state.current_solid = Some(drawable_shape(k, raw));
        Ok(())
    }

    fn handle_fuse(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &FuseOp,
    ) -> Result<(), KernelError> {
        if let Some(base) = state.current_solid {
            let addend = if let Some(ref face_ref) = op.face {
                make_tool_on_face(k, base, face_ref, &op.profile, op.depth, op.at, false)?
            } else {
                make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, false, None)?
            };
            state.last_tool = Some(addend);
            state.last_boolean = Some(LastBoolean::Fuse);
            state.current_solid = Some(fuse_robust(k, base, addend, "fuse")?);
            Ok(())
        } else {
            // First feature on the body: extruded profile becomes the solid.
            let addend = make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, false, None)?;
            join_or_set(k, state, addend)
        }
    }

    fn handle_common(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &CommonOp,
    ) -> Result<(), KernelError> {
        let base = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("common requires an existing solid".into()))?;

        let tool = if let Some(ref face_ref) = op.face {
            make_tool_on_face(k, base, face_ref, &op.profile, op.depth, op.at, false)?
        } else {
            make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, false, None)?
        };
        let raw = k
            .common(base, tool)
            .map_err(|e| occt_err(format!("common: {:?}", e)))?;
        let solid = unwrap_to_solid(k, raw);
        state.current_solid = Some(heal_shape(k, solid));
        state.last_tool = None;
        state.last_boolean = None;
        Ok(())
    }

    fn handle_hole(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &HoleOp,
    ) -> Result<(), KernelError> {
        let base = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("hole requires an existing solid".into()))?;

        let radius = op.diameter / 2.0;
        let cyl = if let Some(ref face_ref) = op.face {
            let frame = resolve_face_frame(k, base, face_ref)?;
            let (length, z0) = cutter_span(k, Some(base), op.depth, op.through);
            let cyl = k
                .make_cylinder(radius, length)
                .map_err(|e| occt_err(format!("make_cylinder (hole): {:?}", e)))?;
            let cyl = k
                .translate(cyl, 0.0, 0.0, z0)
                .map_err(|e| occt_err(format!("translate hole: {:?}", e)))?;
            let u = op.center[0];
            let v = op.center[1];
            let ox = frame.origin[0] + frame.x_dir[0] * u + frame.y_dir[0] * v;
            let oy = frame.origin[1] + frame.x_dir[1] * u + frame.y_dir[1] * v;
            let oz = frame.origin[2] + frame.x_dir[2] * u + frame.y_dir[2] * v;
            let cyl = rotate_z_to_dir(k, cyl, frame.normal)?;
            k.translate(cyl, ox, oy, oz)
                .map_err(|e| occt_err(format!("place hole on face: {:?}", e)))?
        } else {
            let (length, z0) = cutter_span(k, Some(base), op.depth, op.through);
            let cyl = k
                .make_cylinder(radius, length)
                .map_err(|e| occt_err(format!("make_cylinder (hole): {:?}", e)))?;

            // Built along +Z starting at 0. Shift so it covers both sides of the
            // profile plane, then rotate onto the hole's plane.
            let [cx, cy] = map_uv(&op.plane, op.center[0], op.center[1]);
            let cyl = k
                .translate(cyl, cx, cy, z0)
                .map_err(|e| occt_err(format!("translate hole: {:?}", e)))?;
            rotate_to_plane(k, cyl, &op.plane)?
        };

        state.last_tool = Some(cyl);
        state.last_boolean = Some(LastBoolean::Cut);
        let raw = k
            .cut(base, cyl)
            .map_err(|e| occt_err(format!("cut (hole): {:?}", e)))?;
        state.current_solid = Some(drawable_shape(k, raw));
        Ok(())
    }

    fn handle_fillet(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &FilletOp,
    ) -> Result<(), KernelError> {
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("fillet requires an existing solid".into()))?;
        let solid = unwrap_to_solid(k, solid);

        let edge_ids = k
            .get_sub_shapes(solid, "edge")
            .map_err(|e| occt_err(format!("get_sub_shapes (fillet): {:?}", e)))?;

        if edge_ids.is_empty() {
            return Ok(());
        }

        let candidate_ids: Vec<u32> = match &op.edges {
            crate::ir::EdgeSelection::Named(name) => {
                filter_edges_by_name(k, solid, &edge_ids, name, op.radius)
            }
            crate::ir::EdgeSelection::Indices(idxs) => idxs
                .iter()
                .filter_map(|&i| edge_ids.get(i).copied())
                .collect(),
        };

        let straight_ids = filter_to_line_edges(k, candidate_ids.clone());
        let pool = if !straight_ids.is_empty() {
            straight_ids
        } else {
            candidate_ids
        };
        let selected = select_blend_edges(k, solid, pool, op.radius);

        let result = try_fillet(k, solid, &selected, op.radius)
            .or_else(|| {
                let outer = longest_edges(k, &selected, 4);
                try_fillet(k, solid, &outer, op.radius)
            })
            .or_else(|| try_fillet(k, solid, &selected, op.radius * 0.6));

        match result {
            Some(shape) => state.current_solid = Some(shape),
            None => eprintln!(
                "[AgentCAD] fillet degraded gracefully (radius {:.3} on {} edges)",
                op.radius,
                selected.len()
            ),
        }
        Ok(())
    }

    fn handle_chamfer(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ChamferOp,
    ) -> Result<(), KernelError> {
        let solid = state.current_solid.ok_or_else(|| {
            KernelError::InvalidState("chamfer requires an existing solid".into())
        })?;
        let solid = unwrap_to_solid(k, solid);

        let edge_ids = k
            .get_sub_shapes(solid, "edge")
            .map_err(|e| occt_err(format!("get_sub_shapes (chamfer): {:?}", e)))?;

        if edge_ids.is_empty() {
            return Ok(());
        }

        let candidate_ids: Vec<u32> = match &op.edges {
            crate::ir::EdgeSelection::Named(name) => {
                filter_edges_by_name(k, solid, &edge_ids, name, op.distance)
            }
            crate::ir::EdgeSelection::Indices(idxs) => idxs
                .iter()
                .filter_map(|&i| edge_ids.get(i).copied())
                .collect(),
        };

        let straight_ids = filter_to_line_edges(k, candidate_ids.clone());
        let pool = if !straight_ids.is_empty() {
            straight_ids
        } else {
            candidate_ids
        };
        let selected = select_blend_edges(k, solid, pool, op.distance);

        let result = try_chamfer(k, solid, &selected, op.distance, op.angle)
            .or_else(|| {
                let outer = longest_edges(k, &selected, 4);
                try_chamfer(k, solid, &outer, op.distance, op.angle)
            })
            .or_else(|| try_chamfer(k, solid, &selected, op.distance * 0.6, op.angle));

        match result {
            Some(shape) => state.current_solid = Some(shape),
            None => eprintln!(
                "[AgentCAD] chamfer degraded gracefully (distance {:.3} on {} edges)",
                op.distance,
                selected.len()
            ),
        }
        Ok(())
    }

    fn handle_transform(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &TransformOp,
    ) -> Result<(), KernelError> {
        let mut shape = state.current_solid.ok_or_else(|| {
            KernelError::InvalidState("transform requires an existing solid".into())
        })?;

        if let Some([tx, ty, tz]) = op.translate {
            shape = k
                .translate(shape, tx, ty, tz)
                .map_err(|e| occt_err(format!("translate: {:?}", e)))?;
        }
        if let Some(r) = &op.rotate {
            let angle_rad = r.angle * PI / 180.0;
            let [px, py, pz] = r.origin;
            let [dx, dy, dz] = r.axis;
            shape = rotate_shape(k, shape, px, py, pz, dx, dy, dz, angle_rad, "rotate")?;
        }
        if let Some(s) = op.scale {
            shape = k
                .scale(shape, 0.0, 0.0, 0.0, s)
                .map_err(|e| occt_err(format!("scale: {:?}", e)))?;
        }

        state.current_solid = Some(shape);
        Ok(())
    }

    fn handle_box(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &BoxOp,
    ) -> Result<(), KernelError> {
        let [dx, dy, dz] = op.size;
        let solid = k
            .make_box(dx, dy, dz)
            .map_err(|e| occt_err(format!("make_box: {:?}", e)))?;
        let mut tx = op.at[0];
        let mut ty = op.at[1];
        let tz = op.at[2];
        if op.centered {
            tx -= dx / 2.0;
            ty -= dy / 2.0;
        }
        let solid = translate_if_needed(k, solid, [tx, ty, tz])?;
        join_or_set(k, state, solid)
    }

    fn handle_cylinder(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &CylinderOp,
    ) -> Result<(), KernelError> {
        let mut solid = k
            .make_cylinder(op.diameter / 2.0, op.height)
            .map_err(|e| occt_err(format!("make_cylinder: {:?}", e)))?;
        solid = align_z_primitive_to_axis(k, solid, &op.axis)?;
        solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn handle_sphere(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &SphereOp,
    ) -> Result<(), KernelError> {
        let solid = k
            .make_sphere(op.diameter / 2.0)
            .map_err(|e| occt_err(format!("make_sphere: {:?}", e)))?;
        let solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn handle_cone(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ConeOp,
    ) -> Result<(), KernelError> {
        let solid = k
            .make_cone(op.d1 / 2.0, op.d2 / 2.0, op.height)
            .map_err(|e| occt_err(format!("make_cone: {:?}", e)))?;
        let solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn handle_torus(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &TorusOp,
    ) -> Result<(), KernelError> {
        let solid = k
            .make_torus(op.major, op.minor)
            .map_err(|e| occt_err(format!("make_torus: {:?}", e)))?;
        let solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn handle_loft(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &LoftOp,
    ) -> Result<(), KernelError> {
        let mut wires: Vec<Handle> = Vec::with_capacity(op.sections.len());
        for (i, sec) in op.sections.iter().enumerate() {
            let face = make_profile_face(k, &sec.profile, [0.0, 0.0])?;
            let wire = k
                .outer_wire(face)
                .map_err(|e| occt_err(format!("loft section {i} outer_wire: {:?}", e)))?;
            let wire = translate_if_needed(k, wire, sec.at)?;
            wires.push(wire);
        }

        let solid = if let Some(apex) = op.apex {
            let vertex = k
                .make_vertex(apex[0], apex[1], apex[2])
                .map_err(|e| occt_err(format!("loft apex vertex: {:?}", e)))?;
            if wires.is_empty() {
                return Err(KernelError::InvalidState(
                    "loft with apex still needs at least one section".into(),
                ));
            }
            // No start vertex: pass a null shape. If that fails, loft the
            // sections then the caller still has a frustum.
            let start = k.make_null_shape().unwrap_or(vertex);
            k.loft_with_vertices(&wires, true, op.ruled, start, vertex)
                .or_else(|_| k.loft(&wires, true, op.ruled))
                .map_err(|e| occt_err(format!("loft: {:?}", e)))?
        } else {
            k.loft(&wires, true, op.ruled)
                .map_err(|e| occt_err(format!("loft: {:?}", e)))?
        };

        let solid = unwrap_to_solid(k, solid);
        join_or_set(k, state, solid)
    }

    fn handle_mirror(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &MirrorOp,
    ) -> Result<(), KernelError> {
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("mirror requires an existing solid".into()))?;
        let [px, py, pz] = op.origin;
        let n = plane_normal(&op.plane);
        let mirrored = k
            .mirror(solid, px, py, pz, n[0], n[1], n[2])
            .map_err(|e| occt_err(format!("mirror: {:?}", e)))?;
        let result = if op.fuse {
            let raw = k
                .fuse(solid, mirrored)
                .map_err(|e| occt_err(format!("mirror fuse: {:?}", e)))?;
            unwrap_to_solid(k, raw)
        } else {
            mirrored
        };
        set_solid(state, result);
        Ok(())
    }

    fn handle_pattern(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &PatternOp,
    ) -> Result<(), KernelError> {
        let solid = state.current_solid.ok_or_else(|| {
            KernelError::InvalidState("pattern requires an existing solid".into())
        })?;
        let count = op.count as i32;

        if matches!(op.scope, PatternScope::Feature) {
            let tool = state.last_tool.ok_or_else(|| {
                KernelError::InvalidState(
                    "pattern scope=feature requires a preceding cut/fuse/hole".into(),
                )
            })?;
            let mode = state.last_boolean.unwrap_or(LastBoolean::Cut);
            let mut result = solid;
            // Instance 0 is already in the solid; apply 1..count-1.
            for i in 1..op.count {
                let instance = match op.kind {
                    PatternKind::Linear => {
                        let [dx, dy, dz] = op.direction.unwrap_or([1.0, 0.0, 0.0]);
                        let spacing = op.spacing.unwrap_or(1.0);
                        let t = i as f64 * spacing;
                        k.translate(tool, dx * t, dy * t, dz * t)
                            .map_err(|e| occt_err(format!("pattern feature translate: {:?}", e)))?
                    }
                    PatternKind::Circular => {
                        let [cx, cy, cz] = op.center;
                        let axis = op.axis.clone().unwrap_or(RevolveAxis::Z);
                        let (ax, ay, az) = axis_dir(&axis);
                        let angle_deg = op.angle.unwrap_or(360.0 / op.count as f64);
                        let angle_rad = (i as f64) * angle_deg * PI / 180.0;
                        k.rotate(tool, cx, cy, cz, ax, ay, az, angle_rad)
                            .map_err(|e| occt_err(format!("pattern feature rotate: {:?}", e)))?
                    }
                };
                let raw = match mode {
                    LastBoolean::Cut => k
                        .cut(result, instance)
                        .map_err(|e| occt_err(format!("pattern feature cut: {:?}", e)))?,
                    LastBoolean::Fuse => k
                        .fuse(result, instance)
                        .map_err(|e| occt_err(format!("pattern feature fuse: {:?}", e)))?,
                };
                result = unwrap_to_solid(k, raw);
            }
            set_solid(state, result);
            return Ok(());
        }

        let result = match op.kind {
            PatternKind::Linear => {
                let [dx, dy, dz] = op.direction.unwrap_or([1.0, 0.0, 0.0]);
                let spacing = op.spacing.unwrap_or(1.0);
                k.linear_pattern(solid, dx, dy, dz, spacing, count)
                    .map_err(|e| occt_err(format!("linear_pattern: {:?}", e)))?
            }
            PatternKind::Circular => {
                let [cx, cy, cz] = op.center;
                let axis = op.axis.clone().unwrap_or(RevolveAxis::Z);
                let (ax, ay, az) = match axis {
                    RevolveAxis::X => (1.0, 0.0, 0.0),
                    RevolveAxis::Y => (0.0, 1.0, 0.0),
                    RevolveAxis::Z => (0.0, 0.0, 1.0),
                };
                let angle_deg = op.angle.unwrap_or(360.0 / op.count as f64);
                let angle_rad = angle_deg * PI / 180.0;
                k.circular_pattern(solid, cx, cy, cz, ax, ay, az, angle_rad, count)
                    .map_err(|e| occt_err(format!("circular_pattern: {:?}", e)))?
            }
        };
        set_solid(state, unwrap_to_solid(k, result));
        Ok(())
    }

    fn handle_shell(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ShellOp,
    ) -> Result<(), KernelError> {
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("shell requires an existing solid".into()))?;
        let face_ids = k
            .get_sub_shapes(solid, "face")
            .map_err(|e| occt_err(format!("get_sub_shapes (shell): {:?}", e)))?;
        let open: Vec<occt_wasm::ShapeHandle> = match &op.faces {
            crate::ir::EdgeSelection::Named(s) if s == "all" => vec![],
            crate::ir::EdgeSelection::Named(_) => vec![],
            crate::ir::EdgeSelection::Indices(idxs) => idxs
                .iter()
                .filter_map(|&i| face_ids.get(i).copied())
                .map(id_to_handle)
                .collect(),
        };
        let result = k
            .shell(solid, &open, op.thickness, 1e-3)
            .map_err(|e| occt_err(format!("shell: {:?}", e)))?;
        set_solid(state, unwrap_to_solid(k, result));
        Ok(())
    }

    fn handle_draft_extrude(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &DraftExtrudeOp,
    ) -> Result<(), KernelError> {
        let face = state.current_face.ok_or_else(|| {
            KernelError::InvalidState("draft_extrude requires a preceding sketch".into())
        })?;
        let solid = k
            .draft_prism(face, 0.0, 0.0, op.depth, op.angle)
            .map_err(|e| occt_err(format!("draft_prism: {:?}", e)))?;
        let solid = rotate_to_plane(k, solid, &state.active_plane)?;
        join_or_set(k, state, solid)
    }

    fn handle_thread(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ThreadOp,
        units: &Units,
    ) -> Result<(), KernelError> {
        let (major, pitch) = thread_dims(op, units)?;
        let internal = matches!(op.kind, ThreadKind::Internal);
        let through = op.through || (internal && op.length <= 0.0);
        let length = if op.length > 0.0 {
            op.length
        } else if internal {
            1.0
        } else {
            (major * 2.0).max(pitch * 4.0)
        };

        if internal {
            let base = state.current_solid.ok_or_else(|| {
                KernelError::InvalidState("internal thread (tap) needs an existing solid".into())
            })?;
            let [cx, cy] = map_uv(&op.plane, op.center[0], op.center[1]);
            let (span, z0) = cutter_span(k, Some(base), length, through);
            let tap_d = crate::thread::tap_drill_diameter(major, pitch);
            let hole = k
                .make_cylinder(tap_d / 2.0, span)
                .map_err(|e| occt_err(format!("tap drill: {e}")))?;
            let hole = k
                .translate(hole, cx, cy, z0)
                .map_err(|e| occt_err(format!("tap drill translate: {e}")))?;
            let hole = rotate_to_plane(k, hole, &op.plane)?;
            let hole = translate_if_needed(k, hole, op.at)?;
            let drilled = k
                .cut(base, hole)
                .map_err(|e| occt_err(format!("tap hole: {e}")))?;
            let drilled = drawable_shape(k, drilled);

            let cutter_len = if through { span } else { length };
            let cutter = thread_cutter(k, major, pitch, cutter_len, true, z0)?;
            let cutter = k
                .translate(cutter, cx, cy, 0.0)
                .map_err(|e| occt_err(format!("tap cutter translate: {e}")))?;
            let cutter = rotate_to_plane(k, cutter, &op.plane)?;
            let cutter = translate_if_needed(k, cutter, op.at)?;
            let cutter = maybe_left_hand(k, cutter, &op.hand)?;
            let raw = k
                .cut(drilled, cutter)
                .map_err(|e| occt_err(format!("tap thread: {e}")))?;
            state.current_solid = Some(drawable_shape(k, raw));
            Ok(())
        } else {
            // Long Z bolts: leave the fused hex+shank as a smooth cylinder.
            // WASM traps if we cut/tessellate 20+ helical turns as one solid.
            // The viewport instances short V-groove rods instead.
            if preview_from_thread(op, units).is_some() {
                if state.current_solid.is_none() {
                    let rod = k
                        .make_cylinder(major / 2.0, length)
                        .map_err(|e| occt_err(format!("thread cylinder: {e}")))?;
                    let rod = align_z_primitive_to_axis(k, rod, &op.axis)?;
                    let rod = translate_if_needed(k, rod, op.at)?;
                    join_or_set(k, state, rod)?;
                }
                return Ok(());
            }
            if let Some(base) = state.current_solid {
                match apply_external_thread_cut(
                    k, base, major, pitch, length, op.at, &op.axis, &op.hand,
                ) {
                    Ok(solid) => {
                        state.current_solid = Some(solid);
                        Ok(())
                    }
                    Err(_) => {
                        let rod = threaded_rod(k, major, pitch, length)?;
                        let rod = maybe_left_hand(k, rod, &op.hand)?;
                        let rod = align_z_primitive_to_axis(k, rod, &op.axis)?;
                        let rod = translate_if_needed(k, rod, op.at)?;
                        join_or_set(k, state, rod)
                    }
                }
            } else {
                let rod = threaded_rod(k, major, pitch, length)?;
                let rod = maybe_left_hand(k, rod, &op.hand)?;
                let rod = align_z_primitive_to_axis(k, rod, &op.axis)?;
                let rod = translate_if_needed(k, rod, op.at)?;
                join_or_set(k, state, rod)
            }
        }
    }

    fn thread_dims(op: &ThreadOp, units: &Units) -> Result<(f64, f64), KernelError> {
        let inch = matches!(units, Units::Inch);
        if let Some(size) = op.size.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let spec = crate::thread::parse_size(size).map_err(KernelError::InvalidState)?;
            let spec = crate::thread::to_units(&spec, inch);
            Ok((
                op.diameter.unwrap_or(spec.major_diameter),
                op.pitch.unwrap_or(spec.pitch),
            ))
        } else {
            Ok((op.diameter.unwrap(), op.pitch.unwrap()))
        }
    }

    fn maybe_left_hand(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        hand: &ThreadHand,
    ) -> Result<Handle, KernelError> {
        match hand {
            ThreadHand::Right => Ok(shape),
            ThreadHand::Left => k
                .mirror(shape, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0)
                .map_err(|e| occt_err(format!("left-hand thread: {e}"))),
        }
    }

    fn thread_cutter(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
        internal: bool,
        z0: f64,
    ) -> Result<Handle, KernelError> {
        // MakePipeShell only pipes an in-place circular Face. The disk is
        // the circumcircle of the ISO 5H/8 root and P/8 crest-edge points,
        // Frenet-piped along the overlapped helix. Never a meridian square.
        helical_round_groove(k, major, pitch, length, internal, z0)
    }

    /// Prefer one helical cutter. Only chunk if that boolean fails — five
    /// sequential cuts on a 40 mm M8 is far too slow for the viewport.
    fn apply_external_thread_cut(
        k: &mut occt_wasm::OcctKernel,
        mut solid: Handle,
        major: f64,
        pitch: f64,
        length: f64,
        at: [f64; 3],
        axis: &RevolveAxis,
        hand: &ThreadHand,
    ) -> Result<Handle, KernelError> {
        if length / pitch.max(1e-9) > MAX_INLINE_THREAD_TURNS {
            return Err(occt_err(
                "long helical cut skipped (viewport uses segmented thread mesh)",
            ));
        }
        let place = |k: &mut occt_wasm::OcctKernel, cutter: Handle| -> Result<Handle, KernelError> {
            let cutter = maybe_left_hand(k, cutter, hand)?;
            let cutter = align_z_primitive_to_axis(k, cutter, axis)?;
            translate_if_needed(k, cutter, at)
        };
        if let Ok(cutter) = thread_cutter(k, major, pitch, length, false, 0.0) {
            if let Ok(cutter) = place(k, cutter) {
                if let Ok(raw) = k.cut(solid, cutter) {
                    let out = drawable_shape(k, raw);
                    if shape_has_extent(k, out) {
                        return Ok(out);
                    }
                }
            }
        }
        let chunk = (pitch * 6.0).max(6.0);
        let mut z = 0.0;
        let mut any = false;
        while z < length - 1e-9 {
            let seg = (length - z).min(chunk);
            if let Ok(cutter) = thread_cutter(k, major, pitch, seg, false, z) {
                if let Ok(cutter) = place(k, cutter) {
                    match k.cut(solid, cutter) {
                        Ok(raw) => {
                            solid = drawable_shape(k, raw);
                            any = true;
                        }
                        Err(e) => {
                            let err = occt_err(format!("thread cut: {e}"));
                            if is_fatal_occt(&err) {
                                return Err(err);
                            }
                        }
                    }
                }
            }
            z += seg;
        }
        if any {
            Ok(solid)
        } else {
            Err(occt_err("helical thread cut removed no material"))
        }
    }

    fn pipe_iso_circle_bead(
        k: &mut occt_wasm::OcctKernel,
        r_h: f64,
        sec_r: f64,
        pitch: f64,
        height: f64,
        z0: f64,
        ppt: u32,
    ) -> Result<Handle, KernelError> {
        let path = crate::thread::cutter_helix_path(r_h, pitch, height, z0, ppt);
        let poly = wire_from_polyline3(k, &path)?;
        let p0 = path[0];
        let p1 = path
            .get(1)
            .copied()
            .unwrap_or([p0[0], p0[1], p0[2] + pitch]);
        let edge = k
            .make_circle_edge(
                p0[0],
                p0[1],
                p0[2],
                p1[0] - p0[0],
                p1[1] - p0[1],
                p1[2] - p0[2],
                sec_r,
            )
            .map_err(|e| occt_err(format!("ISO bead circle: {e}")))?;
        let wire = k
            .make_wire(&[edge])
            .map_err(|e| occt_err(format!("ISO bead wire: {e}")))?;
        let face = k
            .make_face(wire)
            .map_err(|e| occt_err(format!("ISO bead face: {e}")))?;
        pipe_thread_cutter(k, face, poly)
    }

    /// Helical groove whose circular section is the circumcircle of the
    /// ISO 5H/8 root and the two P/8 crest-edge points. Frenet-piped along
    /// an overlapped polyline helix so yaw walks and the +X seam closes.
    fn helical_round_groove(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
        internal: bool,
        z0: f64,
    ) -> Result<Handle, KernelError> {
        let (r_h, sec_r) = if internal {
            let r_h = crate::thread::tap_drill_diameter(major, pitch) / 2.0;
            (r_h, crate::thread::external_depth(pitch) * 0.55)
        } else {
            crate::thread::cutter_iso_circle(major, pitch)
        };
        let height = (length + pitch).max(pitch * 2.0);
        pipe_iso_circle_bead(
            k,
            r_h,
            sec_r,
            pitch,
            height,
            z0,
            thread_polyline_samples(height, pitch),
        )
    }

    /// Sweep a thread bead with a rolling (Frenet) trihedron so yaw walks
    /// around the shank. MakePipe is tried last — it can freeze the section
    /// and leave the leftover generator strip.
    fn pipe_thread_cutter(
        k: &mut occt_wasm::OcctKernel,
        profile: Handle,
        spine: Handle,
    ) -> Result<Handle, KernelError> {
        let as_solid = |k: &mut occt_wasm::OcctKernel, s: Handle| -> Option<Handle> {
            let ids = k.get_sub_shapes(s, "solid").ok()?;
            if ids.is_empty() {
                k.thicken(s, 0.05, 1e-3).ok().map(|t| unwrap_to_solid(k, t))
            } else {
                Some(unwrap_to_solid(k, s))
            }
        };
        for (freenet, smooth) in [(true, true), (true, false)] {
            if let Ok(s) = k.sweep_pipe_shell(profile, spine, freenet, smooth) {
                if let Some(sol) = as_solid(k, s) {
                    return Ok(sol);
                }
            }
        }
        let wire = k.outer_wire(profile).unwrap_or(profile);
        if let Ok(s) = k.sweep(wire, spine, 1) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        if let Ok(s) = k.pipe(profile, spine) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        Err(occt_err("thread cutter pipe failed"))
    }

    fn thread_polyline_samples(height: f64, pitch: f64) -> u32 {
        // Short cutters only (long Z bolts instance ≤8-turn rods). Spend
        // points so zoomed flanks are a helix, not rectangular bites.
        let turns = (height / pitch.max(1e-9)).max(0.25);
        if turns <= 8.5 {
            48
        } else {
            let max_pts = 220.0;
            ((max_pts / turns).floor() as u32).clamp(16, 32)
        }
    }

    fn threaded_rod(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
    ) -> Result<Handle, KernelError> {
        let cyl = k
            .make_cylinder(major / 2.0, length)
            .map_err(|e| occt_err(format!("thread cylinder: {e}")))?;
        apply_external_thread_cut(
            k,
            cyl,
            major,
            pitch,
            length,
            [0.0, 0.0, 0.0],
            &RevolveAxis::Z,
            &ThreadHand::Right,
        )
    }

    fn helix_samples_per_turn(height: f64, pitch: f64) -> u32 {
        let turns = (height / pitch.max(1e-9)).max(0.25);
        if turns > 16.0 {
            12
        } else if turns > 8.0 {
            16
        } else {
            36
        }
    }

    fn handle_sweep(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &SweepOp,
    ) -> Result<(), KernelError> {
        let profile = if let Some(p) = &op.profile {
            make_profile_face(k, p, [0.0, 0.0])?
        } else {
            state.current_face.ok_or_else(|| {
                KernelError::InvalidState("sweep needs a profile or a preceding sketch".into())
            })?
        };
        let spine = make_sweep_path(k, &op.path)?;
        let solid = match pipe_along(k, profile, spine) {
            Ok(s) => s,
            Err(_) => {
                let swept = k
                    .pipe(profile, spine)
                    .or_else(|_| k.simple_pipe(profile, spine))
                    .map_err(|e| occt_err(format!("sweep/pipe: {:?}", e)))?;
                unwrap_to_solid(k, swept)
            }
        };
        absorb_solid(k, state, solid, op.fuse)?;
        Ok(())
    }

    fn handle_helix(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &HelixOp,
    ) -> Result<(), KernelError> {
        let sec_r = op.diameter / 2.0;
        let spine = helix_spine(k, op.pitch, op.height, op.radius, [0.0; 3], &RevolveAxis::Z)?;
        let solid = helix_solid(k, spine, op.radius, op.pitch, op.height, sec_r, true)?;
        let solid = align_z_primitive_to_axis(k, solid, &op.axis)?;
        let solid = translate_if_needed(k, solid, op.center)?;
        absorb_solid(k, state, solid, op.fuse)?;
        Ok(())
    }

    fn helix_solid(
        k: &mut occt_wasm::OcctKernel,
        spine: Handle,
        radius: f64,
        pitch: f64,
        height: f64,
        sec_r: f64,
        allow_tori: bool,
    ) -> Result<Handle, KernelError> {
        let sec_d = sec_r * 2.0;
        // Square section at the helix start (XZ plane) — more reliable than a disk.
        let square = face_from_polygon_3d(
            k,
            &[
                [radius - sec_r, 0.0, -sec_r],
                [radius + sec_r, 0.0, -sec_r],
                [radius + sec_r, 0.0, sec_r],
                [radius - sec_r, 0.0, sec_r],
            ],
        )
        .ok();
        if let Some(face) = square {
            if let Ok(s) = pipe_along(k, face, spine) {
                return Ok(s);
            }
        }
        // Circle in XY at the origin; MakePipe relocates it onto the spine.
        if let Ok(edge) = k.make_circle_edge(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, sec_r) {
            if let Ok(wire) = k.make_wire(&[edge]) {
                if let Ok(face) = k.make_face(wire) {
                    if let Ok(s) = pipe_along(k, face, spine) {
                        return Ok(s);
                    }
                }
            }
        }
        // Rectangle on XY, centered.
        if let Ok(rect) = k.make_rectangle(sec_d, sec_d) {
            let rect = k.translate(rect, -sec_r, -sec_r, 0.0).unwrap_or(rect);
            if let Ok(s) = pipe_along(k, rect, spine) {
                return Ok(s);
            }
        }
        // Approximate the helix with a polyline (C0) and sweep with round corners.
        let path = helix_polyline(
            radius,
            pitch,
            height,
            helix_samples_per_turn(height, pitch).min(24),
        );
        if let Ok(poly) = wire_from_polyline3(k, &path) {
            if let Ok(rect) = k.make_rectangle(sec_d, sec_d) {
                let rect = k.translate(rect, -sec_r, -sec_r, 0.0).unwrap_or(rect);
                if let Ok(s) = pipe_along(k, rect, poly) {
                    return Ok(s);
                }
            }
            if let Some(face) = square {
                if let Ok(s) = pipe_along(k, face, poly) {
                    return Ok(s);
                }
            }
        }
        if !allow_tori {
            return Err(occt_err("helix/coil sweep failed"));
        }
        // Last resort for decorative springs only — never for thread cutters.
        let n = ((height / pitch).round() as i32).max(1);
        let mut acc: Option<Handle> = None;
        for i in 0..n {
            let ring = k
                .make_torus(radius, sec_r)
                .map_err(|e| occt_err(format!("helix torus: {e}")))?;
            let ring = k
                .translate(ring, 0.0, 0.0, i as f64 * pitch)
                .map_err(|e| occt_err(format!("helix torus translate: {e}")))?;
            acc = Some(match acc {
                None => ring,
                Some(a) => k.fuse(a, ring).map(|s| unwrap_to_solid(k, s)).unwrap_or(a),
            });
        }
        acc.ok_or_else(|| occt_err("helix/coil sweep failed"))
    }

    fn helix_polyline(radius: f64, pitch: f64, height: f64, pts_per_turn: u32) -> Vec<[f64; 3]> {
        let turns = (height / pitch).max(0.25);
        let n = ((turns * pts_per_turn as f64).ceil() as usize).max(8);
        (0..=n)
            .map(|i| {
                let t = i as f64 / pts_per_turn as f64;
                let a = t * 2.0 * PI;
                [radius * a.cos(), radius * a.sin(), (t * pitch).min(height)]
            })
            .collect()
    }

    fn handle_offset(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &OffsetOp,
    ) -> Result<(), KernelError> {
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("offset requires an existing solid".into()))?;
        let result = k
            .offset(solid, op.distance, 1e-3)
            .map_err(|e| occt_err(format!("offset: {e}")))?;
        set_solid(state, unwrap_to_solid(k, result));
        Ok(())
    }

    fn handle_pipe(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &PipeOp,
    ) -> Result<(), KernelError> {
        let profile = make_profile_face(
            k,
            &Profile::Circle(CircleProfile {
                d: op.diameter,
                at: [0.0, 0.0],
            }),
            [0.0, 0.0],
        )?;
        let spine = make_sweep_path(k, &op.path)?;
        // Place profile near the path start so the pipe starts cleanly.
        let start = path_start(&op.path);
        let profile = k
            .translate(profile, start[0], start[1], start[2])
            .map_err(|e| occt_err(format!("pipe profile translate: {:?}", e)))?;
        let solid = match pipe_along(k, profile, spine) {
            Ok(s) => s,
            Err(_) => {
                let swept = k
                    .pipe(profile, spine)
                    .or_else(|_| k.simple_pipe(profile, spine))
                    .map_err(|e| occt_err(format!("pipe: {:?}", e)))?;
                unwrap_to_solid(k, swept)
            }
        };
        absorb_solid(k, state, solid, op.fuse)?;
        Ok(())
    }

    fn handle_thicken(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ThickenOp,
    ) -> Result<(), KernelError> {
        let shape = if let Some(ref face_ref) = op.face {
            let solid = state.current_solid.ok_or_else(|| {
                KernelError::InvalidState("thicken face requires an existing solid".into())
            })?;
            resolve_face_handle(k, solid, face_ref)?
        } else if let Some(face) = state.current_face {
            face
        } else {
            state.current_solid.ok_or_else(|| {
                KernelError::InvalidState(
                    "thicken requires a preceding sketch, a face selector, or an existing solid"
                        .into(),
                )
            })?
        };
        let solid = k
            .thicken(shape, op.thickness, 1e-3)
            .map_err(|e| occt_err(format!("thicken: {:?}", e)))?;
        let solid = unwrap_to_solid(k, solid);
        absorb_solid(k, state, solid, op.fuse)?;
        state.current_face = None;
        Ok(())
    }

    fn handle_ellipsoid(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &EllipsoidOp,
    ) -> Result<(), KernelError> {
        let [rx, ry, rz] = op.radii;
        let solid = k
            .make_ellipsoid(rx, ry, rz)
            .map_err(|e| occt_err(format!("make_ellipsoid: {e}")))?;
        let solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn handle_draft(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &DraftOp,
    ) -> Result<(), KernelError> {
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("draft requires an existing solid".into()))?;
        let solid = unwrap_to_solid(k, solid);
        let face_ids = k
            .get_sub_shapes(solid, "face")
            .map_err(|e| occt_err(format!("get_sub_shapes (draft): {:?}", e)))?;
        let selected = match &op.faces {
            EdgeSelection::Named(name) => select_faces_by_name(k, solid, &face_ids, name),
            EdgeSelection::Indices(idxs) => idxs
                .iter()
                .filter_map(|&i| face_ids.get(i).copied())
                .collect(),
        };
        if selected.is_empty() {
            return Err(KernelError::InvalidState(
                "draft found no matching faces".into(),
            ));
        }
        let angle_rad = op.angle * PI / 180.0;
        let [dx, dy, dz] = op.direction;
        let mut result = solid;
        for id in selected {
            result = k
                .draft(result, id_to_handle(id), angle_rad, dx, dy, dz)
                .map_err(|e| occt_err(format!("draft: {:?}", e)))?;
        }
        set_solid(state, unwrap_to_solid(k, result));
        Ok(())
    }

    fn helix_spine(
        k: &mut occt_wasm::OcctKernel,
        pitch: f64,
        height: f64,
        radius: f64,
        at: [f64; 3],
        axis: &RevolveAxis,
    ) -> Result<Handle, KernelError> {
        let (dx, dy, dz) = axis_dir(axis);
        k.make_helix_wire(at[0], at[1], at[2], dx, dy, dz, pitch, height, radius)
            .map_err(|e| occt_err(format!("make_helix_wire: {e}")))
    }

    fn wire_from_polyline3(
        k: &mut occt_wasm::OcctKernel,
        pts: &[[f64; 3]],
    ) -> Result<Handle, KernelError> {
        if pts.len() < 2 {
            return Err(KernelError::InvalidState(
                "polyline path needs at least 2 points".into(),
            ));
        }
        let mut edges = Vec::with_capacity(pts.len() - 1);
        for w in pts.windows(2) {
            let a = w[0];
            let b = w[1];
            let e = k
                .make_line_edge(a[0], a[1], a[2], b[0], b[1], b[2])
                .map_err(|err| occt_err(format!("path edge: {err}")))?;
            edges.push(e);
        }
        k.make_wire(&edges)
            .map_err(|e| occt_err(format!("path wire: {e}")))
    }

    fn closed_polygon_wire(
        k: &mut occt_wasm::OcctKernel,
        pts: &[[f64; 3]],
    ) -> Result<Handle, KernelError> {
        if pts.len() < 3 {
            return Err(KernelError::InvalidState(
                "polygon wire needs at least 3 points".into(),
            ));
        }
        let mut edges = Vec::with_capacity(pts.len());
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            let e = k
                .make_line_edge(a[0], a[1], a[2], b[0], b[1], b[2])
                .map_err(|err| occt_err(format!("polygon edge: {err}")))?;
            edges.push(e);
        }
        k.make_wire(&edges)
            .map_err(|e| occt_err(format!("polygon wire: {e}")))
    }

    fn face_from_polygon_3d(
        k: &mut occt_wasm::OcctKernel,
        pts: &[[f64; 3]],
    ) -> Result<Handle, KernelError> {
        let wire = closed_polygon_wire(k, pts)?;
        k.make_face(wire)
            .or_else(|_| k.make_non_planar_face(wire))
            .map_err(|e| occt_err(format!("polygon face: {e}")))
    }

    fn pipe_along(
        k: &mut occt_wasm::OcctKernel,
        profile: Handle,
        spine: Handle,
    ) -> Result<Handle, KernelError> {
        let as_solid = |k: &mut occt_wasm::OcctKernel, s: Handle| -> Option<Handle> {
            let ids = k.get_sub_shapes(s, "solid").ok()?;
            if ids.is_empty() {
                k.thicken(s, 0.05, 1e-3).ok().map(|t| unwrap_to_solid(k, t))
            } else {
                Some(unwrap_to_solid(k, s))
            }
        };
        if let Ok(s) = k.pipe(profile, spine) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        let wire = k.outer_wire(profile).unwrap_or(profile);
        if let Ok(s) = k.simple_pipe(wire, spine) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        if let Ok(s) = k.sweep(wire, spine, 0) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        if let Ok(s) = k.sweep(wire, spine, 1) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        let aux = k.make_null_shape().unwrap_or(spine);
        if let Ok(s) = k.sweep_oriented(profile, spine, 1, 0.0, 0.0, 1.0, aux) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        if let Ok(s) = k.sweep_pipe_shell(profile, spine, true, false) {
            if let Some(sol) = as_solid(k, s) {
                return Ok(sol);
            }
        }
        Err(occt_err("sweep/pipe along path failed"))
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn absorb_solid(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        solid: Handle,
        fuse: bool,
    ) -> Result<(), KernelError> {
        if fuse {
            if let Some(base) = state.current_solid {
                set_solid(state, fuse_robust(k, base, solid, "fuse absorb")?);
                return Ok(());
            }
        }
        set_solid(state, solid);
        Ok(())
    }

    fn path_start(path: &SweepPath) -> [f64; 3] {
        match path {
            SweepPath::Polyline { points } => points.first().copied().unwrap_or([0.0; 3]),
            SweepPath::Helix {
                center,
                radius,
                axis,
                ..
            } => {
                let (dx, dy, dz) = axis_dir(axis);
                // Point offset from axis by radius in a perpendicular direction.
                let (px, py, pz) = if dx.abs() < 0.9 {
                    (0.0, 0.0, 1.0)
                } else {
                    (0.0, 1.0, 0.0)
                };
                let cxp = dy * pz - dz * py;
                let cyp = dz * px - dx * pz;
                let czp = dx * py - dy * px;
                let len = (cxp * cxp + cyp * cyp + czp * czp).sqrt().max(1e-9);
                [
                    center[0] + radius * cxp / len,
                    center[1] + radius * cyp / len,
                    center[2] + radius * czp / len,
                ]
            }
        }
    }

    fn make_sweep_path(
        k: &mut occt_wasm::OcctKernel,
        path: &SweepPath,
    ) -> Result<Handle, KernelError> {
        match path {
            SweepPath::Polyline { points } => {
                let mut edges = Vec::with_capacity(points.len().saturating_sub(1));
                for w in points.windows(2) {
                    let a = w[0];
                    let b = w[1];
                    let e = k
                        .make_line_edge(a[0], a[1], a[2], b[0], b[1], b[2])
                        .map_err(|e| occt_err(format!("path edge: {:?}", e)))?;
                    edges.push(e);
                }
                k.make_wire(&edges)
                    .map_err(|e| occt_err(format!("path wire: {:?}", e)))
            }
            SweepPath::Helix {
                pitch,
                height,
                radius,
                center,
                axis,
            } => {
                let (dx, dy, dz) = axis_dir(axis);
                k.make_helix_wire(
                    center[0], center[1], center[2], dx, dy, dz, *pitch, *height, *radius,
                )
                .map_err(|e| occt_err(format!("make_helix_wire: {:?}", e)))
            }
        }
    }

    struct FaceFrame {
        origin: [f64; 3],
        normal: [f64; 3],
        x_dir: [f64; 3],
        y_dir: [f64; 3],
        face: Handle,
    }

    fn resolve_face_handle(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        face_ref: &FaceRef,
    ) -> Result<Handle, KernelError> {
        Ok(resolve_face_frame(k, solid, face_ref)?.face)
    }

    fn resolve_face_frame(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        face_ref: &FaceRef,
    ) -> Result<FaceFrame, KernelError> {
        let face_ids = k
            .get_sub_shapes(solid, "face")
            .map_err(|e| occt_err(format!("get_sub_shapes (face): {:?}", e)))?;
        if face_ids.is_empty() {
            return Err(KernelError::InvalidState("solid has no faces".into()));
        }
        let id = match face_ref {
            FaceRef::Index(i) => face_ids.get(*i).copied().ok_or_else(|| {
                KernelError::InvalidState(format!(
                    "face index {i} out of range (0..{})",
                    face_ids.len()
                ))
            })?,
            FaceRef::Named(name) => {
                let selected = select_faces_by_name(k, solid, &face_ids, name);
                selected
                    .into_iter()
                    .next()
                    .ok_or_else(|| KernelError::InvalidState(format!("no face matched '{name}'")))?
            }
        };
        let face = id_to_handle(id);
        let center = k
            .get_surface_center_of_mass(face)
            .or_else(|_| {
                k.get_bounding_box(face, false).map(|bb| {
                    vec![
                        0.5 * (bb.min.x + bb.max.x),
                        0.5 * (bb.min.y + bb.max.y),
                        0.5 * (bb.min.z + bb.max.z),
                    ]
                })
            })
            .map_err(|e| occt_err(format!("face center: {:?}", e)))?;
        let origin = [
            *center.first().unwrap_or(&0.0),
            *center.get(1).unwrap_or(&0.0),
            *center.get(2).unwrap_or(&0.0),
        ];
        let uv = k.uv_bounds(face).unwrap_or(vec![0.0, 1.0, 0.0, 1.0]);
        let u_mid = 0.5 * (uv.first().copied().unwrap_or(0.0) + uv.get(1).copied().unwrap_or(1.0));
        let v_mid = 0.5 * (uv.get(2).copied().unwrap_or(0.0) + uv.get(3).copied().unwrap_or(1.0));
        let normal_v = k
            .surface_normal(face, u_mid, v_mid)
            .unwrap_or(vec![0.0, 0.0, 1.0]);
        let mut normal = [
            *normal_v.first().unwrap_or(&0.0),
            *normal_v.get(1).unwrap_or(&0.0),
            *normal_v.get(2).unwrap_or(&1.0),
        ];
        let nlen = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if nlen > 1e-12 {
            normal[0] /= nlen;
            normal[1] /= nlen;
            normal[2] /= nlen;
        }
        let (x_dir, y_dir) = orthonormal_basis(normal);
        Ok(FaceFrame {
            origin,
            normal,
            x_dir,
            y_dir,
            face,
        })
    }

    fn select_faces_by_name(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        face_ids: &[u32],
        name: &str,
    ) -> Vec<u32> {
        let name = name.to_ascii_lowercase();
        let mut scored: Vec<(u32, f64, [f64; 3], [f64; 3])> = face_ids
            .iter()
            .filter_map(|&id| {
                let h = id_to_handle(id);
                let area = k.get_surface_area(h).unwrap_or(0.0);
                let center = k.get_surface_center_of_mass(h).ok().or_else(|| {
                    k.get_bounding_box(h, false).ok().map(|bb| {
                        vec![
                            0.5 * (bb.min.x + bb.max.x),
                            0.5 * (bb.min.y + bb.max.y),
                            0.5 * (bb.min.z + bb.max.z),
                        ]
                    })
                })?;
                let c = [
                    center.first().copied().unwrap_or(0.0),
                    center.get(1).copied().unwrap_or(0.0),
                    center.get(2).copied().unwrap_or(0.0),
                ];
                let uv = k.uv_bounds(h).unwrap_or(vec![0.0, 1.0, 0.0, 1.0]);
                let u_mid =
                    0.5 * (uv.first().copied().unwrap_or(0.0) + uv.get(1).copied().unwrap_or(1.0));
                let v_mid =
                    0.5 * (uv.get(2).copied().unwrap_or(0.0) + uv.get(3).copied().unwrap_or(1.0));
                let n = k
                    .surface_normal(h, u_mid, v_mid)
                    .unwrap_or(vec![0.0, 0.0, 1.0]);
                let normal = [
                    n.first().copied().unwrap_or(0.0),
                    n.get(1).copied().unwrap_or(0.0),
                    n.get(2).copied().unwrap_or(1.0),
                ];
                Some((id, area, c, normal))
            })
            .collect();
        if scored.is_empty() {
            return face_ids.to_vec();
        }
        match name.as_str() {
            "largest" | "all" => {
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                if name == "all" {
                    scored.into_iter().map(|s| s.0).collect()
                } else {
                    vec![scored[0].0]
                }
            }
            "top" => {
                scored.sort_by(|a, b| {
                    b.2[2]
                        .partial_cmp(&a.2[2])
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                vec![scored[0].0]
            }
            "bottom" => {
                scored.sort_by(|a, b| {
                    a.2[2]
                        .partial_cmp(&b.2[2])
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                vec![scored[0].0]
            }
            "side" | "sides" => {
                let solid_bb = k.get_bounding_box(solid, false).ok();
                scored
                    .into_iter()
                    .filter(|(_, _, _, n)| n[2].abs() < 0.5)
                    .map(|s| s.0)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .chain(solid_bb.map(|_| vec![]).unwrap_or_default())
                    .collect()
            }
            _ => face_ids.to_vec(),
        }
    }

    fn filter_edges_by_name(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        edge_ids: &[u32],
        name: &str,
        blend: f64,
    ) -> Vec<u32> {
        let name = name.to_ascii_lowercase();
        match name.as_str() {
            "all" => select_blend_edges(k, solid, edge_ids.to_vec(), blend),
            "top" => {
                let Ok(bb) = k.get_bounding_box(solid, false) else {
                    return edge_ids.to_vec();
                };
                let edges = classify_line_edges(k, &bb, edge_ids);
                edges
                    .into_iter()
                    .filter(|e| e.is_top && !e.is_thickness)
                    .map(|e| e.id)
                    .collect()
            }
            "longest" | "outer" => longest_edges(k, edge_ids, 4.max(edge_ids.len().min(8))),
            _ => edge_ids.to_vec(),
        }
    }

    fn orthonormal_basis(n: [f64; 3]) -> ([f64; 3], [f64; 3]) {
        let helper = if n[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let mut x = [
            n[1] * helper[2] - n[2] * helper[1],
            n[2] * helper[0] - n[0] * helper[2],
            n[0] * helper[1] - n[1] * helper[0],
        ];
        let xl = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt().max(1e-12);
        x[0] /= xl;
        x[1] /= xl;
        x[2] /= xl;
        let y = [
            n[1] * x[2] - n[2] * x[1],
            n[2] * x[0] - n[0] * x[2],
            n[0] * x[1] - n[1] * x[0],
        ];
        (x, y)
    }

    fn dominant_plane(n: &[f64; 3]) -> SketchPlane {
        let ax = n[0].abs();
        let ay = n[1].abs();
        let az = n[2].abs();
        if az >= ax && az >= ay {
            SketchPlane::XY
        } else if ay >= ax {
            SketchPlane::XZ
        } else {
            SketchPlane::YZ
        }
    }

    fn rotate_z_to_dir(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        dir: [f64; 3],
    ) -> Result<Handle, KernelError> {
        let mut d = dir;
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-12 {
            return Ok(shape);
        }
        d[0] /= len;
        d[1] /= len;
        d[2] /= len;
        let z = [0.0, 0.0, 1.0];
        let dot = (z[0] * d[0] + z[1] * d[1] + z[2] * d[2]).clamp(-1.0, 1.0);
        if (dot - 1.0).abs() < 1e-9 {
            return Ok(shape);
        }
        if (dot + 1.0).abs() < 1e-9 {
            return k
                .rotate(shape, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, PI)
                .map_err(|e| occt_err(format!("rotate 180: {:?}", e)));
        }
        let axis = [
            z[1] * d[2] - z[2] * d[1],
            z[2] * d[0] - z[0] * d[2],
            z[0] * d[1] - z[1] * d[0],
        ];
        let angle = dot.acos();
        k.rotate(shape, 0.0, 0.0, 0.0, axis[0], axis[1], axis[2], angle)
            .map_err(|e| occt_err(format!("rotate_z_to_dir: {:?}", e)))
    }

    fn place_xy_shape_on_frame(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        frame: &FaceFrame,
    ) -> Result<Handle, KernelError> {
        let shape = rotate_z_to_dir(k, shape, frame.normal)?;
        // After rotating +Z → normal, +X may not align with frame.x_dir.
        // For agentic prismatic work, aligning the normal + translating to the
        // face center is enough; UV offsets are applied in make_tool_on_face.
        k.translate(shape, frame.origin[0], frame.origin[1], frame.origin[2])
            .map_err(|e| occt_err(format!("place on face: {:?}", e)))
    }

    fn make_tool_on_face(
        k: &mut occt_wasm::OcctKernel,
        base: Handle,
        face_ref: &FaceRef,
        profile: &Profile,
        depth: f64,
        at: [f64; 3],
        through: bool,
    ) -> Result<Handle, KernelError> {
        let frame = resolve_face_frame(k, base, face_ref)?;
        let face = make_profile_face(k, profile, [0.0, 0.0])?;
        let (length, z0) = cutter_span(k, Some(base), depth, through);
        let solid = k
            .extrude(face, 0.0, 0.0, length)
            .map_err(|e| occt_err(format!("extrude (face tool): {:?}", e)))?;
        let solid = k
            .translate(solid, 0.0, 0.0, z0)
            .map_err(|e| occt_err(format!("translate face tool: {:?}", e)))?;
        let solid = rotate_z_to_dir(k, solid, frame.normal)?;
        let ox = frame.origin[0] + at[0];
        let oy = frame.origin[1] + at[1];
        let oz = frame.origin[2] + at[2];
        k.translate(solid, ox, oy, oz)
            .map_err(|e| occt_err(format!("place face tool: {:?}", e)))
    }

    fn make_tool_solid(
        k: &mut occt_wasm::OcctKernel,
        profile: &Profile,
        depth: f64,
        at: [f64; 3],
        plane: &SketchPlane,
        through: bool,
        base: Option<Handle>,
    ) -> Result<Handle, KernelError> {
        let face = make_profile_face(k, profile, [0.0, 0.0])?;
        let face = remap_face_uv_for_plane(k, face, plane)?;
        let (length, z0) = cutter_span(k, base, depth, through);
        let solid = k
            .extrude(face, 0.0, 0.0, length)
            .map_err(|e| occt_err(format!("extrude (tool): {:?}", e)))?;
        let solid = k
            .translate(solid, 0.0, 0.0, z0)
            .map_err(|e| occt_err(format!("translate cutter: {:?}", e)))?;
        let solid = rotate_to_plane(k, solid, plane)?;
        translate_if_needed(k, solid, at)
    }

    /// Length along +Z and starting Z of a cutter.
    /// Through-cutters span the solid's bbox in every direction plus margin,
    /// centered on the profile plane, so a wrong-side `at` still punches through.
    fn cutter_span(
        k: &mut occt_wasm::OcctKernel,
        base: Option<Handle>,
        depth: f64,
        through: bool,
    ) -> (f64, f64) {
        if through {
            let span = base
                .and_then(|s| k.get_bounding_box(s, true).ok())
                .map(|bb| {
                    let dx = (bb.max.x - bb.min.x).abs();
                    let dy = (bb.max.y - bb.min.y).abs();
                    let dz = (bb.max.z - bb.min.z).abs();
                    dx.max(dy).max(dz) + 20.0
                })
                .filter(|&s| s > 1.0)
                .unwrap_or(1000.0);
            (span * 2.0, -span)
        } else {
            (depth + 2.0, -1.0)
        }
    }

    /// Build a planar face on XY from any profile, applying `origin` and the
    /// profile's own `at` / `centered` fields. Negative coordinates are fine.
    fn make_profile_face(
        k: &mut occt_wasm::OcctKernel,
        profile: &Profile,
        origin: [f64; 2],
    ) -> Result<Handle, KernelError> {
        match profile {
            Profile::Rect(r) => {
                let f = k
                    .make_rectangle(r.w, r.h)
                    .map_err(|e| occt_err(format!("make_rectangle: {:?}", e)))?;
                let mut dx = origin[0] + r.at[0];
                let mut dy = origin[1] + r.at[1];
                if r.centered {
                    dx -= r.w / 2.0;
                    dy -= r.h / 2.0;
                }
                translate_if_needed(k, f, [dx, dy, 0.0])
            }
            Profile::Circle(c) => {
                let cx = origin[0] + c.at[0];
                let cy = origin[1] + c.at[1];
                let radius = c.d / 2.0;
                let edge = k
                    .make_circle_edge(cx, cy, 0.0, 0.0, 0.0, 1.0, radius)
                    .map_err(|e| occt_err(format!("make_circle_edge: {:?}", e)))?;
                let wire = k
                    .make_wire(&[edge])
                    .map_err(|e| occt_err(format!("make_wire (circle): {:?}", e)))?;
                k.make_face(wire)
                    .map_err(|e| occt_err(format!("make_face (circle): {:?}", e)))
            }
            Profile::Polyline(p) => {
                if !p.closed {
                    return Err(KernelError::InvalidState(
                        "open polyline cannot form a face; set closed: true".into(),
                    ));
                }
                let n = p.points.len();
                let mut edges = Vec::with_capacity(n);
                for i in 0..n {
                    let a = p.points[i];
                    let b = p.points[(i + 1) % n];
                    let dx = a[0] - b[0];
                    let dy = a[1] - b[1];
                    if dx * dx + dy * dy < 1e-16 {
                        continue;
                    }
                    let e = k
                        .make_line_edge(
                            a[0] + origin[0],
                            a[1] + origin[1],
                            0.0,
                            b[0] + origin[0],
                            b[1] + origin[1],
                            0.0,
                        )
                        .map_err(|e| occt_err(format!("make_line_edge: {e}")))?;
                    edges.push(e);
                }
                if edges.len() < 3 {
                    return Err(KernelError::InvalidState(
                        "polyline needs at least 3 distinct edges (coincident points were skipped)"
                            .into(),
                    ));
                }
                let wire = k
                    .make_wire(&edges)
                    .map_err(|e| occt_err(format!("make_wire (polyline): {e}")))?;
                k.make_face(wire)
                    .map_err(|e| occt_err(format!("make_face (polyline): {e}")))
            }
            Profile::Arc(a) => {
                let cx = origin[0] + a.center[0];
                let cy = origin[1] + a.center[1];
                let start = a.start_angle * PI / 180.0;
                let end = a.end_angle * PI / 180.0;
                let p1x = cx + a.radius * start.cos();
                let p1y = cy + a.radius * start.sin();
                let p2x = cx + a.radius * end.cos();
                let p2y = cy + a.radius * end.sin();
                let arc = k
                    .make_circle_arc(cx, cy, 0.0, 0.0, 0.0, 1.0, a.radius, start, end)
                    .map_err(|e| occt_err(format!("make_circle_arc: {:?}", e)))?;
                let to_start = k
                    .make_line_edge(cx, cy, 0.0, p1x, p1y, 0.0)
                    .map_err(|e| occt_err(format!("arc radius 1: {:?}", e)))?;
                let to_center = k
                    .make_line_edge(p2x, p2y, 0.0, cx, cy, 0.0)
                    .map_err(|e| occt_err(format!("arc radius 2: {:?}", e)))?;
                let wire = k
                    .make_wire(&[to_start, arc, to_center])
                    .map_err(|e| occt_err(format!("make_wire (arc): {:?}", e)))?;
                k.make_face(wire)
                    .map_err(|e| occt_err(format!("make_face (arc): {:?}", e)))
            }
            Profile::Compound(c) => {
                let outer = make_profile_face(k, &c.outer, origin)?;
                if c.holes.is_empty() {
                    return Ok(outer);
                }
                let mut hole_wires = Vec::with_capacity(c.holes.len());
                for hole in &c.holes {
                    let hole_face = make_profile_face(k, hole, origin)?;
                    let wire = k
                        .outer_wire(hole_face)
                        .map_err(|e| occt_err(format!("outer_wire (hole): {:?}", e)))?;
                    hole_wires.push(wire);
                }
                k.add_holes_in_face(outer, &hole_wires)
                    .map_err(|e| occt_err(format!("add_holes_in_face: {:?}", e)))
            }
            Profile::Hex(hx) => {
                let poly = Profile::Polyline(PolylineProfile {
                    points: crate::ir::hex_vertices(
                        hx.across_flats,
                        [origin[0] + hx.at[0], origin[1] + hx.at[1]],
                    ),
                    closed: true,
                });
                make_profile_face(k, &poly, [0.0, 0.0])
            }
            Profile::Ellipse(e) => {
                let cx = origin[0] + e.at[0];
                let cy = origin[1] + e.at[1];
                let edge = k
                    .make_ellipse_edge(cx, cy, 0.0, 0.0, 0.0, 1.0, e.major / 2.0, e.minor / 2.0)
                    .map_err(|err| occt_err(format!("make_ellipse_edge: {:?}", err)))?;
                let wire = k
                    .make_wire(&[edge])
                    .map_err(|err| occt_err(format!("make_wire (ellipse): {:?}", err)))?;
                k.make_face(wire)
                    .map_err(|err| occt_err(format!("make_face (ellipse): {:?}", err)))
            }
        }
    }

    fn translate_if_needed(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        at: [f64; 3],
    ) -> Result<Handle, KernelError> {
        if at == [0.0, 0.0, 0.0] {
            return Ok(shape);
        }
        k.translate(shape, at[0], at[1], at[2])
            .map_err(|e| occt_err(format!("translate: {:?}", e)))
    }

    fn set_solid(state: &mut ExecState, solid: Handle) {
        state.current_solid = Some(solid);
        state.current_face = None;
    }

    /// First solid on a body is adopted as-is. Later primitives/extrudes join.
    fn join_or_set(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        solid: Handle,
    ) -> Result<(), KernelError> {
        let joined = if let Some(base) = state.current_solid {
            fuse_robust(k, base, solid, "join")?
        } else {
            solid
        };
        set_solid(state, joined);
        Ok(())
    }

    /// Boolean union that survives coplanar and helical contact.
    ///
    /// Fusing a hex prism onto a threaded shank fails in OCCT (helical caps /
    /// coincident faces). Nudge the addend into the base, then keep both solids
    /// in a compound so tessellation still shows the full part.
    fn fuse_robust(
        k: &mut occt_wasm::OcctKernel,
        a: Handle,
        b: Handle,
        ctx: &str,
    ) -> Result<Handle, KernelError> {
        if let Ok(raw) = k.fuse(a, b) {
            if let Some(ok) = valid_drawable(k, raw) {
                return Ok(ok);
            }
        }

        if let (Ok(ba), Ok(bb)) = (k.get_bounding_box(a, false), k.get_bounding_box(b, false)) {
            let ca = [
                (ba.min.x + ba.max.x) * 0.5,
                (ba.min.y + ba.max.y) * 0.5,
                (ba.min.z + ba.max.z) * 0.5,
            ];
            let cb = [
                (bb.min.x + bb.max.x) * 0.5,
                (bb.min.y + bb.max.y) * 0.5,
                (bb.min.z + bb.max.z) * 0.5,
            ];
            let dx = ca[0] - cb[0];
            let dy = ca[1] - cb[1];
            let dz = ca[2] - cb[2];
            let len = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-9);
            let overlap = 0.08;
            if let Ok(shifted) = k.translate(
                b,
                dx / len * overlap,
                dy / len * overlap,
                dz / len * overlap,
            ) {
                if let Ok(raw) = k.fuse(a, shifted) {
                    if let Some(ok) = valid_drawable(k, raw) {
                        return Ok(ok);
                    }
                }
            }
        }

        k.make_compound(&[a, b]).map_err(|e| {
            occt_err(format!(
                "{ctx}: fuse failed and compound fallback failed: {e}"
            ))
        })
    }

    fn valid_drawable(k: &mut occt_wasm::OcctKernel, raw: Handle) -> Option<Handle> {
        let d = drawable_shape(k, raw);
        shape_has_extent(k, d).then_some(d)
    }

    /// Fuse leftover solids in a compound so a body of overlapping bosses is one part.
    fn coalesce_solids(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
    ) -> Result<Handle, KernelError> {
        let ids = k.get_sub_shapes(shape, "solid").unwrap_or_default();
        if ids.len() <= 1 {
            return Ok(shape);
        }
        let handles: Vec<Handle> = ids.into_iter().map(id_to_handle).collect();
        if let Ok(fused) = k.fuse_all(&handles) {
            return Ok(drawable_shape(k, fused));
        }
        let mut acc = handles[0];
        for &part in &handles[1..] {
            acc = fuse_robust(k, acc, part, "coalesce")?;
        }
        Ok(acc)
    }

    /// Rotate a solid, or each solid in a compound. OCCT rotate on a multi-solid
    /// compound has trapped the wasm heap; rotating pieces then compounding is safe.
    fn rotate_shape(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        px: f64,
        py: f64,
        pz: f64,
        dx: f64,
        dy: f64,
        dz: f64,
        angle_rad: f64,
        ctx: &str,
    ) -> Result<Handle, KernelError> {
        let solids = k.get_sub_shapes(shape, "solid").unwrap_or_default();
        if solids.len() <= 1 {
            return k
                .rotate(shape, px, py, pz, dx, dy, dz, angle_rad)
                .map_err(|e| occt_err(format!("{ctx}: {e}")));
        }
        let mut rotated = Vec::with_capacity(solids.len());
        for id in solids {
            let part = k
                .rotate(id_to_handle(id), px, py, pz, dx, dy, dz, angle_rad)
                .map_err(|e| occt_err(format!("{ctx}: {e}")))?;
            rotated.push(part);
        }
        k.make_compound(&rotated)
            .map_err(|e| occt_err(format!("{ctx} compound: {e}")))
    }

    /// Cylinder/cone primitives are built along +Z. Rotate onto X or Y if asked.
    fn align_z_primitive_to_axis(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        axis: &RevolveAxis,
    ) -> Result<Handle, KernelError> {
        let a = std::f64::consts::FRAC_PI_2;
        match axis {
            RevolveAxis::Z => Ok(shape),
            RevolveAxis::X => rotate_shape(
                k,
                shape,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                a,
                "align cylinder to X",
            ),
            RevolveAxis::Y => rotate_shape(
                k,
                shape,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                -a,
                "align cylinder to Y",
            ),
        }
    }

    fn axis_dir(axis: &RevolveAxis) -> (f64, f64, f64) {
        match axis {
            RevolveAxis::X => (1.0, 0.0, 0.0),
            RevolveAxis::Y => (0.0, 1.0, 0.0),
            RevolveAxis::Z => (0.0, 0.0, 1.0),
        }
    }

    /// Map construction-plane UV into the XY coords used *before* `rotate_to_plane`.
    /// After that rotation, (u, v) lands as:
    ///   XY → (u, v, 0)
    ///   XZ → (u, 0, v)
    ///   YZ → (0, u, v)
    fn map_uv(plane: &SketchPlane, u: f64, v: f64) -> [f64; 2] {
        match plane {
            SketchPlane::XY => [u, v],
            SketchPlane::XZ => [u, -v],
            SketchPlane::YZ => [-v, u],
        }
    }

    /// 2-D transform on an XY face so that a later `rotate_to_plane` preserves UV.
    fn remap_face_uv_for_plane(
        k: &mut occt_wasm::OcctKernel,
        face: Handle,
        plane: &SketchPlane,
    ) -> Result<Handle, KernelError> {
        match plane {
            SketchPlane::XY => Ok(face),
            // (u, v) → (u, -v)
            SketchPlane::XZ => k
                .rotate(face, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, PI)
                .map_err(|e| occt_err(format!("remap UV for XZ: {:?}", e))),
            // (u, v) → (-v, u)
            SketchPlane::YZ => k
                .rotate(face, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, PI / 2.0)
                .map_err(|e| occt_err(format!("remap UV for YZ: {:?}", e))),
        }
    }

    /// Put an XY sketch face onto the construction plane, preserving UV:
    ///   XZ: (u, v, 0) → (u, 0, v)
    ///   YZ: (u, v, 0) → (0, u, v)
    /// This is *not* `rotate_to_plane` (that maps +Z to the plane normal for extrude).
    fn place_sketch_on_plane(
        k: &mut occt_wasm::OcctKernel,
        face: Handle,
        plane: &SketchPlane,
    ) -> Result<Handle, KernelError> {
        let a = std::f64::consts::FRAC_PI_2;
        match plane {
            SketchPlane::XY => Ok(face),
            SketchPlane::XZ => k
                .rotate(face, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, a)
                .map_err(|e| occt_err(format!("place sketch on XZ: {:?}", e))),
            SketchPlane::YZ => {
                let face = k
                    .rotate(face, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, a)
                    .map_err(|e| occt_err(format!("place sketch on YZ (rx): {:?}", e)))?;
                k.rotate(face, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, a)
                    .map_err(|e| occt_err(format!("place sketch on YZ (rz): {:?}", e)))
            }
        }
    }

    /// Rotate a +Z extrusion so the extrusion direction becomes the plane normal.
    fn rotate_to_plane(
        k: &mut occt_wasm::OcctKernel,
        shape: Handle,
        plane: &SketchPlane,
    ) -> Result<Handle, KernelError> {
        let a = std::f64::consts::FRAC_PI_2;
        match plane {
            SketchPlane::XY => Ok(shape),
            SketchPlane::XZ => k
                .rotate(shape, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -a)
                .map_err(|e| occt_err(format!("rotate_to_XZ: {:?}", e))),
            SketchPlane::YZ => k
                .rotate(shape, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, a)
                .map_err(|e| occt_err(format!("rotate_to_YZ: {:?}", e))),
        }
    }

    fn plane_normal(plane: &SketchPlane) -> [f64; 3] {
        match plane {
            SketchPlane::XY => [0.0, 0.0, 1.0],
            SketchPlane::XZ => [0.0, 1.0, 0.0],
            SketchPlane::YZ => [1.0, 0.0, 0.0],
        }
    }

    /// Catch the "flat washer" failure mode before we tessellate. A real tube
    /// has comparable extents in two directions; a disk is paper-thin in one.
    fn reject_if_planar(k: &mut occt_wasm::OcctKernel, solid: Handle) -> Result<(), KernelError> {
        let bb = k
            .get_bounding_box(solid, false)
            .map_err(|e| occt_err(format!("bbox: {:?}", e)))?;
        let dx = (bb.max.x - bb.min.x).abs();
        let dy = (bb.max.y - bb.min.y).abs();
        let dz = (bb.max.z - bb.min.z).abs();
        let max = dx.max(dy).max(dz);
        let min = dx.min(dy).min(dz);
        if max > 1.0 && min / max < 0.03 {
            return Err(KernelError::InvalidState(format!(
                "Result is nearly planar ({dx:.1}×{dy:.1}×{dz:.1}). That is a disk/washer, not a \
                 3-D tube. For a venturi/pipe/vase: sketch the half-section on XZ with points \
                 [radius, height] and revolve around Z — do not revolve around the sketch-plane normal."
            )));
        }
        Ok(())
    }

    pub(crate) fn list_topology_with_occt(
        program: &CadProgram,
    ) -> Result<crate::topology::TopologyReport, KernelError> {
        with_kernel(|k| {
            let solid =
                STEP_CACHE.with(|c| execute_in_kernel(k, program, &mut c.borrow_mut(), 0))?;
            let solid = heal_shape(k, solid);
            let face_ids = k
                .get_sub_shapes(solid, "face")
                .map_err(|e| occt_err(format!("faces: {:?}", e)))?;
            let edge_ids = k
                .get_sub_shapes(solid, "edge")
                .map_err(|e| occt_err(format!("edges: {:?}", e)))?;

            let mut faces = Vec::with_capacity(face_ids.len());
            for (index, &id) in face_ids.iter().enumerate() {
                let h = id_to_handle(id);
                let area = k.get_surface_area(h).unwrap_or(0.0);
                let center_v = k
                    .get_surface_center_of_mass(h)
                    .unwrap_or(vec![0.0, 0.0, 0.0]);
                let center = [
                    center_v.first().copied().unwrap_or(0.0),
                    center_v.get(1).copied().unwrap_or(0.0),
                    center_v.get(2).copied().unwrap_or(0.0),
                ];
                let uv = k.uv_bounds(h).unwrap_or(vec![0.0, 1.0, 0.0, 1.0]);
                let u_mid =
                    0.5 * (uv.first().copied().unwrap_or(0.0) + uv.get(1).copied().unwrap_or(1.0));
                let v_mid =
                    0.5 * (uv.get(2).copied().unwrap_or(0.0) + uv.get(3).copied().unwrap_or(1.0));
                let n = k
                    .surface_normal(h, u_mid, v_mid)
                    .unwrap_or(vec![0.0, 0.0, 1.0]);
                let normal = [
                    n.first().copied().unwrap_or(0.0),
                    n.get(1).copied().unwrap_or(0.0),
                    n.get(2).copied().unwrap_or(1.0),
                ];
                let surface_type = k.surface_type(h).unwrap_or_else(|_| "unknown".into());
                let mut tags = Vec::new();
                if normal[2].abs() > 0.85 {
                    if center[2]
                        >= faces
                            .iter()
                            .map(|f: &crate::topology::FaceInfo| f.center[2])
                            .fold(f64::NEG_INFINITY, f64::max)
                    {
                        tags.push("top_candidate".into());
                    }
                    if center[2]
                        <= faces
                            .iter()
                            .map(|f: &crate::topology::FaceInfo| f.center[2])
                            .fold(f64::INFINITY, f64::min)
                    {
                        tags.push("bottom_candidate".into());
                    }
                } else {
                    tags.push("side".into());
                }
                if surface_type.to_ascii_lowercase().contains("plane") {
                    tags.push("planar".into());
                }
                faces.push(crate::topology::FaceInfo {
                    index,
                    area,
                    center,
                    normal,
                    surface_type,
                    tags,
                });
            }

            let mut edges = Vec::with_capacity(edge_ids.len());
            for (index, &id) in edge_ids.iter().enumerate() {
                let h = id_to_handle(id);
                let length = k.get_length(h).unwrap_or(0.0);
                let bb = k.get_bounding_box(h, false).ok();
                let mid = bb
                    .map(|b| {
                        [
                            0.5 * (b.min.x + b.max.x),
                            0.5 * (b.min.y + b.max.y),
                            0.5 * (b.min.z + b.max.z),
                        ]
                    })
                    .unwrap_or([0.0; 3]);
                let curve_type = k.curve_type(h).unwrap_or_else(|_| "unknown".into());
                let mut tags = Vec::new();
                if curve_type.eq_ignore_ascii_case("line") {
                    tags.push("line".into());
                } else if curve_type.to_ascii_lowercase().contains("circle") {
                    tags.push("circle".into());
                }
                edges.push(crate::topology::EdgeInfo {
                    index,
                    length,
                    mid,
                    curve_type,
                    tags,
                });
            }

            let largest_face = faces
                .iter()
                .max_by(|a, b| {
                    a.area
                        .partial_cmp(&b.area)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|f| f.index);
            let top_face = faces
                .iter()
                .max_by(|a, b| {
                    a.center[2]
                        .partial_cmp(&b.center[2])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|f| f.index);
            let bottom_face = faces
                .iter()
                .min_by(|a, b| {
                    a.center[2]
                        .partial_cmp(&b.center[2])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|f| f.index);
            let longest_edge = edges
                .iter()
                .max_by(|a, b| {
                    a.length
                        .partial_cmp(&b.length)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|e| e.index);

            // Finalize top/bottom tags using the chosen indices.
            for f in &mut faces {
                if Some(f.index) == top_face {
                    f.tags.push("top".into());
                }
                if Some(f.index) == bottom_face {
                    f.tags.push("bottom".into());
                }
                if Some(f.index) == largest_face {
                    f.tags.push("largest".into());
                }
            }
            if let Some(i) = longest_edge {
                if let Some(e) = edges.get_mut(i) {
                    e.tags.push("longest".into());
                }
            }

            Ok(crate::topology::TopologyReport {
                summary: crate::topology::TopologySummary {
                    face_count: faces.len(),
                    edge_count: edges.len(),
                    largest_face,
                    top_face,
                    bottom_face,
                    longest_edge,
                    tip:
                        "Use face: \"largest\"|\"top\"|\"bottom\"|<index> on cut/fuse/hole/sketch. \
                          Use edges: \"all\"|\"top\"|\"longest\"|[indices] on fillet/chamfer. \
                          Pattern holes with scope:\"feature\" after hole/cut."
                            .into(),
                },
                faces,
                edges,
            })
        })
    }
}

// ── Shared geometry utilities ─────────────────────────────────────────────────

/// Compute per-vertex normals from an indexed triangle mesh by averaging
/// adjacent face normals. Works with both the OCCT and mock backends.
pub fn compute_normals_indexed(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let vc = positions.len() / 3;
    let mut normals = vec![0.0f32; positions.len()];

    for tri in indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let (i0, i1, i2) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let p = |i: usize| [positions[i * 3], positions[i * 3 + 1], positions[i * 3 + 2]];
        let (p0, p1, p2) = (p(i0), p(i1), p(i2));
        let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        for &vi in &[i0, i1, i2] {
            normals[vi * 3] += n[0];
            normals[vi * 3 + 1] += n[1];
            normals[vi * 3 + 2] += n[2];
        }
    }

    for chunk in normals.chunks_mut(3) {
        let len = (chunk[0] * chunk[0] + chunk[1] * chunk[1] + chunk[2] * chunk[2]).sqrt();
        if len > 1e-9 {
            chunk[0] /= len;
            chunk[1] /= len;
            chunk[2] /= len;
        }
    }

    let _ = vc; // suppress unused if no indexing path needed
    normals
}

/// Combine several tessellations into one mesh (index-offset).
pub fn combine_meshes(meshes: &[&MeshData]) -> MeshData {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut vertex_offset = 0u32;
    let mut any_indexed = false;
    for mesh in meshes {
        if !mesh.indices.is_empty() {
            any_indexed = true;
        }
    }
    for mesh in meshes {
        let vcount = (mesh.positions.len() / 3) as u32;
        positions.extend_from_slice(&mesh.positions);
        if mesh.normals.len() == mesh.positions.len() {
            normals.extend_from_slice(&mesh.normals);
        } else {
            normals.extend(std::iter::repeat(0.0).take(mesh.positions.len()));
        }
        if any_indexed {
            if mesh.indices.is_empty() {
                for i in 0..vcount {
                    indices.push(vertex_offset + i);
                }
            } else {
                indices.extend(mesh.indices.iter().map(|i| i + vertex_offset));
            }
        }
        vertex_offset += vcount;
    }
    MeshData {
        positions,
        normals,
        indices,
    }
}

pub(crate) fn combine_metrics(parts: &[&MetricsData]) -> MetricsData {
    if parts.is_empty() {
        return MetricsData {
            volume: 0.0,
            bbox: [0.0; 6],
            surface_area: 0.0,
            is_solid: false,
        };
    }
    let mut bbox = parts[0].bbox;
    let mut volume = 0.0;
    let mut surface_area = 0.0;
    let mut is_solid = true;
    for m in parts {
        volume += m.volume;
        surface_area += m.surface_area;
        is_solid = is_solid && m.is_solid;
        bbox[0] = bbox[0].min(m.bbox[0]);
        bbox[1] = bbox[1].min(m.bbox[1]);
        bbox[2] = bbox[2].min(m.bbox[2]);
        bbox[3] = bbox[3].max(m.bbox[3]);
        bbox[4] = bbox[4].max(m.bbox[4]);
        bbox[5] = bbox[5].max(m.bbox[5]);
    }
    MetricsData {
        volume,
        bbox,
        surface_area,
        is_solid,
    }
}

fn document_output_from_bodies(bodies: Vec<BodyOutput>) -> DocumentOutput {
    let visible: Vec<&MetricsData> = bodies
        .iter()
        .filter(|b| b.visible && !b.suppressed)
        .map(|b| &b.metrics)
        .collect();
    let metrics = combine_metrics(&visible);
    DocumentOutput { bodies, metrics }
}

/// Compute axis-aligned bounding box from flat position array.
pub fn bbox_from_positions(positions: &[f32]) -> [f64; 6] {
    if positions.is_empty() {
        return [0.0; 6];
    }
    let mut mn = [f32::MAX; 3];
    let mut mx = [f32::MIN; 3];
    for chunk in positions.chunks(3) {
        for i in 0..3 {
            mn[i] = mn[i].min(chunk[i]);
            mx[i] = mx[i].max(chunk[i]);
        }
    }
    [
        mn[0] as f64,
        mn[1] as f64,
        mn[2] as f64,
        mx[0] as f64,
        mx[1] as f64,
        mx[2] as f64,
    ]
}

#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::ir::CadDocument;

    #[test]
    fn execute_document_returns_one_mesh_per_body() {
        let doc: CadDocument = serde_json::from_str(
            r#"{
            "documentId": "assembly",
            "units": "mm",
            "bodies": [
                { "bodyId": "body_a", "name": "A", "features": [{ "op": "box", "size": [10, 10, 10] }] },
                {
                    "bodyId": "body_b",
                    "name": "B",
                    "transform": { "position": [20, 0, 0], "rotation": [0, 0, 0] },
                    "features": [{ "op": "box", "size": [4, 4, 4] }]
                }
            ]
        }"#,
        )
        .unwrap();
        let out = Engine::default().execute_document(&doc).unwrap();
        assert_eq!(out.bodies.len(), 2);
        assert_eq!(out.bodies[0].body_id, "body_a");
        assert_eq!(out.bodies[1].body_id, "body_b");
        assert!(!out.bodies[0].mesh.positions.is_empty());
        assert!(!out.bodies[1].mesh.positions.is_empty());
    }
}
