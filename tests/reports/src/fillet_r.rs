//! Fillet look-right: measurable under-head junction R on the golden M8.
//!
//! Silent no-op is FAIL. Δvolume alone is FAIL. Hex-corner R (XY inset of
//! AF13 vertices) is FAIL unless the under-head torus at head≈5.3 / Ø8 is
//! also measurable. Named-edge `"all"` / junction indices still have to
//! show that junction R — they are not a hex-corner escape hatch.

use kernel::engine::{MeshData, MetricsData};
use kernel::topology::TopologyReport;

use crate::golden::{FILLET_RADIUS_MM, HEAD_HEIGHT_MM, SHANK_R_MM};
use crate::mesh_util::{hex_head_metrics, HexHead};

const FILLET_VOLUME_EPS_MM3: f64 = 0.25;
const FILLET_LEN_EPS_MM: f64 = 0.05;
const HEX_LOOK_EPS_MM: f64 = 0.12;
const R_FIT_EPS_MM: f64 = 0.35;

#[derive(Clone, Debug)]
pub struct FilletRFit {
    pub samples: usize,
    pub median_err: f64,
    pub expected_r: f64,
}

impl FilletRFit {
    pub fn ok(&self) -> bool {
        self.samples >= 12 && self.median_err <= R_FIT_EPS_MM
    }
}

pub fn under_head_edge_indices(topo: &TopologyReport, head_z: f64, shank_r: f64) -> Vec<usize> {
    let mut idxs = Vec::new();
    for e in &topo.edges {
        let r = e.mid[0].hypot(e.mid[1]);
        let on_junction_z = (e.mid[2] - head_z).abs() <= 0.85;
        let around_shank = r >= shank_r - 0.35 && r <= shank_r + 2.8;
        let named = e
            .tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("circle") || t.eq_ignore_ascii_case("underhead"));
        let circular = e.curve_type.to_ascii_lowercase().contains("circle");
        if on_junction_z && around_shank && (named || circular || e.length > 4.0) {
            idxs.push(e.index);
        }
    }
    idxs.sort_unstable();
    idxs.dedup();
    idxs
}

/// Fillet JSON inserted after the cylinder (hex+shank exists) so the under-head
/// junction is present. Named `"all"` is the fallback when topology has no
/// junction edges — still requires measurable R / hex look, not Δvol.
pub fn fillet_feature_json(radius: f64, edges: &FilletEdges) -> serde_json::Value {
    match edges {
        FilletEdges::Indices(ix) if !ix.is_empty() => serde_json::json!({
            "op": "fillet",
            "radius": radius,
            "edges": ix
        }),
        FilletEdges::Named(name) => serde_json::json!({
            "op": "fillet",
            "radius": radius,
            "edges": name
        }),
        _ => serde_json::json!({
            "op": "fillet",
            "radius": radius,
            "edges": "all"
        }),
    }
}

#[derive(Clone, Debug)]
pub enum FilletEdges {
    Indices(Vec<usize>),
    Named(String),
}

pub fn insert_fillet_after_cylinder(
    ir: &serde_json::Value,
    radius: f64,
    edges: &FilletEdges,
) -> serde_json::Value {
    let mut ir = ir.clone();
    let feat = fillet_feature_json(radius, edges);
    if let Some(bodies) = ir.get_mut("bodies").and_then(|b| b.as_array_mut()) {
        if let Some(features) = bodies
            .first_mut()
            .and_then(|b| b.get_mut("features"))
            .and_then(|f| f.as_array_mut())
        {
            let insert_at = features
                .iter()
                .position(|f| f["op"] == "cylinder")
                .map(|i| i + 1)
                .or_else(|| {
                    features
                        .iter()
                        .position(|f| f["op"] == "extrude")
                        .map(|i| i + 1)
                })
                .unwrap_or(features.len());
            features.insert(insert_at, feat);
            return ir;
        }
    }
    if let Some(features) = ir.get_mut("features").and_then(|f| f.as_array_mut()) {
        let insert_at = features
            .iter()
            .position(|f| f["op"] == "cylinder")
            .map(|i| i + 1)
            .or_else(|| {
                features
                    .iter()
                    .position(|f| f["op"] == "extrude")
                    .map(|i| i + 1)
            })
            .unwrap_or(features.len());
        features.insert(insert_at, feat);
    }
    ir
}

