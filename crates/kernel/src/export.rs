//! Export utilities for geometry data.
//!
//! Binary STL is implemented in pure Rust and works with both the mock and OCCT
//! backends. STEP/glTF export is delegated to the OCCT backend.

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
                [mesh.positions[s], mesh.positions[s + 1], mesh.positions[s + 2]]
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

fn write_triangle(buf: &mut Vec<u8>, n: &[f32; 3], v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3]) {
    for &f in n { buf.extend_from_slice(&f.to_le_bytes()); }
    for &f in v0 { buf.extend_from_slice(&f.to_le_bytes()); }
    for &f in v1 { buf.extend_from_slice(&f.to_le_bytes()); }
    for &f in v2 { buf.extend_from_slice(&f.to_le_bytes()); }
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
            positions: vec![
                0.0, 0.0, 0.0,
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
            ],
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
}
