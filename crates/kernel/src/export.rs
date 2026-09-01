//! Export utilities for geometry data.
//!
//! Binary STL and faceted STEP are implemented in pure Rust and work with both
//! the mock and OCCT backends. OCCT's WASM `export_step` traps on even a hex
//! prism (wasm memory); we never call it. glTF still tessellates via OCCT.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::engine::MeshData;

/// Encode `mesh` as a binary STL file (no external dependencies).
///
/// Handles both indexed (`indices.len() > 0`) and non-indexed (sequential
/// vertex array) meshes.
pub fn to_stl(mesh: &MeshData) -> Vec<u8> {
    let tri_count: usize = if mesh.indices.is_empty() {
        mesh.positions.len() / 9
    } else {
        mesh.indices.len() / 3
    };

    // 80-byte header + 4-byte count + 50 bytes per triangle
    let mut buf = Vec::with_capacity(84 + tri_count * 50);

    // 80-byte ASCII header
    let mut header = [b' '; 80];
    let tag = b"AgentCAD Binary STL";
    header[..tag.len()].copy_from_slice(tag);
    buf.extend_from_slice(&header);

    // Triangle count (u32 LE)
    buf.extend_from_slice(&(tri_count as u32).to_le_bytes());

    if mesh.indices.is_empty() {
        // Non-indexed: every 9 f32 values = one triangle (3 verts × 3 components)
        for chunk in mesh.positions.chunks(9) {
            if chunk.len() < 9 {
                break;
            }
            let v0 = [chunk[0], chunk[1], chunk[2]];
            let v1 = [chunk[3], chunk[4], chunk[5]];
            let v2 = [chunk[6], chunk[7], chunk[8]];
            let n = face_normal(&v0, &v1, &v2);
            write_triangle(&mut buf, &n, &v0, &v1, &v2);
        }
    } else {
        for tri in mesh.indices.chunks(3) {
            if tri.len() < 3 {
                break;
            }
            let v = |i: u32| {
                let s = i as usize * 3;
                [
                    mesh.positions[s],
                    mesh.positions[s + 1],
                    mesh.positions[s + 2],
                ]
            };
            let (v0, v1, v2) = (v(tri[0]), v(tri[1]), v(tri[2]));
            let n = face_normal(&v0, &v1, &v2);
            write_triangle(&mut buf, &n, &v0, &v1, &v2);
        }
    }

    buf
}

/// Encode `mesh` as Wavefront OBJ (ASCII).
pub fn to_obj(mesh: &MeshData) -> String {
    let vc = mesh.positions.len() / 3;
    let mut out = String::from("# AgentCAD\no model\n");
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            break;
        }
        out.push_str(&format!("v {} {} {}\n", chunk[0], chunk[1], chunk[2]));
    }
    if mesh.normals.len() >= vc * 3 {
        for chunk in mesh.normals.chunks(3).take(vc) {
            if chunk.len() < 3 {
                break;
            }
            out.push_str(&format!("vn {} {} {}\n", chunk[0], chunk[1], chunk[2]));
        }
    }
    if mesh.indices.is_empty() {
        let tris = vc / 3;
        for t in 0..tris {
            let a = t * 3 + 1;
            let b = a + 1;
            let c = a + 2;
            if mesh.normals.len() >= vc * 3 {
                out.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n"));
            } else {
                out.push_str(&format!("f {a} {b} {c}\n"));
            }
        }
    } else {
        for tri in mesh.indices.chunks(3) {
            if tri.len() < 3 {
                break;
            }
            let a = tri[0] + 1;
            let b = tri[1] + 1;
            let c = tri[2] + 1;
            if mesh.normals.len() >= vc * 3 {
                out.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n"));
            } else {
                out.push_str(&format!("f {a} {b} {c}\n"));
            }
        }
    }
    out
}