pub fn under_head_fillet_r_fit(
    mesh: &MeshData,
    head_z: f64,
    r_shank: f64,
    expected_r: f64,
) -> Option<FilletRFit> {
    let c_r = r_shank + expected_r;
    let c_z = head_z + expected_r;
    let mut errs = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let z = chunk[2] as f64;
        let r = x.hypot(y);
        if r < r_shank - 0.15 || r > r_shank + expected_r + 0.40 {
            continue;
        }
        if z < head_z - 0.15 || z > head_z + expected_r + 0.40 {
            continue;
        }
        // Off the two walls so we sample the arc, not the cylinder/bearing face.
        if r <= r_shank + 0.06 || z <= head_z + 0.06 {
            continue;
        }
        let d = (r - c_r).hypot(z - c_z);
        errs.push((d - expected_r).abs());
    }
    fit_from_errs(errs, expected_r)
}

/// Named-edge / hex-corner R: vertices inset from a sharp hex vertex along a
/// circle of radius `expected_r` in XY (top or vertical corners).
pub fn hex_corner_fillet_r_fit(mesh: &MeshData, expected_r: f64, af: f64) -> Option<FilletRFit> {
    if af < 8.0 {
        return None;
    }
    let r_vertex = af / 3.0_f64.sqrt();
    let mut errs = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = x.hypot(y);
        if r < r_vertex - expected_r - 0.4 || r > r_vertex + 0.15 {
            continue;
        }
        let yaw = y.atan2(x);
        // Kernel hex vertices at k·60°.
        let k = ((yaw / (std::f64::consts::PI / 3.0)).round()).rem_euclid(6.0);
        let a = k * std::f64::consts::PI / 3.0;
        let vx = r_vertex * a.cos();
        let vy = r_vertex * a.sin();
        // Fillet center sits inset from the vertex along the radial.
        let inset = (expected_r / (std::f64::consts::PI / 3.0).sin()).min(expected_r * 2.0);
        let cx = vx - inset * a.cos();
        let cy = vy - inset * a.sin();
        let d = (x - cx).hypot(y - cy);
        if (d - expected_r).abs() < 1.2 {
            errs.push((d - expected_r).abs());
        }
    }
    fit_from_errs(errs, expected_r)
}

fn fit_from_errs(mut errs: Vec<f64>, expected_r: f64) -> Option<FilletRFit> {
    if errs.len() < 12 {
        return None;
    }
    errs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = errs[errs.len() / 2];
    Some(FilletRFit {
        samples: errs.len(),
        median_err: median,
        expected_r,
    })
}

