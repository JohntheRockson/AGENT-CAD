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
            return Err(KernelError::InvalidState("Document produced no visible solid".into()));
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
        document.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::execute_document_with_occt(document);
            #[cfg(not(feature = "occt"))]
            let _ = self.use_occt;
        }
        mock_backend::execute_document_with_mock(document)
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
        document.validate()?;

        if self.use_occt {
            #[cfg(feature = "occt")]
            return occt_backend::export_document_with_occt(document, format);
        }
        mock_backend::export_document_with_mock(document, format)
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
                    w = (op.radius + op.section_diameter) * 2.0;
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

    pub fn execute_document_with_mock(document: &CadDocument) -> Result<DocumentOutput, KernelError> {
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
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    use std::f64::consts::PI;
    use crate::ir::*;
    use super::*;

    // Maximum number of cached shape states kept per thread.
    // Each entry is tiny (2 Option<u32> + an enum), but the WASM arena holding
    // the actual shapes grows with each unique result — cap to bound memory.
    const CACHE_LIMIT: usize = 64;

    /// Snapshot of execution state after one feature step, keyed by cumulative hash.
    #[derive(Clone)]
    struct StepEntry {
        face:  Option<u32>,   // raw arena ID, not ShapeHandle (avoids Send/Sync issues)
        solid: Option<u32>,
        plane: SketchPlane,
    }

    thread_local! {
        static KERNEL: RefCell<Option<occt_wasm::OcctKernel>> = const { RefCell::new(None) };
        /// Maps (cumulative feature hash) → state after that feature.
        static STEP_CACHE: RefCell<HashMap<u64, StepEntry>> = RefCell::new(HashMap::new());
    }

    fn is_fatal_occt(err: &KernelError) -> bool {
        let s = err.to_string().to_lowercase();
        s.contains("out of bounds")
            || s.contains("wasm trap")
            || s.contains("wasm runtime")
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

    /// After a wasm trap the instance is dead. Rebuild once on a fresh kernel
    /// so a poisoned thread-local does not burn the rest of the chat.
    fn with_kernel_retry<T>(f: impl Fn() -> Result<T, KernelError>) -> Result<T, KernelError> {
        match f() {
            Ok(v) => Ok(v),
            Err(e) if is_fatal_occt(&e) => f(),
            Err(e) => Err(e),
        }
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
    fn unwrap_to_solid(k: &mut occt_wasm::OcctKernel, shape: occt_wasm::ShapeHandle) -> occt_wasm::ShapeHandle {
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
    fn heal_shape(k: &mut occt_wasm::OcctKernel, shape: occt_wasm::ShapeHandle) -> occt_wasm::ShapeHandle {
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
        ids.iter().filter_map(|&id| {
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
        }).collect()
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
        edges.sort_by(|a, b| b.length.partial_cmp(&a.length).unwrap_or(std::cmp::Ordering::Equal));
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
        if out.is_empty() { ids } else { out }
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
        let diag = (ext[0] * ext[0] + ext[1] * ext[1] + ext[2] * ext[2]).sqrt().max(1.0);
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
    const KERNEL_SEMANTICS: u64 = 0xA6E1_CAD0_0000_0007;

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
        features.iter().map(|feat| {
            let json = serde_json::to_string(feat).unwrap_or_default();
            let mut h = DefaultHasher::new();
            acc.hash(&mut h);
            json.hash(&mut h);
            acc = h.finish();
            acc
        }).collect()
    }

    // ── State machine ─────────────────────────────────────────────────────────

    type Handle = occt_wasm::ShapeHandle;

    struct ExecState {
        current_face: Option<Handle>,
        current_solid: Option<Handle>,
        active_plane: SketchPlane,
    }

    impl Default for ExecState {
        fn default() -> Self {
            ExecState {
                current_face: None,
                current_solid: None,
                active_plane: SketchPlane::XY,
            }
        }
    }

    pub fn execute_with_occt(program: &CadProgram) -> Result<ModelOutput, KernelError> {
        with_kernel_retry(|| {
            with_kernel(|k| {
                let solid = STEP_CACHE.with(|c| {
                    execute_in_kernel(k, program, &mut c.borrow_mut(), 0)
                })?;
                tessellate_solid(k, solid)
            })
        })
    }

    fn tessellate_solid(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
    ) -> Result<ModelOutput, KernelError> {
        let solid = heal_shape(k, solid);
        reject_if_planar(k, solid)?;
        let tess = tessellate_with(k, solid, 0.002, 0.25)?;
        let bbox = bbox_from_positions(&tess.positions);
        let volume = k
            .get_volume(solid)
            .map_err(|e| occt_err(format!("get_volume: {:?}", e)))?;
        let surface_area = k.get_surface_area(solid).unwrap_or(0.0);
        Ok(ModelOutput {
            mesh: MeshData {
                positions: tess.positions,
                normals: tess.normals,
                indices: tess.indices,
            },
            metrics: MetricsData {
                volume,
                bbox,
                surface_area,
                is_solid: true,
            },
        })
    }

    /// Relative chordal tolerance first; fall back to an absolute deflection
    /// derived from the bounding-box diagonal if the relative mesher fails.
    fn tessellate_with(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
        relative: f64,
        angular: f64,
    ) -> Result<occt_wasm::Mesh, KernelError> {
        match k.tessellate_relative(solid, relative, angular) {
            Ok(mut mesh) => {
                if let Ok(bb) = k.get_bounding_box(solid, false) {
                    strip_spike_triangles(&mut mesh, &bb);
                }
                Ok(mesh)
            }
            Err(_) => {
                let linear = linear_from_bbox(k, solid, relative);
                let mut mesh = k
                    .tessellate(solid, linear, angular)
                    .map_err(|e| occt_err(format!("tessellate: {:?}", e)))?;
                if let Ok(bb) = k.get_bounding_box(solid, false) {
                    strip_spike_triangles(&mut mesh, &bb);
                }
                Ok(mesh)
            }
        }
    }

    fn linear_from_bbox(k: &mut occt_wasm::OcctKernel, solid: Handle, fraction: f64) -> f64 {
        k.get_bounding_box(solid, false)
            .ok()
            .map(|b| {
                let dx = b.max.x - b.min.x;
                let dy = b.max.y - b.min.y;
                let dz = b.max.z - b.min.z;
                let diag = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);
                (diag * fraction).clamp(0.02, 2.0)
            })
            .unwrap_or(0.1)
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

    pub fn execute_document_with_occt(document: &CadDocument) -> Result<DocumentOutput, KernelError> {
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
                    let solid = execute_in_kernel(k, &prog, &mut cache, body_cache_ns(&body.body_id))?;
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
                            BodyRefOp::Fuse => k
                                .fuse(*target, tool)
                                .map_err(|e| occt_err(format!("body fuse: {:?}", e)))?,
                        };
                        *target = drawable_shape(k, raw);
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
                    let out = tessellate_solid(k, solid)?;
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
                    let solid = execute_in_kernel(k, &prog, &mut cache, body_cache_ns(&body.body_id))?;
                    let solid = drawable_shape(k, solid);
                    let solid = apply_body_transform(k, solid, &body.transform)?;
                    combined = Some(match combined {
                        None => solid,
                        Some(acc) => {
                            let raw = k
                                .fuse(acc, solid)
                                .map_err(|e| occt_err(format!("export fuse: {:?}", e)))?;
                            drawable_shape(k, raw)
                        }
                    });
                }
                let solid = combined.ok_or_else(|| {
                    KernelError::InvalidState("nothing visible to export".into())
                })?;
                let solid = heal_shape(k, solid);
                match format {
                    ExportFormat::Step => k
                        .export_step(solid)
                        .map(|s| s.into_bytes())
                        .map_err(|e| occt_err(format!("export_step: {:?}", e))),
                    ExportFormat::Stl => {
                        let tess = tessellate_with(k, solid, 0.001, 0.5)?;
                        Ok(crate::export::to_stl(&MeshData {
                            positions: tess.positions,
                            normals: tess.normals,
                            indices: tess.indices,
                        }))
                    }
                    ExportFormat::Gltf => {
                        let tess = tessellate_with(k, solid, 0.001, 0.3)?;
                        Ok(mesh_to_glb(&tess))
                    }
                    ExportFormat::Obj => {
                        let tess = tessellate_with(k, solid, 0.001, 0.5)?;
                        Ok(crate::export::to_obj(&MeshData {
                            positions: tess.positions,
                            normals: tess.normals,
                            indices: tess.indices,
                        })
                        .into_bytes())
                    }
                    ExportFormat::Brep => k
                        .to_brep(solid)
                        .map(|s| s.into_bytes())
                        .map_err(|e| occt_err(format!("to_brep: {:?}", e))),
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
            let solid = STEP_CACHE.with(|c| {
                execute_in_kernel(k, program, &mut c.borrow_mut(), 0)
            })?;
            let solid = heal_shape(k, solid);
            match format {
                ExportFormat::Step => k
                    .export_step(solid)
                    .map(|s| s.into_bytes())
                    .map_err(|e| occt_err(format!("export_step: {:?}", e))),
                ExportFormat::Stl => {
                    let tess = tessellate_with(k, solid, 0.001, 0.5)?;
                    Ok(crate::export::to_stl(&MeshData {
                        positions: tess.positions,
                        normals: tess.normals,
                        indices: tess.indices,
                    }))
                }
                ExportFormat::Gltf => {
                    let tess = tessellate_with(k, solid, 0.001, 0.3)?;
                    Ok(mesh_to_glb(&tess))
                }
                ExportFormat::Obj => {
                    let tess = tessellate_with(k, solid, 0.001, 0.5)?;
                    Ok(crate::export::to_obj(&MeshData {
                        positions: tess.positions,
                        normals: tess.normals,
                        indices: tess.indices,
                    })
                    .into_bytes())
                }
                ExportFormat::Brep => k
                    .to_brep(solid)
                    .map(|s| s.into_bytes())
                    .map_err(|e| occt_err(format!("to_brep: {:?}", e))),
            }
        })
    }

    /// Build a minimal binary glTF (GLB 2.0) from a tessellated mesh.
    fn mesh_to_glb(mesh: &occt_wasm::Mesh) -> Vec<u8> {
        let vertex_count = mesh.positions.len() / 3;
        let index_count = mesh.indices.len();

        let pos_bytes: Vec<u8> = mesh.positions.iter().flat_map(|f| f.to_le_bytes()).collect();
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
        let (mut mx_x, mut mx_y, mut mx_z) = (f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for chunk in mesh.positions.chunks(3) {
            if chunk.len() == 3 {
                mn_x = mn_x.min(chunk[0]); mx_x = mx_x.max(chunk[0]);
                mn_y = mn_y.min(chunk[1]); mx_y = mx_y.max(chunk[1]);
                mn_z = mn_z.min(chunk[2]); mx_z = mx_z.max(chunk[2]);
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
            vc = vertex_count, ic = index_count,
            mn_x = mn_x, mn_y = mn_y, mn_z = mn_z,
            mx_x = mx_x, mx_y = mx_y, mx_z = mx_z,
            pl = pos_len, no = nrm_offset, nl = nrm_len,
            io = idx_offset, il = idx_len, bl = bin_len,
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
                state.current_face  = e.face.map(id_to_handle);
                state.current_solid = e.solid.map(id_to_handle);
                state.active_plane  = e.plane.clone();
                resume_from = i + 1;
            } else {
                break;
            }
        }

        // Execute only the uncached suffix.
        for i in resume_from..program.features.len() {
            match &program.features[i] {
                Feature::Sketch(op)        => handle_sketch(k, &mut state, op)?,
                Feature::Extrude(op)       => handle_extrude(k, &mut state, op)?,
                Feature::Revolve(op)       => handle_revolve(k, &mut state, op)?,
                Feature::Cut(op)           => handle_cut(k, &mut state, op)?,
                Feature::Fuse(op)          => handle_fuse(k, &mut state, op)?,
                Feature::Hole(op)          => handle_hole(k, &mut state, op)?,
                Feature::Fillet(op)        => handle_fillet(k, &mut state, op)?,
                Feature::Chamfer(op)       => handle_chamfer(k, &mut state, op)?,
                Feature::Transform(op)     => handle_transform(k, &mut state, op)?,
                Feature::Box(op)           => handle_box(k, &mut state, op)?,
                Feature::Cylinder(op)      => handle_cylinder(k, &mut state, op)?,
                Feature::Sphere(op)        => handle_sphere(k, &mut state, op)?,
                Feature::Cone(op)          => handle_cone(k, &mut state, op)?,
                Feature::Torus(op)         => handle_torus(k, &mut state, op)?,
                Feature::Loft(op)          => handle_loft(k, &mut state, op)?,
                Feature::Mirror(op)        => handle_mirror(k, &mut state, op)?,
                Feature::Pattern(op)       => handle_pattern(k, &mut state, op)?,
                Feature::Shell(op)         => handle_shell(k, &mut state, op)?,
                Feature::DraftExtrude(op)  => handle_draft_extrude(k, &mut state, op)?,
                Feature::Thread(op)        => handle_thread(k, &mut state, op, &program.units)?,
                Feature::Sweep(op)         => handle_sweep(k, &mut state, op)?,
                Feature::Helix(op)         => handle_helix(k, &mut state, op)?,
                Feature::Offset(op)        => handle_offset(k, &mut state, op)?,
                Feature::Thicken(op)       => handle_thicken(k, &mut state, op)?,
                Feature::Common(op)        => handle_common(k, &mut state, op)?,
                Feature::Ellipsoid(op)     => handle_ellipsoid(k, &mut state, op)?,
                Feature::Draft(op)         => handle_draft(k, &mut state, op)?,
            }

            // Evict oldest entry when the cache is full.
            if cache.len() >= CACHE_LIMIT {
                if let Some(&oldest) = cache.keys().next() {
                    cache.remove(&oldest);
                }
            }
            cache.insert(hashes[i], StepEntry {
                face:  state.current_face.map(handle_to_id),
                solid: state.current_solid.map(handle_to_id),
                plane: state.active_plane.clone(),
            });
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
        // Always build the profile on XY. The plane is applied after the 3-D
        // operation (extrude/revolve/draft) so OCCT never has to prism a
        // face along +Y/+X — that path produced perpendicular "cross" solids.
        let face = make_profile_face(k, &op.profile, op.origin)?;
        state.current_face = Some(face);
        state.active_plane = op.plane.clone();
        Ok(())
    }

    fn handle_extrude(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ExtrudeOp,
    ) -> Result<(), KernelError> {
        let face = state
            .current_face
            .ok_or_else(|| KernelError::InvalidState("extrude requires a preceding sketch".into()))?;

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

        let solid = rotate_to_plane(k, solid, &state.active_plane)?;
        join_or_set(k, state, solid)
    }

    fn handle_revolve(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &RevolveOp,
    ) -> Result<(), KernelError> {
        let face = state
            .current_face
            .ok_or_else(|| KernelError::InvalidState("revolve requires a preceding sketch".into()))?;

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

        let tool = make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, op.through, Some(base))?;
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
        let addend = make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, false, None)?;
        join_or_set(k, state, addend)
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
        let cyl = rotate_to_plane(k, cyl, &op.plane)?;

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
            crate::ir::EdgeSelection::Named(_) => edge_ids,
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
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("chamfer requires an existing solid".into()))?;
        let solid = unwrap_to_solid(k, solid);

        let edge_ids = k
            .get_sub_shapes(solid, "edge")
            .map_err(|e| occt_err(format!("get_sub_shapes (chamfer): {:?}", e)))?;

        if edge_ids.is_empty() {
            return Ok(());
        }

        let candidate_ids: Vec<u32> = match &op.edges {
            crate::ir::EdgeSelection::Named(_) => edge_ids,
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
        let mut shape = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("transform requires an existing solid".into()))?;

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
        let solid = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("pattern requires an existing solid".into()))?;
        let count = op.count as i32;
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
            let rod = threaded_rod(k, major, pitch, length)?;
            let rod = maybe_left_hand(k, rod, &op.hand)?;
            let rod = align_z_primitive_to_axis(k, rod, &op.axis)?;
            let rod = translate_if_needed(k, rod, op.at)?;
            if state.current_solid.is_some() {
                // Thread an existing boss: cut the groove tool from the body.
                let base = state.current_solid.unwrap();
                let cutter = thread_cutter(k, major, pitch, length, false, 0.0)?;
                let cutter = maybe_left_hand(k, cutter, &op.hand)?;
                let cutter = align_z_primitive_to_axis(k, cutter, &op.axis)?;
                let cutter = translate_if_needed(k, cutter, op.at)?;
                match k.cut(base, cutter) {
                    Ok(raw) => {
                        state.current_solid = Some(drawable_shape(k, raw));
                        Ok(())
                    }
                    Err(_) => join_or_set(k, state, rod),
                }
            } else {
                join_or_set(k, state, rod)
            }
        }
    }

    fn thread_dims(op: &ThreadOp, units: &Units) -> Result<(f64, f64), KernelError> {
        let inch = matches!(units, Units::Inch);
        if let Some(size) = op.size.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let spec = crate::thread::parse_size(size)
                .map_err(KernelError::InvalidState)?;
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

    /// External threaded cylinder along +Z from the origin (revolve fallback is the
    /// reliable path; a helical pipe is tried first).
    fn threaded_rod(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
    ) -> Result<Handle, KernelError> {
        let cyl = k
            .make_cylinder(major / 2.0, length)
            .map_err(|e| occt_err(format!("thread cylinder: {e}")))?;
        if let Ok(cutter) = thread_cutter(k, major, pitch, length, false, 0.0) {
            if let Ok(cut) = k.cut(cyl, cutter) {
                return Ok(drawable_shape(k, cut));
            }
        }
        revolved_thread_solid(k, major, pitch, length)
    }

    fn thread_cutter(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
        internal: bool,
        z0: f64,
    ) -> Result<Handle, KernelError> {
        helical_thread_cutter(k, major, pitch, length, internal, z0)
            .or_else(|_| revolved_thread_cutter(k, major, pitch, length, internal, z0))
    }

    fn helical_thread_cutter(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
        internal: bool,
        z0: f64,
    ) -> Result<Handle, KernelError> {
        let depth = crate::thread::external_depth(pitch);
        let half = pitch * 0.5;
        let (r_out, r_in) = if internal {
            let r_hole = crate::thread::tap_drill_diameter(major, pitch) / 2.0;
            (major / 2.0, (r_hole - 0.12 * pitch).max(0.05))
        } else {
            (major / 2.0 + 0.18 * pitch, major / 2.0 - depth)
        };
        let r_h = 0.5 * (r_out + r_in);
        let z_start = z0 - half;
        let pts = [
            [r_out, 0.0, z_start],
            [r_out, 0.0, z_start + pitch],
            [r_in, 0.0, z_start + half],
        ];
        let face = face_from_polygon_3d(k, &pts)?;
        let height = length + pitch;
        let spine = k
            .make_helix_wire(0.0, 0.0, z_start, 0.0, 0.0, 1.0, pitch, height, r_h)
            .map_err(|e| occt_err(format!("thread helix: {e}")))?;
        pipe_along(k, face, spine)
    }

    fn revolved_thread_cutter(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
        internal: bool,
        z0: f64,
    ) -> Result<Handle, KernelError> {
        let depth = crate::thread::external_depth(pitch);
        let n = ((length / pitch).ceil() as i32 + 2).max(2);
        let (r_a, r_b) = if internal {
            let r_hole = crate::thread::tap_drill_diameter(major, pitch) / 2.0;
            ((r_hole - 0.08 * pitch).max(0.05), major / 2.0 + 0.05 * pitch)
        } else {
            (major / 2.0 + 0.2 * pitch, major / 2.0 - depth)
        };
        let mut pts: Vec<[f64; 2]> = Vec::new();
        let z_lo = z0 - pitch;
        pts.push([r_a, z_lo]);
        for i in 0..=n {
            let z = z_lo + i as f64 * pitch;
            pts.push([r_a, z]);
            pts.push([r_b, z + pitch * 0.5]);
        }
        let z_hi = z_lo + (n as f64 + 1.0) * pitch;
        pts.push([r_a, z_hi]);
        let pad = (r_a - r_b).abs() + pitch;
        if internal {
            pts.push([(r_a - pad).max(0.02), z_hi]);
            pts.push([(r_a - pad).max(0.02), z_lo]);
        } else {
            pts.push([r_a + pad, z_hi]);
            pts.push([r_a + pad, z_lo]);
        }
        revolve_xz_polyline(k, &pts)
    }

    fn revolved_thread_solid(
        k: &mut occt_wasm::OcctKernel,
        major: f64,
        pitch: f64,
        length: f64,
    ) -> Result<Handle, KernelError> {
        let depth = crate::thread::external_depth(pitch);
        let r_crest = major / 2.0;
        let r_root = (r_crest - depth).max(major * 0.15);
        let mut pts: Vec<[f64; 2]> = vec![[0.0, 0.0], [0.0, length], [r_crest, length]];
        let mut z = length;
        while z > 1e-9 {
            let z_mid = (z - pitch * 0.5).max(0.0);
            let z_next = (z - pitch).max(0.0);
            pts.push([r_root, z_mid]);
            pts.push([r_crest, z_next]);
            if z_next <= 0.0 {
                break;
            }
            z = z_next;
        }
        revolve_xz_polyline(k, &pts)
    }

    fn revolve_xz_polyline(
        k: &mut occt_wasm::OcctKernel,
        pts: &[[f64; 2]],
    ) -> Result<Handle, KernelError> {
        let profile = Profile::Polyline(PolylineProfile {
            points: pts.to_vec(),
            closed: true,
        });
        let face = make_profile_face(k, &profile, [0.0, 0.0])?;
        let face = place_sketch_on_plane(k, face, &SketchPlane::XZ)?;
        k.revolve(face, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0 * PI)
            .map_err(|e| occt_err(format!("revolve thread: {e}")))
            .map(|s| unwrap_to_solid(k, s))
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
        let spine = match &op.path {
            SweepPath::Helix(h) => helix_spine(k, h.pitch, h.height, h.radius, h.at, &h.axis)?,
            SweepPath::Polyline(p) => wire_from_polyline3(k, &p.points)?,
        };
        let solid = pipe_along(k, profile, spine)?;
        join_or_set(k, state, solid)
    }

    fn handle_helix(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &HelixOp,
    ) -> Result<(), KernelError> {
        let sec_r = op.section_diameter / 2.0;
        let spine =
            helix_spine(k, op.pitch, op.height, op.radius, [0.0; 3], &RevolveAxis::Z)?;
        let solid = helix_solid(k, spine, op.radius, op.pitch, op.height, sec_r)?;
        let solid = align_z_primitive_to_axis(k, solid, &op.axis)?;
        let solid = translate_if_needed(k, solid, op.at)?;
        join_or_set(k, state, solid)
    }

    fn helix_solid(
        k: &mut occt_wasm::OcctKernel,
        spine: Handle,
        radius: f64,
        pitch: f64,
        height: f64,
        sec_r: f64,
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
            let rect = k
                .translate(rect, -sec_r, -sec_r, 0.0)
                .unwrap_or(rect);
            if let Ok(s) = pipe_along(k, rect, spine) {
                return Ok(s);
            }
        }
        // Approximate the helix with a polyline (C0) and sweep with round corners.
        let path = helix_polyline(radius, pitch, height, 24);
        if let Ok(poly) = wire_from_polyline3(k, &path) {
            if let Ok(rect) = k.make_rectangle(sec_d, sec_d) {
                let rect = k
                    .translate(rect, -sec_r, -sec_r, 0.0)
                    .unwrap_or(rect);
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
        // Last resort: stacked torus rings so the op still yields a coil-like solid.
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

    fn handle_thicken(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &ThickenOp,
    ) -> Result<(), KernelError> {
        let shape = state.current_solid.ok_or_else(|| {
            KernelError::InvalidState("thicken requires an existing solid or shell".into())
        })?;
        let result = k
            .thicken(shape, op.thickness, 1e-3)
            .map_err(|e| occt_err(format!("thicken: {e}")))?;
        set_solid(state, unwrap_to_solid(k, result));
        Ok(())
    }

    fn handle_common(
        k: &mut occt_wasm::OcctKernel,
        state: &mut ExecState,
        op: &CommonOp,
    ) -> Result<(), KernelError> {
        let base = state
            .current_solid
            .ok_or_else(|| KernelError::InvalidState("common requires an existing solid".into()))?;
        let tool = make_tool_solid(k, &op.profile, op.depth, op.at, &op.plane, false, None)?;
        let raw = k
            .common(base, tool)
            .map_err(|e| occt_err(format!("common: {e}")))?;
        state.current_solid = Some(drawable_shape(k, raw));
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
            .map_err(|e| occt_err(format!("get_sub_shapes (draft): {e}")))?;
        let (dx, dy, dz) = axis_dir(&op.pull);
        let pull = [dx, dy, dz];
        let candidates: Vec<u32> = match &op.faces {
            crate::ir::EdgeSelection::Named(_) => face_ids
                .into_iter()
                .filter(|&id| {
                    let n = k
                        .surface_normal(id_to_handle(id), 0.5, 0.5)
                        .ok()
                        .filter(|v| v.len() >= 3)
                        .map(|v| [v[0], v[1], v[2]])
                        .unwrap_or([0.0, 0.0, 1.0]);
                    let dot = n[0] * pull[0] + n[1] * pull[1] + n[2] * pull[2];
                    dot.abs() < 0.85
                })
                .collect(),
            crate::ir::EdgeSelection::Indices(idxs) => idxs
                .iter()
                .filter_map(|&i| face_ids.get(i).copied())
                .collect(),
        };
        let angle_rad = op.angle * PI / 180.0;
        let mut current = solid;
        let mut any = false;
        for id in candidates {
            if let Ok(next) = k.draft(current, id_to_handle(id), angle_rad, dx, dy, dz) {
                current = unwrap_to_solid(k, next);
                any = true;
            }
        }
        if any {
            set_solid(state, current);
        }
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

    fn face_from_polygon_3d(
        k: &mut occt_wasm::OcctKernel,
        pts: &[[f64; 3]],
    ) -> Result<Handle, KernelError> {
        if pts.len() < 3 {
            return Err(KernelError::InvalidState(
                "polygon face needs at least 3 points".into(),
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
        let wire = k
            .make_wire(&edges)
            .map_err(|e| occt_err(format!("polygon wire: {e}")))?;
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
            let raw = k
                .fuse(base, solid)
                .map_err(|e| occt_err(format!("join: {e}")))?;
            drawable_shape(k, raw)
        } else {
            solid
        };
        set_solid(state, joined);
        Ok(())
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
            if let Ok(raw) = k.fuse(acc, part) {
                acc = drawable_shape(k, raw);
            }
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
            RevolveAxis::X => rotate_shape(k, shape, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, a, "align cylinder to X"),
            RevolveAxis::Y => {
                rotate_shape(k, shape, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -a, "align cylinder to Y")
            }
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
    fn reject_if_planar(
        k: &mut occt_wasm::OcctKernel,
        solid: Handle,
    ) -> Result<(), KernelError> {
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
        let p = |i: usize| {
            [
                positions[i * 3],
                positions[i * 3 + 1],
                positions[i * 3 + 2],
            ]
        };
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

fn combine_metrics(parts: &[&MetricsData]) -> MetricsData {
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
        mn[0] as f64, mn[1] as f64, mn[2] as f64,
        mx[0] as f64, mx[1] as f64, mx[2] as f64,
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