/// ISO-10303-21 faceted `MANIFOLD_SOLID_BREP` from a triangle mesh.
///
/// Used for STEP export because the OCCT WASM STEP writer traps (`export_step:
/// internal CAD kernel crash (wasm memory)`) on hex-only, hex+shank, and the
/// golden M8×40 bolt. The mesh is the same one the viewport / STL uses
/// (instanced short rods on a long thread). Empty tessellation yields no solid.
pub fn to_step(mesh: &MeshData) -> String {
    let tris = mesh_triangles(mesh);
    if tris.is_empty() {
        return String::new();
    }

    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut weld: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut faces: Vec<[u32; 3]> = Vec::new();
    for [a, b, c] in tris {
        let ia = weld_vertex(&mut verts, &mut weld, a);
        let ib = weld_vertex(&mut verts, &mut weld, b);
        let ic = weld_vertex(&mut verts, &mut weld, c);
        if ia == ib || ib == ic || ic == ia {
            continue;
        }
        let n = tri_normal(verts[ia as usize], verts[ib as usize], verts[ic as usize]);
        if n[0] * n[0] + n[1] * n[1] + n[2] * n[2] < 1e-20 {
            continue;
        }
        faces.push([ia, ib, ic]);
    }
    if faces.is_empty() {
        return String::new();
    }

    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    let mut edge_list: Vec<(u32, u32)> = Vec::new();
    for &[ia, ib, ic] in &faces {
        for (u, v) in [(ia, ib), (ib, ic), (ic, ia)] {
            let key = if u < v { (u, v) } else { (v, u) };
            if let std::collections::hash_map::Entry::Vacant(e) = edges.entry(key) {
                let id = edge_list.len() as u32;
                e.insert(id);
                edge_list.push(key);
            }
        }
    }

    let mut w = StepBuf::new(verts.len(), edge_list.len(), faces.len());
    w.header();

    let app = w.emit("APPLICATION_CONTEXT('core data for automotive mechanical design processes')");
    w.emit(&format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
    ));
    let prod_ctx = w.emit(&format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let prod = w.emit(&format!("PRODUCT('model','model','',(#{prod_ctx}))"));
    let pdf = w.emit(&format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
    let pdc = w.emit(&format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"
    ));
    let pd = w.emit(&format!("PRODUCT_DEFINITION('','',#{pdf},#{pdc})"));
    let pds = w.emit(&format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));

    let length_si = w.emit("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )");
    let angle_si = w.emit("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )");
    let solid_si = w.emit("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )");
    let unc = w.emit(&format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-6),#{length_si},'distance_accuracy_value','')"
    ));
    let ctx = w.emit(&format!(
        "( GEOMETRIC_REPRESENTATION_CONTEXT(3) \
           GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) \
           GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length_si},#{angle_si},#{solid_si})) \
           REPRESENTATION_CONTEXT('Context','3D') )"
    ));

    let mut cart: Vec<usize> = Vec::with_capacity(verts.len());
    let mut vpt: Vec<usize> = Vec::with_capacity(verts.len());
    for p in &verts {
        let c = w.emit(&format!(
            "CARTESIAN_POINT('',({:.8},{:.8},{:.8}))",
            p[0], p[1], p[2]
        ));
        cart.push(c);
        vpt.push(w.emit(&format!("VERTEX_POINT('',#{c})")));
    }

    let mut edge_ids: Vec<usize> = Vec::with_capacity(edge_list.len());
    for &(u, v) in &edge_list {
        let a = verts[u as usize];
        let b = verts[v as usize];
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let mag = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-15);
        let dir = w.emit(&format!(
            "DIRECTION('',({:.8},{:.8},{:.8}))",
            d[0] / mag,
            d[1] / mag,
            d[2] / mag
        ));
        let vec = w.emit(&format!("VECTOR('',#{dir},{mag:.8})"));
        let line = w.emit(&format!("LINE('',#{},#{vec})", cart[u as usize]));
        edge_ids.push(w.emit(&format!(
            "EDGE_CURVE('',#{},#{},#{line},.T.)",
            vpt[u as usize], vpt[v as usize]
        )));
    }

    let mut face_ids: Vec<usize> = Vec::with_capacity(faces.len());
    for &[ia, ib, ic] in &faces {
        let n = tri_normal(verts[ia as usize], verts[ib as usize], verts[ic as usize]);
        let nlen = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-15);
        let n = [n[0] / nlen, n[1] / nlen, n[2] / nlen];
        let e01 = verts[ib as usize][0] - verts[ia as usize][0];
        let e02 = verts[ib as usize][1] - verts[ia as usize][1];
        let e03 = verts[ib as usize][2] - verts[ia as usize][2];
        let mut rx = e01 - n[0] * (e01 * n[0] + e02 * n[1] + e03 * n[2]);
        let mut ry = e02 - n[1] * (e01 * n[0] + e02 * n[1] + e03 * n[2]);
        let mut rz = e03 - n[2] * (e01 * n[0] + e02 * n[1] + e03 * n[2]);
        let rlen = (rx * rx + ry * ry + rz * rz).sqrt();
        if rlen < 1e-12 {
            let (ax, ay, az) = if n[0].abs() < 0.9 {
                (1.0, 0.0, 0.0)
            } else {
                (0.0, 1.0, 0.0)
            };
            rx = ay * n[2] - az * n[1];
            ry = az * n[0] - ax * n[2];
            rz = ax * n[1] - ay * n[0];
        }
        let rlen = (rx * rx + ry * ry + rz * rz).sqrt().max(1e-15);
        let axis = w.emit(&format!(
            "DIRECTION('',({:.8},{:.8},{:.8}))",
            n[0], n[1], n[2]
        ));
        let refd = w.emit(&format!(
            "DIRECTION('',({:.8},{:.8},{:.8}))",
            rx / rlen,
            ry / rlen,
            rz / rlen
        ));
        let place = w.emit(&format!(
            "AXIS2_PLACEMENT_3D('',#{},#{axis},#{refd})",
            cart[ia as usize]
        ));
        let plane = w.emit(&format!("PLANE('',#{place})"));

        let mut oes = [0usize; 3];
        for (k, (u, v)) in [(ia, ib), (ib, ic), (ic, ia)].into_iter().enumerate() {
            let key = if u < v { (u, v) } else { (v, u) };
            let eid = edge_ids[edges[&key] as usize];
            let sense = if key == (u, v) { ".T." } else { ".F." };
            oes[k] = w.emit(&format!("ORIENTED_EDGE('',*,*,#{eid},{sense})"));
        }
        let loop_id = w.emit(&format!(
            "EDGE_LOOP('',(#{},#{},#{}))",
            oes[0], oes[1], oes[2]
        ));
        let bound = w.emit(&format!("FACE_OUTER_BOUND('',#{loop_id},.T.)"));
        face_ids.push(w.emit(&format!("ADVANCED_FACE('',(#{bound}),#{plane},.T.)")));
    }

    let mut shell_args = String::from("CLOSED_SHELL('',(");
    for (i, f) in face_ids.iter().enumerate() {
        if i > 0 {
            shell_args.push(',');
        }
        let _ = write!(shell_args, "#{f}");
    }
    shell_args.push_str("))");
    let shell = w.emit(&shell_args);
    let solid = w.emit(&format!("MANIFOLD_SOLID_BREP('solid',#{shell})"));
    let rep = w.emit(&format!(
        "ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{solid},#{ctx}),#{ctx})"
    ));
    w.emit(&format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));
    w.footer();
    w.out
}