pub fn check_fillet(
    fillet: &Result<(MetricsData, MeshData), String>,
    baseline: Option<&MetricsData>,
    head_base: Option<&HexHead>,
    baseline_mesh: Option<&MeshData>,
) -> (bool, String, Option<MetricsData>, Option<HexHead>) {
    match fillet {
        Err(e) => (
            false,
            format!("fillet run FAILED (not a silent no-op): {e}"),
            None,
            None,
        ),
        Ok((metrics, mesh)) => {
            let Some(base) = baseline else {
                return (
                    false,
                    "cannot compare fillet: baseline execute failed".into(),
                    Some(metrics.clone()),
                    Some(hex_head_metrics(mesh, SHANK_R_MM)),
                );
            };
            let head = hex_head_metrics(mesh, SHANK_R_MM);
            let dvol = (metrics.volume - base.volume).abs();
            let dbbox = base
                .bbox
                .iter()
                .zip(metrics.bbox.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f64, f64::max);
            let d_max_r = head_base.map(|h| (h.max_r - head.max_r).abs()).unwrap_or(0.0);
            let d_min_r = head_base.map(|h| (h.min_r - head.min_r).abs()).unwrap_or(0.0);
            let d_af = head_base
                .map(|h| (h.across_flats - head.across_flats).abs())
                .unwrap_or(0.0);
            let d_dz = head_base.map(|h| (h.dz - head.dz).abs()).unwrap_or(0.0);

            let any_change = dvol > FILLET_VOLUME_EPS_MM3
                || dbbox > FILLET_LEN_EPS_MM
                || d_max_r > FILLET_LEN_EPS_MM
                || d_min_r > FILLET_LEN_EPS_MM
                || d_af > FILLET_LEN_EPS_MM
                || d_dz > FILLET_LEN_EPS_MM;

            if !any_change {
                return (
                    false,
                    format!(
                        "SILENT NO-OP fillet (FAIL): execute succeeded but hex-head metrics unchanged \
                         (Δvolume={dvol:.6} Δbbox={dbbox:.6} Δmax_r={d_max_r:.6} Δmin_r={d_min_r:.6} \
                         ΔAF={d_af:.6}; eps volume {FILLET_VOLUME_EPS_MM3} mm³ / length {FILLET_LEN_EPS_MM} mm)"
                    ),
                    Some(metrics.clone()),
                    Some(head),
                );
            }

            let hex_look = d_max_r > HEX_LOOK_EPS_MM
                || d_min_r > HEX_LOOK_EPS_MM
                || d_af > HEX_LOOK_EPS_MM
                || d_dz > HEX_LOOK_EPS_MM;

            let head_z = head_base
                .map(|h| h.z1)
                .filter(|z| z.abs() > 0.1)
                .unwrap_or(HEAD_HEIGHT_MM);
            let uh = under_head_fillet_r_fit(mesh, head_z, SHANK_R_MM, FILLET_RADIUS_MM);
            let corners = hex_corner_fillet_r_fit(
                mesh,
                FILLET_RADIUS_MM,
                head_base
                    .map(|h| h.across_flats)
                    .filter(|a| *a > 8.0)
                    .unwrap_or(13.0),
            );
            let uh_ok = uh.as_ref().map(|f| f.ok()).unwrap_or(false);
            let corner_ok = corners.as_ref().map(|f| f.ok()).unwrap_or(false);

            // Under-head junction R is required. Hex-corner XY R and Δvol are
            // diagnostics only — they must not PASS a sharp head/shank step.
            if uh_ok {
                let f = uh.as_ref().unwrap();
                let corner_note = if corner_ok {
                    "; hex-corner R also present (not sufficient by itself)"
                } else {
                    ""
                };
                return (
                    true,
                    format!(
                        "under-head junction R≈{} mm (median err {:.3} mm, n={}){corner_note}; \
                         ΔAF={d_af:.3} Δmin_r={d_min_r:.3} Δmax_r={d_max_r:.3} Δvolume={dvol:.3} \
                         (hex-corner / Δvol alone is not sufficient)",
                        f.expected_r, f.median_err, f.samples
                    ),
                    Some(metrics.clone()),
                    Some(head),
                );
            }

            let _ = baseline_mesh;
            (
                false,
                format!(
                    "no measurable under-head junction R (FAIL): Δvolume={dvol:.4} \
                     hex_look={hex_look} under_head={} hex_corner={} ΔAF={d_af:.4} Δmin_r={d_min_r:.4}. \
                     Fillet must show R≈{FILLET_RADIUS_MM} mm at the head≈{HEAD_HEIGHT_MM} / Ø8 \
                     junction. Hex-corner R or Δvolume alone is not enough.",
                    uh.as_ref()
                        .map(|f| format!("n={} err={:.3}", f.samples, f.median_err))
                        .unwrap_or_else(|| "none".into()),
                    corners
                        .as_ref()
                        .map(|f| format!("n={} err={:.3}", f.samples, f.median_err))
                        .unwrap_or_else(|| "none".into()),
                ),
                Some(metrics.clone()),
                Some(head),
            )
        }
    }
}

/// Hex + shank + quarter-torus under-head fillet of radius `r`.
pub fn synthetic_under_head_fillet_mesh(r: f64) -> MeshData {
    use crate::golden::{AF_MM, HEAD_HEIGHT_MM, LENGTH_MM};
    let mut pts = Vec::new();
    let r_hex = AF_MM / 3.0_f64.sqrt();
    // Unfilleted outer hex (look change comes from inset corners + torus).
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::PI / 3.0;
        let inset = r_hex - 0.25 * r;
        pts.push([inset * a.cos(), inset * a.sin(), 0.0]);
        pts.push([inset * a.cos(), inset * a.sin(), HEAD_HEIGHT_MM]);
        // Corner fillet samples in XY.
        for t in 0..10 {
            let ang = a - 0.25 + 0.5 * (t as f64) / 9.0;
            let cr = r_hex - r + r * (1.0 - (t as f64 / 9.0 - 0.5).abs());
            pts.push([cr * ang.cos(), cr * ang.sin(), HEAD_HEIGHT_MM]);
        }
    }
    let n_yaw = 36usize;
    let n_z = 24usize;
    for iz in 0..=n_z {
        let z = HEAD_HEIGHT_MM + r + (LENGTH_MM - HEAD_HEIGHT_MM - r) * (iz as f64) / (n_z as f64);
        for iy in 0..n_yaw {
            let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
            pts.push([SHANK_R_MM * yaw.cos(), SHANK_R_MM * yaw.sin(), z]);
        }
    }
    // Under-head quarter torus: center (r_shank+R, head_z+R).
    let c_r = SHANK_R_MM + r;
    let c_z = HEAD_HEIGHT_MM + r;
    for iy in 0..n_yaw {
        let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
        for it in 1..10 {
            let th = (it as f64) / 10.0 * std::f64::consts::FRAC_PI_2;
            let rr = c_r - r * th.cos();
            let z = c_z - r * th.sin();
            pts.push([rr * yaw.cos(), rr * yaw.sin(), z]);
        }
    }
    crate::mesh_util::mesh_from_points(&pts)
}

