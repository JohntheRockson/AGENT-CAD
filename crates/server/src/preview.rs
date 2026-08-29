//! Tiny isometric preview of tessellated bodies for the agent verify loop.

use std::collections::HashMap;
use std::io::Write;

use kernel::{BodyOutput, DocumentOutput, MeshData};

const SIZE: usize = 320;
const PALETTE: [[u8; 3]; 8] = [
    [74, 144, 217],
    [80, 200, 120],
    [200, 80, 110],
    [160, 100, 210],
    [220, 90, 70],
    [60, 190, 200],
    [230, 160, 60],
    [180, 180, 90],
];

pub struct QualityNote {
    pub body_id: String,
    pub name: String,
    pub components: usize,
    pub fragmented: bool,
}

pub fn quality_notes(output: &DocumentOutput) -> Vec<QualityNote> {
    output
        .bodies
        .iter()
        .filter(|b| b.visible && !b.suppressed)
        .map(|b| {
            let components = mesh_component_count(&b.mesh);
            QualityNote {
                fragmented: is_fragmented(&b.name, components),
                body_id: b.body_id.clone(),
                name: b.name.clone(),
                components,
            }
        })
        .collect()
}

pub fn quality_report(notes: &[QualityNote]) -> String {
    if notes.is_empty() {
        return "No visible bodies.".into();
    }
    notes
        .iter()
        .map(|n| {
            format!(
                "- {} ({}) mesh shells={}: {}",
                n.name,
                n.body_id,
                n.components,
                if n.fragmented {
                    "FRAGMENTED — looks like disconnected bars/primitives, not one machined part"
                } else {
                    "ok"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn any_fragmented(notes: &[QualityNote]) -> bool {
    notes.iter().any(|n| n.fragmented)
}

pub fn assembly_failures(_document: &kernel::CadDocument, output: &DocumentOutput) -> Vec<String> {
    let mut reasons = Vec::new();

    let vis: Vec<&BodyOutput> = output
        .bodies
        .iter()
        .filter(|b| b.visible && !b.suppressed)
        .collect();

    let gap_ok = |a: &BodyOutput, b: &BodyOutput| {
        bbox_gap(&a.metrics.bbox, &b.metrics.bbox) <= mate_tolerance(&a.metrics.bbox, &b.metrics.bbox)
    };

    let named = |pred: fn(&str) -> bool| -> Vec<&BodyOutput> {
        vis.iter().copied().filter(|b| pred(&b.name)).collect()
    };

    let require_close = |label_a: &str, label_b: &str, a: &[&BodyOutput], b: &[&BodyOutput]| {
        if a.is_empty() || b.is_empty() {
            return None;
        }
        let hits = a.iter().any(|x| b.iter().any(|y| gap_ok(x, y)));
        if hits {
            None
        } else {
            Some(format!(
                "{label_a} does not meet {label_b} — bounding boxes are separated. \
                 This is an exploded layout, not an assembly. Overlap the joint (ball in knuckle taper, \
                 strut on the LCA pad, bushings in the arm eyes)."
            ))
        }
    };

    if let Some(msg) = require_close(
        "Control arm",
        "knuckle/upright",
        &named(is_arm),
        &named(is_knuckle),
    ) {
        reasons.push(msg);
    }
    if let Some(msg) = require_close(
        "Strut/coilover",
        "an arm or knuckle",
        &named(is_strut),
        &named(|n| is_arm(n) || is_knuckle(n)),
    ) {
        reasons.push(msg);
    }
    if let Some(msg) = require_close(
        "Ball joint",
        "an arm or knuckle",
        &named(is_ball),
        &named(|n| is_arm(n) || is_knuckle(n)),
    ) {
        reasons.push(msg);
    }

    if vis.len() >= 3 {
        let any_touch = vis.iter().enumerate().any(|(i, a)| {
            vis.iter().skip(i + 1).any(|b| gap_ok(a, b))
        });
        if !any_touch {
            reasons.push(
                "No two bodies even come close in space. Plant every part on its mate; do not leave \
                 a strut or knuckle floating beside the arms."
                    .into(),
            );
        }
    }

    reasons
}

pub fn reject_reason(document: &kernel::CadDocument, output: &DocumentOutput) -> Option<String> {
    let quality = quality_notes(output);
    let mut reasons = Vec::new();
    if any_fragmented(&quality) {
        reasons.push(quality_report(&quality));
    }
    reasons.extend(assembly_failures(document, output));
    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join("\n"))
    }
}

fn is_fastener(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("bushing")
        || n.contains("bolt")
        || n.contains("hardware")
        || n.contains("fastener")
        || n.contains("washer")
        || n.contains("nut")
}

fn is_arm(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("control arm") || n.contains("wishbone") || n.contains(" lca") || n.contains(" uca")
        || n.starts_with("lca")
        || n.starts_with("uca")
        || n.contains("lower control")
        || n.contains("upper control")
}

fn is_knuckle(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("knuckle") || n.contains("upright")
}

fn is_strut(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("strut") || n.contains("coilover") || n.contains("shock") || n.contains("damper")
}

fn is_ball(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("ball")
}

fn bbox_gap(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    let gx = (a[0] - b[3]).max(b[0] - a[3]).max(0.0);
    let gy = (a[1] - b[4]).max(b[1] - a[4]).max(0.0);
    let gz = (a[2] - b[5]).max(b[2] - a[5]).max(0.0);
    (gx * gx + gy * gy + gz * gz).sqrt()
}

fn bbox_diag(b: &[f64; 6]) -> f64 {
    let dx = (b[3] - b[0]).abs();
    let dy = (b[4] - b[1]).abs();
    let dz = (b[5] - b[2]).abs();
    (dx * dx + dy * dy + dz * dz).sqrt().max(1.0)
}

fn mate_tolerance(a: &[f64; 6], b: &[f64; 6]) -> f64 {
    (0.06 * bbox_diag(a).max(bbox_diag(b))).clamp(8.0, 60.0)
}

pub fn render_png(output: &DocumentOutput) -> Option<Vec<u8>> {
    let bodies: Vec<&BodyOutput> = output
        .bodies
        .iter()
        .filter(|b| b.visible && !b.suppressed && !b.mesh.positions.is_empty())
        .collect();
    if bodies.is_empty() {
        return None;
    }

    let mut xmin = f32::MAX;
    let mut ymin = f32::MAX;
    let mut zmin = f32::MAX;
    let mut xmax = f32::MIN;
    let mut ymax = f32::MIN;
    let mut zmax = f32::MIN;
    for b in &bodies {
        for c in b.mesh.positions.chunks(3) {
            if c.len() < 3 {
                continue;
            }
            xmin = xmin.min(c[0]);
            ymin = ymin.min(c[1]);
            zmin = zmin.min(c[2]);
            xmax = xmax.max(c[0]);
            ymax = ymax.max(c[1]);
            zmax = zmax.max(c[2]);
        }
    }
    let cx = (xmin + xmax) * 0.5;
    let cy = (ymin + ymax) * 0.5;
    let cz = (zmin + zmax) * 0.5;
    let span = (xmax - xmin).max(ymax - ymin).max(zmax - zmin).max(1.0);

    let mut color = vec![18u8, 18, 22].repeat(SIZE * SIZE);
    let mut zbuf = vec![f32::NEG_INFINITY; SIZE * SIZE];

    for (bi, body) in bodies.iter().enumerate() {
        let rgb = PALETTE[bi % PALETTE.len()];
        for_each_triangle(&body.mesh, |p0, p1, p2| {
            fill_triangle(
                &mut color,
                &mut zbuf,
                project(p0, cx, cy, cz, span),
                project(p1, cx, cy, cz, span),
                project(p2, cx, cy, cz, span),
                rgb,
            );
        });
    }

    encode_png(&color, SIZE, SIZE)
}

fn is_fragmented(name: &str, components: usize) -> bool {
    if components <= 1 {
        return false;
    }
    if is_fastener(name) {
        return false;
    }
    let n = name.to_ascii_lowercase();
    if n.contains("ball") {
        return components > 4;
    }
    if n.contains("strut") || n.contains("coilover") || n.contains("shock") || n.contains("damper")
    {
        return components > 6;
    }
    // One machined part is 1 welded shell. A bag of unfused primitives is many.
    components > 3
}

/// Count disconnected *solids* in a tessellation.
///
/// OCCT emits unique vertices per face (sharp edges do not share indices), so a
/// union-find on triangle indices alone treats every face as its own shell.
/// Weld coincident vertices first; ignore dust; if one island owns most of the
/// mesh, count it as a single part.
fn mesh_component_count(mesh: &MeshData) -> usize {
    let n = mesh.positions.len() / 3;
    if n == 0 {
        return 0;
    }
    let mut parent: Vec<usize> = (0..n).collect();
    let find = |parent: &mut [usize], mut i: usize| {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    };
    let unite = |parent: &mut [usize], a: usize, b: usize| {
        let pa = find(parent, a);
        let pb = find(parent, b);
        if pa != pb {
            parent[pa] = pb;
        }
    };

    let mut grid: HashMap<(i32, i32, i32), usize> = HashMap::new();
    for i in 0..n {
        let x = (mesh.positions[i * 3] * 20.0).round() as i32;
        let y = (mesh.positions[i * 3 + 1] * 20.0).round() as i32;
        let z = (mesh.positions[i * 3 + 2] * 20.0).round() as i32;
        if let Some(&j) = grid.get(&(x, y, z)) {
            unite(&mut parent, i, j);
        } else {
            grid.insert((x, y, z), i);
        }
    }

    if mesh.indices.is_empty() {
        for tri in (0..n / 3).map(|t| t * 3) {
            unite(&mut parent, tri, tri + 1);
            unite(&mut parent, tri + 1, tri + 2);
        }
    } else {
        for tri in mesh.indices.chunks(3) {
            if tri.len() < 3 {
                continue;
            }
            unite(&mut parent, tri[0] as usize, tri[1] as usize);
            unite(&mut parent, tri[1] as usize, tri[2] as usize);
        }
    }

    let mut size = vec![0usize; n];
    for i in 0..n {
        size[find(&mut parent, i)] += 1;
    }
    let dust = (n / 100).max(3).min(32);
    let significant: Vec<usize> = size.into_iter().filter(|&s| s >= dust).collect();
    if significant.is_empty() {
        return 1;
    }
    let total: usize = significant.iter().sum();
    let largest = *significant.iter().max().unwrap_or(&0);
    // Main solid + a couple of crumbs still reads as one part.
    if largest * 5 >= total * 4 {
        return 1;
    }
    significant.len().max(1)
}

fn for_each_triangle(mesh: &MeshData, mut f: impl FnMut([f32; 3], [f32; 3], [f32; 3])) {
    let p = &mesh.positions;
    let get = |i: usize| [p[i * 3], p[i * 3 + 1], p[i * 3 + 2]];
    if mesh.indices.is_empty() {
        let n = p.len() / 9;
        for t in 0..n {
            f(get(t * 3), get(t * 3 + 1), get(t * 3 + 2));
        }
    } else {
        for tri in mesh.indices.chunks(3) {
            if tri.len() < 3 {
                continue;
            }
            f(get(tri[0] as usize), get(tri[1] as usize), get(tri[2] as usize));
        }
    }
}

fn project(p: [f32; 3], cx: f32, cy: f32, cz: f32, span: f32) -> [f32; 3] {
    let x = (p[0] - cx) / span;
    let y = (p[1] - cy) / span;
    let z = (p[2] - cz) / span;
    let sx = (x - y) * 0.866;
    let sy = (x + y) * 0.5 - z;
    let depth = x + y + z;
    let u = (0.5 + sx * 0.42) * SIZE as f32;
    let v = (0.52 - sy * 0.42) * SIZE as f32;
    [u, v, depth]
}

fn fill_triangle(
    color: &mut [u8],
    zbuf: &mut [f32],
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
    rgb: [u8; 3],
) {
    let minx = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i32;
    let maxx = a[0].max(b[0]).max(c[0]).ceil().min((SIZE - 1) as f32) as i32;
    let miny = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i32;
    let maxy = a[1].max(b[1]).max(c[1]).ceil().min((SIZE - 1) as f32) as i32;
    let area = edge(a, b, c);
    if area.abs() < 1e-6 {
        return;
    }
    for y in miny..=maxy {
        for x in minx..=maxx {
            let p = [x as f32 + 0.5, y as f32 + 0.5, 0.0];
            let w0 = edge(b, c, p) / area;
            let w1 = edge(c, a, p) / area;
            let w2 = edge(a, b, p) / area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
            let idx = y as usize * SIZE + x as usize;
            if z >= zbuf[idx] {
                zbuf[idx] = z;
                let o = idx * 3;
                color[o] = rgb[0];
                color[o + 1] = rgb[1];
                color[o + 2] = rgb[2];
            }
        }
    }
}

fn edge(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

fn encode_png(rgb: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for row in 0..h {
        raw.push(0);
        let s = row * w * 3;
        raw.extend_from_slice(&rgb[s..s + w * 3]);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
    write_chunk(&mut out, *b"IHDR", {
        let mut d = Vec::new();
        d.extend_from_slice(&(w as u32).to_be_bytes());
        d.extend_from_slice(&(h as u32).to_be_bytes());
        d.extend_from_slice(&[8, 2, 0, 0, 0]);
        d
    });
    write_chunk(&mut out, *b"IDAT", zlib_store(&raw)?);
    write_chunk(&mut out, *b"IEND", vec![]);
    Some(out)
}

fn write_chunk(out: &mut Vec<u8>, ty: [u8; 4], data: Vec<u8>) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_buf = Vec::with_capacity(4 + data.len());
    crc_buf.extend_from_slice(&ty);
    crc_buf.extend_from_slice(&data);
    out.extend_from_slice(&crc_buf);
    out.extend_from_slice(&crc32(&crc_buf).to_be_bytes());
}

pub fn to_base64(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let c = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (a << 16) | (b << 8) | c;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn zlib_store(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0x78, 0x01];
    let mut i = 0;
    while i < data.len() {
        let n = (data.len() - i).min(65535);
        let last = i + n == data.len();
        out.push(if last { 0x01 } else { 0x00 });
        let len = n as u16;
        out.write_all(&len.to_le_bytes()).ok()?;
        out.write_all(&(!len).to_le_bytes()).ok()?;
        out.extend_from_slice(&data[i..i + n]);
        i += n;
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    Some(out)
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &byte in data {
        c ^= byte as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    c ^ 0xffff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(positions: Vec<f32>, indices: Vec<u32>) -> MeshData {
        MeshData {
            positions,
            normals: vec![],
            indices,
        }
    }

    /// Six quads of a box, unique verts per face (how OCCT tessellates sharp edges).
    fn box_faces() -> MeshData {
        let faces = [
            // z=0 and z=10
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0, 0.0],
            [0.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0, 10.0, 10.0],
            // y=0 and y=10
            [0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 0.0, 10.0, 0.0, 0.0, 10.0],
            [0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 10.0, 0.0, 10.0, 10.0],
            // x=0 and x=10
            [0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 0.0, 10.0],
            [10.0, 0.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0, 10.0],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for face in faces {
            let base = (positions.len() / 3) as u32;
            positions.extend_from_slice(&face);
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh(positions, indices)
    }

    #[test]
    fn welded_box_faces_count_as_one_part() {
        assert_eq!(mesh_component_count(&box_faces()), 1);
        assert!(!is_fragmented("Lower Control Arm", 1));
    }

    #[test]
    fn index_only_count_would_have_called_a_box_fragmented() {
        // Same mesh without welding would be 6 face islands — that was the old bug.
        assert!(!is_fragmented("Lower Control Arm", mesh_component_count(&box_faces())));
    }

    #[test]
    fn two_distant_triangles_are_two_parts() {
        let m = mesh(
            vec![
                0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 200.0, 0.0, 0.0, 210.0, 0.0, 0.0,
                200.0, 10.0, 0.0,
            ],
            vec![0, 1, 2, 3, 4, 5],
        );
        assert_eq!(mesh_component_count(&m), 2);
    }

    #[test]
    fn twenty_equal_islands_is_fragmented() {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for i in 0..20 {
            let o = (i as f32) * 50.0;
            let base = (positions.len() / 3) as u32;
            positions.extend_from_slice(&[
                o, 0.0, 0.0, o + 5.0, 0.0, 0.0, o, 5.0, 0.0, o + 5.0, 5.0, 0.0,
            ]);
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
        let count = mesh_component_count(&mesh(positions, indices));
        assert!(count > 3, "got {count}");
        assert!(is_fragmented("Steering Knuckle", count));
    }
}