/// STEP bytes from a real triangle mesh. Empty or degenerate tessellation is
/// an error — never a manufactured AABB box and never `Ok` of 0 bytes.
pub fn step_export_bytes(mesh: &MeshData) -> Result<Vec<u8>, String> {
    let tri_count = if mesh.indices.is_empty() {
        mesh.positions.len() / 9
    } else {
        mesh.indices.len() / 3
    };
    if tri_count == 0 || mesh.positions.len() < 9 {
        return Err("step: empty tessellation".into());
    }
    let s = to_step(mesh);
    if s.len() < 512
        || !s.contains("ISO-10303-21")
        || !s.contains("MANIFOLD_SOLID_BREP")
        || !s.contains("CLOSED_SHELL")
    {
        return Err("step: tessellation produced no solid".into());
    }
    Ok(s.into_bytes())
}

/// Axis-aligned bbox of `CARTESIAN_POINT` coordinates in an ISO-10303 file.
pub fn cartesian_bbox_from_step(bytes: &[u8]) -> Option<[f64; 6]> {
    let text = std::str::from_utf8(bytes).ok().unwrap_or("");
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut any = false;
    let mut rest = text;
    while let Some(i) = rest.find("CARTESIAN_POINT") {
        rest = &rest[i + 15..];
        // `#n = CARTESIAN_POINT('',(x,y,z));` — coords follow the name string.
        let Some(comma) = rest.find(",(") else {
            continue;
        };
        let after = &rest[comma + 2..];
        let Some(close) = after.find(')') else {
            continue;
        };
        let coords = &after[..close];
        let mut nums = coords
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok());
        let (Some(x), Some(y), Some(z)) = (nums.next(), nums.next(), nums.next()) else {
            rest = &after[close..];
            continue;
        };
        bb[0] = bb[0].min(x);
        bb[1] = bb[1].min(y);
        bb[2] = bb[2].min(z);
        bb[3] = bb[3].max(x);
        bb[4] = bb[4].max(y);
        bb[5] = bb[5].max(z);
        any = true;
        rest = &after[close..];
    }
    if any {
        Some(bb)
    } else {
        None
    }
}