/// Sharp AF13 hex + Ø8 shank — hex corners filleted in XY, no under-head torus.
/// Hex-corner R + Δvol / hex look must not PASS fillet acceptance.
pub fn synthetic_hex_corner_only_mesh(r: f64) -> MeshData {
    use crate::golden::{AF_MM, HEAD_HEIGHT_MM, LENGTH_MM};
    let mut pts = Vec::new();
    let r_hex = AF_MM / 3.0_f64.sqrt();
    let inset = (r / (std::f64::consts::PI / 3.0).sin()).min(r * 2.0);
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::PI / 3.0;
        let vx = r_hex * a.cos();
        let vy = r_hex * a.sin();
        let cx = vx - inset * a.cos();
        let cy = vy - inset * a.sin();
        // Flat remainder + circular corner matching `hex_corner_fillet_r_fit`.
        let flat = r_hex * (std::f64::consts::PI / 6.0).cos();
        pts.push([flat * (a + std::f64::consts::PI / 6.0).cos(), flat * (a + std::f64::consts::PI / 6.0).sin(), 0.0]);
        pts.push([flat * (a + std::f64::consts::PI / 6.0).cos(), flat * (a + std::f64::consts::PI / 6.0).sin(), HEAD_HEIGHT_MM]);
        for t in 0..12 {
            let ang = a - std::f64::consts::PI / 6.0
                + std::f64::consts::PI / 3.0 * (t as f64) / 11.0;
            pts.push([cx + r * ang.cos(), cy + r * ang.sin(), 0.0]);
            pts.push([cx + r * ang.cos(), cy + r * ang.sin(), HEAD_HEIGHT_MM]);
        }
    }
    let n_yaw = 36usize;
    let n_z = 24usize;
    for iz in 0..=n_z {
        let z = HEAD_HEIGHT_MM + (LENGTH_MM - HEAD_HEIGHT_MM) * (iz as f64) / (n_z as f64);
        for iy in 0..n_yaw {
            let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
            pts.push([SHANK_R_MM * yaw.cos(), SHANK_R_MM * yaw.sin(), z]);
        }
    }
    crate::mesh_util::mesh_from_points(&pts)
}

