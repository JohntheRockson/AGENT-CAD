//! Mesh / STL helpers shared by look-right, STEP, and fillet checks.

use kernel::engine::MeshData;

use crate::golden::SHANK_R_MM;

pub fn bbox_from_mesh(mesh: &MeshData) -> [f64; 6] {
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut n = 0usize;
    for p in mesh_points(mesh) {
        n += 1;
        bb[0] = bb[0].min(p[0]);
        bb[1] = bb[1].min(p[1]);
        bb[2] = bb[2].min(p[2]);
        bb[3] = bb[3].max(p[0]);
        bb[4] = bb[4].max(p[1]);
        bb[5] = bb[5].max(p[2]);
    }
    if n == 0 {
        [0.0; 6]
    } else {
        bb
    }
}

pub fn mesh_points(mesh: &MeshData) -> Vec<[f64; 3]> {
    mesh.positions
        .chunks(3)
        .filter(|c| c.len() == 3)
        .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
        .collect()
}

pub fn mesh_from_points(pts: &[[f64; 3]]) -> MeshData {
    let mut positions = Vec::with_capacity(pts.len() * 3);
    for p in pts {
        positions.push(p[0] as f32);
        positions.push(p[1] as f32);
        positions.push(p[2] as f32);
    }
    MeshData {
        positions,
        normals: vec![],
        indices: vec![],
    }
}

pub fn bbox_close(a: [f64; 6], b: [f64; 6], tol: f64) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
}

pub fn fmt_bb(b: [f64; 6]) -> String {
    format!(
        "[{:.4}, {:.4}, {:.4}, {:.4}, {:.4}, {:.4}]",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

#[derive(Clone, Debug)]
pub struct HexHead {
    pub count: usize,
    pub z0: f64,
    pub z1: f64,
    pub dz: f64,
    pub max_r: f64,
    pub min_r: f64,
    /// Measured across-flats from hex vertices (mm). 0 if no head.
    pub across_flats: f64,
}

pub fn hex_head_metrics(mesh: &MeshData, shank_r: f64) -> HexHead {
    let cut = shank_r + 0.45;
    let mut z0 = f64::MAX;
    let mut z1 = f64::MIN;
    let mut max_r: f64 = 0.0;
    let mut min_r: f64 = f64::MAX;
    let mut count = 0usize;
    let mut hex_xy: Vec<[f64; 2]> = Vec::new();
    for p in mesh_points(mesh) {
        let r = p[0].hypot(p[1]);
        if r > cut {
            z0 = z0.min(p[2]);
            z1 = z1.max(p[2]);
            max_r = max_r.max(r);
            min_r = min_r.min(r);
            count += 1;
            hex_xy.push([p[0], p[1]]);
        }
    }
    if count == 0 {
        HexHead {
            count: 0,
            z0: 0.0,
            z1: 0.0,
            dz: 0.0,
            max_r: 0.0,
            min_r: 0.0,
            across_flats: 0.0,
        }
    } else {
        HexHead {
            count,
            z0,
            z1,
            dz: z1 - z0,
            max_r,
            min_r,
            across_flats: across_flats_from_xy(&hex_xy),
        }
    }
}

/// Across-flats = twice the smallest hex-flat support. Tries vertex-up and
/// flat-up frames (Kernel hex vertices sit at 0°/60°/…).
pub fn across_flats_from_xy(xy: &[[f64; 2]]) -> f64 {
    if xy.len() < 6 {
        return 0.0;
    }
    let a0 = hex_support_span(xy, 0.0);
    let a30 = hex_support_span(xy, std::f64::consts::PI / 6.0);
    a0.min(a30)
}

fn hex_support_span(xy: &[[f64; 2]], offset: f64) -> f64 {
    let mut half = [0.0_f64; 6];
    for k in 0..6 {
        let a = offset + k as f64 * std::f64::consts::PI / 3.0;
        let (c, s) = (a.cos(), a.sin());
        let mut m = 0.0_f64;
        for p in xy {
            m = m.max((p[0] * c + p[1] * s).abs());
        }
        half[k] = m;
    }
    2.0 * half.iter().copied().fold(f64::INFINITY, f64::min)
}

pub fn stl_bbox(stl: &[u8]) -> Option<[f64; 6]> {
    let mesh = stl_to_mesh(stl)?;
    let bb = bbox_from_mesh(&mesh);
    if bb[0].is_finite() {
        Some(bb)
    } else {
        None
    }
}

/// Parse binary STL vertices into a (non-indexed) mesh. Degenerate / ASCII files
/// return None — that is a look-right FAIL, not a silent skip.
pub fn stl_to_mesh(stl: &[u8]) -> Option<MeshData> {
    if stl.len() < 84 {
        return None;
    }
    let tri = u32::from_le_bytes(stl[80..84].try_into().ok()?) as usize;
    let need = 84 + tri * 50;
    if stl.len() < need || tri == 0 {
        return None;
    }
    let mut positions = Vec::with_capacity(tri * 9);
    for i in 0..tri {
        let off = 84 + i * 50;
        for v in 0..3 {
            let b = off + 12 + v * 12;
            positions.push(f32::from_le_bytes(stl[b..b + 4].try_into().ok()?));
            positions.push(f32::from_le_bytes(stl[b + 4..b + 8].try_into().ok()?));
            positions.push(f32::from_le_bytes(stl[b + 8..b + 12].try_into().ok()?));
        }
    }
    Some(MeshData {
        positions,
        normals: vec![],
        indices: vec![],
    })
}

pub fn empty_mesh() -> MeshData {
    MeshData {
        positions: vec![],
        normals: vec![],
        indices: vec![],
    }
}

/// Default shank radius used when the caller does not pass one.
pub fn default_shank_r() -> f64 {
    SHANK_R_MM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_af13_from_vertices() {
        let r = 13.0 / 3.0_f64.sqrt();
        let xy: Vec<[f64; 2]> = (0..6)
            .map(|k| {
                let a = k as f64 * std::f64::consts::PI / 3.0;
                [r * a.cos(), r * a.sin()]
            })
            .collect();
        let af = across_flats_from_xy(&xy);
        assert!((af - 13.0).abs() < 0.05, "af={af}");
    }
}