struct StepBuf {
    next: usize,
    out: String,
}

impl StepBuf {
    fn new(nverts: usize, nedges: usize, nfaces: usize) -> Self {
        // ~2 entities/vert + 4/edge + 12/face + header
        let est = 2048 + nverts * 80 + nedges * 160 + nfaces * 400;
        Self {
            next: 1,
            out: String::with_capacity(est),
        }
    }

    fn header(&mut self) {
        self.out.push_str(
            "ISO-10303-21;\nHEADER;\n\
             FILE_DESCRIPTION(('AgentCAD faceted solid'),'2;1');\n\
             FILE_NAME('model.step','',('AgentCAD'),('AgentCAD'),'AgentCAD kernel','AgentCAD','');\n\
             FILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\n\
             ENDSEC;\nDATA;\n",
        );
    }

    fn footer(&mut self) {
        self.out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    }

    fn emit(&mut self, body: &str) -> usize {
        let id = self.next;
        self.next += 1;
        let _ = writeln!(self.out, "#{id} = {body};");
        id
    }
}

fn mesh_triangles(mesh: &MeshData) -> Vec<[[f64; 3]; 3]> {
    let at = |i: usize| -> Option<[f64; 3]> {
        let s = i * 3;
        if s + 2 >= mesh.positions.len() {
            return None;
        }
        Some([
            mesh.positions[s] as f64,
            mesh.positions[s + 1] as f64,
            mesh.positions[s + 2] as f64,
        ])
    };
    let mut tris = Vec::new();
    if mesh.indices.is_empty() {
        let n = mesh.positions.len() / 9;
        for t in 0..n {
            if let (Some(a), Some(b), Some(c)) = (at(t * 3), at(t * 3 + 1), at(t * 3 + 2)) {
                tris.push([a, b, c]);
            }
        }
    } else {
        for tri in mesh.indices.chunks(3) {
            if tri.len() < 3 {
                break;
            }
            if let (Some(a), Some(b), Some(c)) = (
                at(tri[0] as usize),
                at(tri[1] as usize),
                at(tri[2] as usize),
            ) {
                tris.push([a, b, c]);
            }
        }
    }
    tris
}

fn weld_vertex(
    verts: &mut Vec<[f64; 3]>,
    weld: &mut HashMap<(i64, i64, i64), u32>,
    p: [f64; 3],
) -> u32 {
    let q = 1e5;
    let key = (
        (p[0] * q).round() as i64,
        (p[1] * q).round() as i64,
        (p[2] * q).round() as i64,
    );
    if let Some(&i) = weld.get(&key) {
        return i;
    }
    let i = verts.len() as u32;
    verts.push(p);
    weld.insert(key, i);
    i
}

fn tri_normal(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ]
}