/// Sharp AF13 hex (no corner inset) + quarter-torus under-head R + Ø8 shank.
/// Must PASS even when hex AF / min_r do not change.
pub fn synthetic_under_head_only_mesh(r: f64) -> MeshData {
    use crate::golden::{AF_MM, HEAD_HEIGHT_MM, LENGTH_MM};
    let mut pts = Vec::new();
    let r_hex = AF_MM / 3.0_f64.sqrt();
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::PI / 3.0;
        pts.push([r_hex * a.cos(), r_hex * a.sin(), 0.0]);
        pts.push([r_hex * a.cos(), r_hex * a.sin(), HEAD_HEIGHT_MM]);
    }
    let n_yaw = 36usize;
    let n_z = 24usize;
    for iz in 0..=n_z {
        let z = HEAD_HEIGHT_MM + r + (LENGTH_MM - HEAD_HEIGHT_MM - r) * (iz as f64) / (n_z as f64);
        for iy in 0..n_yaw {
            let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
            pts.push([SHANK_R_MM * yaw.cos(), SHANK_R_MM * yaw.sin(), z]);
        }
    }
    let c_r = SHANK_R_MM + r;
    let c_z = HEAD_HEIGHT_MM + r;
    for iy in 0..n_yaw {
        let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
        for it in 1..10 {
            let th = (it as f64) / 10.0 * std::f64::consts::FRAC_PI_2;
            let rr = c_r - r * th.cos();
            let z = c_z - r * th.sin();
            pts.push([rr * yaw.cos(), rr * yaw.sin(), z]);
        }
    }
    crate::mesh_util::mesh_from_points(&pts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look_right::synthetic_smooth_rod_hex;
    use crate::mesh_util::bbox_from_mesh;

    #[test]
    fn under_head_r_is_measurable_on_torus() {
        let mesh = synthetic_under_head_fillet_mesh(FILLET_RADIUS_MM);
        let fit = under_head_fillet_r_fit(&mesh, HEAD_HEIGHT_MM, SHANK_R_MM, FILLET_RADIUS_MM)
            .expect("fit");
        assert!(fit.ok(), "n={} err={}", fit.samples, fit.median_err);
    }

    #[test]
    fn delta_volume_only_fails() {
        let base_mesh = synthetic_smooth_rod_hex();
        let base = MetricsData {
            volume: 2500.0,
            bbox: bbox_from_mesh(&base_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let head = hex_head_metrics(&base_mesh, SHANK_R_MM);
        let mut filleted = base.clone();
        filleted.volume = 2490.0; // Δvol only
        let (ok, detail, _, _) = check_fillet(
            &Ok((filleted, base_mesh.clone())),
            Some(&base),
            Some(&head),
            Some(&base_mesh),
        );
        assert!(!ok, "Δvol-only must FAIL: {detail}");
        assert!(
            detail.contains("Δvol") || detail.contains("measurable"),
            "{detail}"
        );
    }

    #[test]
    fn silent_noop_fails() {
        let base_mesh = synthetic_smooth_rod_hex();
        let base = MetricsData {
            volume: 2500.0,
            bbox: bbox_from_mesh(&base_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let head = hex_head_metrics(&base_mesh, SHANK_R_MM);
        let (ok, detail, _, _) = check_fillet(
            &Ok((base.clone(), base_mesh.clone())),
            Some(&base),
            Some(&head),
            Some(&base_mesh),
        );
        assert!(!ok, "{detail}");
        assert!(detail.contains("SILENT NO-OP"));
    }

    #[test]
    fn under_head_r_and_hex_look_pass() {
        let base_mesh = synthetic_smooth_rod_hex();
        let base = MetricsData {
            volume: 2500.0,
            bbox: bbox_from_mesh(&base_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let head = hex_head_metrics(&base_mesh, SHANK_R_MM);
        let filleted_mesh = synthetic_under_head_fillet_mesh(FILLET_RADIUS_MM);
        let filleted = MetricsData {
            volume: 2488.0,
            bbox: bbox_from_mesh(&filleted_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let (ok, detail, _, _) = check_fillet(
            &Ok((filleted, filleted_mesh)),
            Some(&base),
            Some(&head),
            Some(&base_mesh),
        );
        assert!(ok, "{detail}");
        assert!(detail.contains("under-head"), "{detail}");
    }

    #[test]
    fn hex_corner_r_and_dvol_fail_without_under_head() {
        let base_mesh = synthetic_smooth_rod_hex();
        let base = MetricsData {
            volume: 2500.0,
            bbox: bbox_from_mesh(&base_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let head = hex_head_metrics(&base_mesh, SHANK_R_MM);
        let corners = synthetic_hex_corner_only_mesh(FILLET_RADIUS_MM);
        let fit = hex_corner_fillet_r_fit(&corners, FILLET_RADIUS_MM, 13.0);
        assert!(
            fit.as_ref().map(|f| f.ok()).unwrap_or(false),
            "precondition: hex-corner R must be measurable, got {fit:?}"
        );
        let uh = under_head_fillet_r_fit(&corners, HEAD_HEIGHT_MM, SHANK_R_MM, FILLET_RADIUS_MM);
        assert!(
            uh.as_ref().map(|f| !f.ok()).unwrap_or(true),
            "precondition: corner-only mesh must not fake under-head R, got {uh:?}"
        );
        let filleted = MetricsData {
            volume: 2488.0,
            bbox: bbox_from_mesh(&corners),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let (ok, detail, _, _) = check_fillet(
            &Ok((filleted, corners)),
            Some(&base),
            Some(&head),
            Some(&base_mesh),
        );
        assert!(!ok, "hex-corner + Δvol must FAIL without under-head R: {detail}");
        assert!(
            detail.contains("under-head") && detail.contains("FAIL"),
            "{detail}"
        );
    }

    #[test]
    fn under_head_r_passes_without_hex_corner_look() {
        let base_mesh = synthetic_smooth_rod_hex();
        let base = MetricsData {
            volume: 2500.0,
            bbox: bbox_from_mesh(&base_mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let head = hex_head_metrics(&base_mesh, SHANK_R_MM);
        let mesh = synthetic_under_head_only_mesh(FILLET_RADIUS_MM);
        let filleted = MetricsData {
            volume: 2501.0, // concave blend may add volume
            bbox: bbox_from_mesh(&mesh),
            surface_area: 1.0,
            is_solid: true,
            mesh_provenance: Default::default(),
        };
        let (ok, detail, _, _) = check_fillet(
            &Ok((filleted, mesh)),
            Some(&base),
            Some(&head),
            Some(&base_mesh),
        );
        assert!(ok, "{detail}");
        assert!(detail.contains("under-head junction"), "{detail}");
    }
}