fn write_triangle(buf: &mut Vec<u8>, n: &[f32; 3], v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3]) {
    for &f in n {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    for &f in v0 {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    for &f in v1 {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    for &f in v2 {
        buf.extend_from_slice(&f.to_le_bytes());
    }
    buf.extend_from_slice(&[0u8; 2]); // attribute byte count
}

fn face_normal(v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3]) -> [f32; 3] {
    let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 1e-10 {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 0.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stl_has_correct_header_and_size() {
        // Single triangle
        let mesh = MeshData {
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![],
        };
        let stl = to_stl(&mesh);
        // Header 80 + count 4 + 1 triangle × 50
        assert_eq!(stl.len(), 80 + 4 + 50);
        // Count field
        let count = u32::from_le_bytes(stl[80..84].try_into().unwrap());
        assert_eq!(count, 1);
    }

    #[test]
    fn step_faceted_solid_has_iso_tokens_and_bbox() {
        let mesh = fixture_box_mesh([-5.7735, -5.0, 0.0, 5.7735, 5.0, 40.0]);
        let step = step_export_bytes(&mesh).expect("real mesh must export");
        let text = String::from_utf8_lossy(&step);
        assert!(
            step.len() > 512,
            "STEP must be a non-empty file, got {} bytes",
            step.len()
        );
        assert!(text.contains("ISO-10303-21"), "missing ISO-10303-21 header");
        assert!(
            text.contains("MANIFOLD_SOLID_BREP") && text.contains("CLOSED_SHELL"),
            "STEP must parse as a solid"
        );
        let bb = cartesian_bbox_from_step(&step).expect("CARTESIAN_POINT bbox");
        assert!((bb[0] + 5.7735).abs() < 1e-4 && (bb[3] - 5.7735).abs() < 1e-4);
        assert!((bb[1] + 5.0).abs() < 1e-4 && (bb[4] - 5.0).abs() < 1e-4);
        assert!(bb[2].abs() < 1e-4 && (bb[5] - 40.0).abs() < 1e-4);
    }

    #[test]
    fn empty_mesh_step_is_empty_not_panic() {
        let mesh = MeshData {
            positions: vec![],
            normals: vec![],
            indices: vec![],
        };
        assert!(to_step(&mesh).is_empty());
    }

    /// Inspector: an AABB box in the mesh bbox family would pass
    /// `bbox_same_family`. Empty tessellation must fail, not write that box.
    #[test]
    fn empty_tessellation_fails_step_export_not_bbox_placeholder() {
        let empty = MeshData {
            positions: vec![],
            normals: vec![],
            indices: vec![],
        };
        let err = step_export_bytes(&empty).expect_err("empty tessellation must fail");
        assert!(
            err.contains("empty") || err.contains("no solid"),
            "unexpected error: {err}"
        );
        assert!(to_step(&empty).is_empty());
        assert!(
            !to_step(&empty).contains("MANIFOLD_SOLID_BREP"),
            "empty tessellation must not write a STEP solid"
        );

        // What the removed fallback would have emitted — a box whose bbox
        // matches the golden M8 mesh family. That is a placeholder solid.
        let mesh_bbox = [-5.7735, -5.0, 0.0, 5.7735, 5.0, 40.0];
        let placeholder = fixture_box_mesh(mesh_bbox);
        let placeholder_step = to_step(&placeholder);
        assert!(placeholder_step.contains("MANIFOLD_SOLID_BREP"));
        let pbb = cartesian_bbox_from_step(placeholder_step.as_bytes()).unwrap();
        for i in 0..3 {
            let ea = (mesh_bbox[i + 3] - mesh_bbox[i]).abs();
            let eb = (pbb[i + 3] - pbb[i]).abs();
            assert!(
                (ea - eb).abs() < 0.05,
                "fixture box is in the mesh bbox family (the forbidden pass)"
            );
        }
        let empty_bytes = to_step(&empty).into_bytes();
        assert_ne!(
            empty_bytes,
            placeholder_step.into_bytes(),
            "empty tessellation must not produce the bbox placeholder STEP"
        );
        assert!(
            step_export_bytes(&empty).is_err(),
            "export of empty tessellation must not succeed"
        );
    }

    /// Test fixture only — not used as an export fallback.
    fn fixture_box_mesh(bbox: [f64; 6]) -> MeshData {
        let [x0, y0, z0, x1, y1, z1] = [
            bbox[0] as f32,
            bbox[1] as f32,
            bbox[2] as f32,
            bbox[3] as f32,
            bbox[4] as f32,
            bbox[5] as f32,
        ];
        let p = [
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ];
        let mut positions = Vec::with_capacity(8 * 3);
        for v in p {
            positions.extend_from_slice(&v);
        }
        MeshData {
            positions,
            normals: vec![],
            indices: vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2,
                7, 6, 3, 0, 4, 3, 4, 7,
            ],
        }
    }
}
