//! Viewport / STL look-right: helix, ISO-V, no vertical sliver, plus
//! instance-window continuity and a clean dead→thread entry.
//!
//! Ported from `crates/kernel/tests/occt_geometry.rs` (angular_radius_spread,
//! distinct_groove_yaws, assert_no_vertical_uncut_strip, assert_iso_v_thread_profile,
//! assert_helix_continuous_across_instance_windows, assert_clean_thread_entry).
//! AABB-only agreement with a smooth rod of the same bbox must FAIL.
//! A mid-shank helix bar must not PASS when instanced slabs jump.

use kernel::engine::MeshData;

use crate::golden::{
    HELIX_WINDOW_Z0_MM, HELIX_WINDOW_Z1_MM, PITCH_MM, SHANK_R_MM, SHANK_Z0_MM, SHANK_Z1_MM,
    THREAD_Z0_MM,
};
use crate::mesh_util::{bbox_close, bbox_from_mesh, fmt_bb, stl_to_mesh};

const STL_BBOX_TOL_MM: f64 = 0.05;

/// Same thresholds as kernel `assert_helix_continuous_across_instance_windows` (#22).
const HELIX_PHASE_WORST_TURN: f64 = 0.10;
const HELIX_PHASE_RMS_TURN: f64 = 0.08;

#[derive(Clone, Debug)]
pub struct LookRight {
    pub ok: bool,
    pub detail: String,
    pub variation: f64,
    pub spread: f64,
    pub n_yaws: usize,
    pub sliver_ok: bool,
    pub iso_v_ok: bool,
    /// Deep-root helix phase does not step at instance-window seams.
    pub continuous_ok: bool,
    /// First turn is a groove on the helix, not leftover cylinder / notch.
    pub entry_ok: bool,
}

impl LookRight {
    fn empty_fail(detail: impl Into<String>) -> Self {
        Self {
            ok: false,
            detail: detail.into(),
            variation: 0.0,
            spread: 0.0,
            n_yaws: 0,
            sliver_ok: false,
            iso_v_ok: false,
            continuous_ok: false,
            entry_ok: false,
        }
    }
}

/// Helix + ISO-V + no leftover generator strip on a shank band.
pub fn check_look_right(
    mesh: &MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) -> LookRight {
    if mesh.positions.is_empty() {
        return LookRight::empty_fail("empty mesh — placeholder / crash, not a threaded shank");
    }

    let mid = 0.5 * (z0 + z1);
    let variation = radius_variation_at_z(mesh, mid, 0.25);
    let spread = angular_radius_spread_at_z(mesh, mid, 0.08);
    let n_yaws = distinct_groove_yaws(mesh, z0, z1, 16);
    let sliver = no_vertical_uncut_strip(mesh, r_major, pitch, z0, z1);
    let iso = iso_v_thread_profile(mesh, r_major, pitch, z0, z1);

    let mut fail: Vec<String> = Vec::new();
    if variation <= 0.08 {
        fail.push(format!(
            "stacked ticks / axisymmetric rings: radius variation={variation:.4} (need > 0.08)"
        ));
    }
    if spread <= 0.25 {
        fail.push(format!(
            "not helical at a thin z-slice: angular spread={spread:.4} (need > 0.25)"
        ));
    }
    if n_yaws < 5 {
        fail.push(format!(
            "groove does not walk around the shank: distinct yaws={n_yaws} (rings stay in 1-2 bins)"
        ));
    }
    if let Err(e) = &sliver {
        fail.push(e.clone());
    }
    if let Err(e) = &iso {
        fail.push(e.clone());
    }

    let ok = fail.is_empty();
    let detail = if ok {
        format!(
            "helix + ISO-V + no sliver: variation={variation:.3} spread={spread:.3} yaws={n_yaws}"
        )
    } else {
        format!("look-right FAIL: {}", fail.join(" | "))
    };
    LookRight {
        ok,
        detail,
        variation,
        spread,
        n_yaws,
        sliver_ok: sliver.is_ok(),
        iso_v_ok: iso.is_ok(),
        // Mid-band helix/ISO/sliver only. Continuity / entry are viewport extras
        // so STEP's `viewport_is_threaded` does not go false on a seamed helix
        // (that would loosen uncut-host detection).
        continuous_ok: false,
        entry_ok: false,
    }
}

/// Viewport / STL path: mid-shank helix bar **plus** instance-window continuity
/// and a clean dead→thread entry (same thresholds as kernel #22).
pub fn check_viewport_look_right(mesh: &MeshData) -> LookRight {
    let mut lr = check_look_right(mesh, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM);
    let cont = helix_continuous_across_instance_windows(
        mesh,
        SHANK_R_MM,
        PITCH_MM,
        HELIX_WINDOW_Z0_MM,
        HELIX_WINDOW_Z1_MM,
    );
    let entry = clean_thread_entry(mesh, SHANK_R_MM, PITCH_MM, THREAD_Z0_MM);
    lr.continuous_ok = cont.is_ok();
    lr.entry_ok = entry.is_ok();
    let mut fail: Vec<String> = Vec::new();
    if !lr.ok {
        fail.push(lr.detail.clone());
    }
    if let Err(e) = &cont {
        fail.push(e.clone());
    }
    if let Err(e) = &entry {
        fail.push(e.clone());
    }
    if fail.is_empty() {
        lr.ok = true;
        lr.detail = format!(
            "{}; continuous across instance windows; clean thread entry",
            lr.detail
        );
    } else {
        lr.ok = false;
        lr.detail = fail.join(" | ");
    }
    lr
}

/// STL must be non-empty, match viewport AABB, AND look like the same helix.
/// A smooth rod with the same bbox must not PASS.
pub fn check_stl_look_right(
    stl: &[u8],
    mesh_bbox: [f64; 6],
    viewport: Option<&MeshData>,
) -> (bool, String, Option<[f64; 6]>) {
    let Some(vp) = viewport else {
        return (false, "no viewport mesh (execute failed)".into(), None);
    };
    if stl.len() < 84 {
        return (false, format!("STL too small ({} bytes)", stl.len()), None);
    }
    let Some(stl_mesh) = stl_to_mesh(stl) else {
        return (false, "could not parse binary STL vertices".into(), None);
    };
    let stl_bb = bbox_from_mesh(&stl_mesh);
    let tri = stl_mesh.positions.len() / 9;
    if !bbox_close(mesh_bbox, stl_bb, STL_BBOX_TOL_MM) {
        return (
            false,
            format!(
                "bbox mismatch: mesh {} vs STL {} (tol {STL_BBOX_TOL_MM} mm)",
                fmt_bb(mesh_bbox),
                fmt_bb(stl_bb)
            ),
            Some(stl_bb),
        );
    }
    let lr = check_viewport_look_right(&stl_mesh);
    if !lr.ok {
        return (
            false,
            format!(
                "AABB matches ({tri} tris) but STL is not a continuous helix/ISO-V (smooth-rod or seamed-slab fake would pass bbox-only): {}",
                lr.detail
            ),
            Some(stl_bb),
        );
    }
    let vp_lr = check_viewport_look_right(vp);
    if !vp_lr.ok {
        return (
            false,
            format!(
                "STL looks threaded but viewport mesh does not: {}",
                vp_lr.detail
            ),
            Some(stl_bb),
        );
    }
    (
        true,
        format!(
            "non-empty ({tri} tris); bbox {} matches mesh within {STL_BBOX_TOL_MM} mm; helix/ISO-V/sliver + continuous windows + clean entry ok",
            fmt_bb(stl_bb)
        ),
        Some(stl_bb),
    )
}

pub fn bbox_tol_mm() -> f64 {
    STL_BBOX_TOL_MM
}

pub fn radius_variation_at_z(mesh: &MeshData, z: f64, band: f64) -> f64 {
    let mut rs = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() <= band {
            let x = chunk[0] as f64;
            let y = chunk[1] as f64;
            rs.push((x * x + y * y).sqrt());
        }
    }
    if rs.len() < 8 {
        return 0.0;
    }
    let mean = rs.iter().sum::<f64>() / rs.len() as f64;
    (rs.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rs.len() as f64).sqrt()
}

fn groove_yaw_at_z(mesh: &MeshData, z: f64, band: f64) -> Option<f64> {
    const N: usize = 32;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() > band {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < 2.5 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
    }
    let mut best = None;
    let mut best_r = f64::MAX;
    for i in 0..N {
        if counts[i] >= 2 && mins[i] < best_r {
            best_r = mins[i];
            let yaw = (i as f64 + 0.5) / N as f64 * 2.0 * std::f64::consts::PI - std::f64::consts::PI;
            best = Some(yaw);
        }
    }
    best.filter(|_| best_r < 3.85)
}

pub fn distinct_groove_yaws(mesh: &MeshData, z0: f64, z1: f64, samples: usize) -> usize {
    let mut bins = std::collections::HashSet::new();
    for i in 0..samples {
        let z = z0 + (z1 - z0) * (i as f64) / (samples as f64);
        if let Some(y) = groove_yaw_at_z(mesh, z, 0.1) {
            let bin = (((y + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * 16.0).floor()
                as i32;
            bins.insert(bin.rem_euclid(16));
        }
    }
    bins.len()
}

pub fn angular_radius_spread_at_z(mesh: &MeshData, z: f64, band: f64) -> f64 {
    const N: usize = 16;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() > band {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < 2.0 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
    }
    let vals: Vec<f64> = mins
        .iter()
        .zip(counts.iter())
        .filter(|(_, c)| **c >= 2)
        .map(|(m, _)| *m)
        .collect();
    if vals.len() < 8 {
        return 0.0;
    }
    vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - vals.iter().copied().fold(f64::INFINITY, f64::min)
}

pub fn no_vertical_uncut_strip(
    mesh: &MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) -> Result<(), String> {
    if mesh.positions.is_empty() {
        return Err("empty mesh — WASM trap / placeholder, not a threaded shank".into());
    }
    let depth = kernel::thread::external_depth(pitch);
    let cut_r = r_major - 0.28 * depth;
    const N: usize = 24;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    let mut plus_x_min = f64::MAX;
    let mut plus_x_n = 0u32;
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let z = chunk[2] as f64;
        if z < z0 || z > z1 {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < r_major * 0.55 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
        if theta.abs() < 0.14 {
            plus_x_min = plus_x_min.min(r);
            plus_x_n += 1;
        }
    }
    let populated = counts.iter().filter(|c| **c >= 4).count();
    if populated < N * 2 / 3 {
        return Err(format!(
            "too few yaw bins have vertices ({populated}/{N}) — placeholder or tessellation failed"
        ));
    }
    let mut uncut: Vec<(usize, f64, u32)> = Vec::new();
    for i in 0..N {
        if counts[i] >= 6 && mins[i] > cut_r {
            uncut.push((i, mins[i], counts[i]));
        }
    }
    if !uncut.is_empty() {
        return Err(format!(
            "vertical uncut strip: yaw bins {:?} stay above r={cut_r:.3} (major={r_major})",
            uncut
                .iter()
                .map(|(i, r, n)| format!("bin{i} r={r:.3} n={n}"))
                .collect::<Vec<_>>()
        ));
    }
    if plus_x_n < 4 {
        return Err("+X meridian has no shank vertices — cannot inspect the sliver".into());
    }
    if plus_x_min > cut_r {
        return Err(format!(
            "+X meridian still at r={plus_x_min:.3} (cut below {cut_r:.3}) — sliver"
        ));
    }
    let panel_z0 = ((z0 + z1) * 0.5 - 4.0).max(z0);
    let panel_z1 = (panel_z0 + 8.0).min(z1);
    let panel = max_full_height_uncut_yaw_span_deg(mesh, r_major, panel_z0, panel_z1);
    if panel >= 25.0 {
        return Err(format!(
            "leftover uncut cylinder panel spans {panel:.1}° of yaw"
        ));
    }
    Ok(())
}

fn max_full_height_uncut_yaw_span_deg(mesh: &MeshData, r_major: f64, z0: f64, z1: f64) -> f64 {
    let min_zspan = ((z1 - z0) * 0.55).clamp(5.0, 7.5);
    let r_lo = r_major - 0.08;
    let r_hi = r_major + 0.15;
    let tris: Vec<[usize; 3]> = if mesh.indices.is_empty() {
        (0..mesh.positions.len() / 9)
            .map(|t| {
                let i = t * 3;
                [i, i + 1, i + 2]
            })
            .collect()
    } else {
        mesh.indices
            .chunks(3)
            .filter_map(|c| {
                if c.len() == 3 {
                    Some([c[0] as usize, c[1] as usize, c[2] as usize])
                } else {
                    None
                }
            })
            .collect()
    };
    let mut yaws: Vec<f64> = Vec::new();
    for [a, b, c] in tris {
        let p = |i: usize| {
            [
                mesh.positions[i * 3] as f64,
                mesh.positions[i * 3 + 1] as f64,
                mesh.positions[i * 3 + 2] as f64,
            ]
        };
        let pa = p(a);
        let pb = p(b);
        let pc = p(c);
        let rs = [
            (pa[0] * pa[0] + pa[1] * pa[1]).sqrt(),
            (pb[0] * pb[0] + pb[1] * pb[1]).sqrt(),
            (pc[0] * pc[0] + pc[1] * pc[1]).sqrt(),
        ];
        if rs.iter().copied().fold(f64::INFINITY, f64::min) < r_lo
            || rs.iter().copied().fold(0.0_f64, f64::max) > r_hi
        {
            continue;
        }
        let zs = [pa[2], pb[2], pc[2]];
        let zspan = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - zs.iter().copied().fold(f64::INFINITY, f64::min);
        if zspan < min_zspan {
            continue;
        }
        let yaw = (pa[1] + pb[1] + pc[1]).atan2(pa[0] + pb[0] + pc[0]);
        yaws.push(yaw);
    }
    if yaws.len() < 3 {
        return 0.0;
    }
    yaws.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut best: f64 = 0.0;
    let mut run_start = yaws[0];
    let mut prev = yaws[0];
    for &y in &yaws[1..] {
        if y - prev > 0.22 {
            best = best.max(prev - run_start);
            run_start = y;
        }
        prev = y;
    }
    best = best.max(prev - run_start);
    let wrap = yaws[0] + 2.0 * std::f64::consts::PI - yaws[yaws.len() - 1];
    if wrap <= 0.22 {
        let extra = (yaws[0] - (-std::f64::consts::PI)) + (std::f64::consts::PI - yaws[yaws.len() - 1]);
        best = best.max(extra);
    }
    best * 180.0 / std::f64::consts::PI
}

fn shank_samples(mesh: &MeshData, z0: f64, z1: f64, r_min: f64) -> Vec<[f64; 3]> {
    let mut pts = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let p = [chunk[0] as f64, chunk[1] as f64, chunk[2] as f64];
        if p[2] >= z0 && p[2] <= z1 && (p[0] * p[0] + p[1] * p[1]).sqrt() >= r_min {
            pts.push(p);
        }
    }
    let tris: Vec<[usize; 3]> = if mesh.indices.is_empty() {
        (0..mesh.positions.len() / 9)
            .map(|t| [t * 3, t * 3 + 1, t * 3 + 2])
            .collect()
    } else {
        mesh.indices
            .chunks(3)
            .filter_map(|c| (c.len() == 3).then_some([c[0] as usize, c[1] as usize, c[2] as usize]))
            .collect()
    };
    for [a, b, c] in tris {
        if (a + 1) * 3 > mesh.positions.len()
            || (b + 1) * 3 > mesh.positions.len()
            || (c + 1) * 3 > mesh.positions.len()
        {
            continue;
        }
        let pa = [
            mesh.positions[a * 3] as f64,
            mesh.positions[a * 3 + 1] as f64,
            mesh.positions[a * 3 + 2] as f64,
        ];
        let pb = [
            mesh.positions[b * 3] as f64,
            mesh.positions[b * 3 + 1] as f64,
            mesh.positions[b * 3 + 2] as f64,
        ];
        let pc = [
            mesh.positions[c * 3] as f64,
            mesh.positions[c * 3 + 1] as f64,
            mesh.positions[c * 3 + 2] as f64,
        ];
        let g = [
            (pa[0] + pb[0] + pc[0]) / 3.0,
            (pa[1] + pb[1] + pc[1]) / 3.0,
            (pa[2] + pb[2] + pc[2]) / 3.0,
        ];
        if g[2] >= z0 && g[2] <= z1 && (g[0] * g[0] + g[1] * g[1]).sqrt() >= r_min {
            pts.push(g);
        }
    }
    pts
}

pub fn iso_v_thread_profile(
    mesh: &MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) -> Result<(), String> {
    let samples = shank_samples(mesh, z0, z1, r_major * 0.55);
    if samples.len() < 80 {
        return Err(format!(
            "too few shank samples ({}) — placeholder or tessellation failed",
            samples.len()
        ));
    }

    const N: usize = 32;
    let mut min_r = [f64::MAX; N];
    let mut counts = [0u32; N];
    let mut n_flank = 0u32;
    let mut r_floor = f64::MAX;
    let depth = kernel::thread::external_depth(pitch);
    for p in &samples {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        r_floor = r_floor.min(r);
        if r > r_major - 0.85 * depth && r < r_major - 0.15 {
            n_flank += 1;
        }
        let yaw = p[1].atan2(p[0]);
        let phase = (p[2] / pitch - yaw / (2.0 * std::f64::consts::PI)).rem_euclid(1.0);
        let bin = ((phase * N as f64).floor() as usize).min(N - 1);
        min_r[bin] = min_r[bin].min(r);
        counts[bin] += 1;
    }
    let populated: Vec<usize> = (0..N).filter(|&i| counts[i] >= 1).collect();
    if populated.len() < 10 {
        return Err(format!(
            "too few thread-phase bins ({}/{})",
            populated.len(),
            N
        ));
    }

    let crest_bins = populated.iter().filter(|&&i| min_r[i] > r_major - 0.12).count();
    let crest_frac = crest_bins as f64 / populated.len() as f64;
    if crest_frac >= 0.22 {
        return Err(format!(
            "thread crest is boxy/wide: {crest_frac:.2} of pitch stays at the major (ISO crest ~0.125)"
        ));
    }
    if r_floor >= r_major - 0.40 * depth {
        return Err(format!(
            "groove too shallow (r={r_floor:.3} vs major {r_major})"
        ));
    }
    let crest_r = populated
        .iter()
        .map(|&i| min_r[i])
        .fold(0.0_f64, |a, r| a.max(r));
    if crest_r <= r_major - 0.20 {
        return Err(format!(
            "crests were cut away (max leftover r={crest_r:.3}); ISO leaves ~P/8 at the major"
        ));
    }
    if n_flank < 20 {
        return Err(format!(
            "flanks missing: only {n_flank} intermediate-r samples"
        ));
    }
    let n_yaws = distinct_groove_yaws(mesh, z0, z1, 12);
    if n_yaws < 5 {
        return Err(format!(
            "groove must still walk around the shank; distinct yaws={n_yaws}"
        ));
    }
    let mid = 0.5 * (z0 + z1);
    let spread = angular_radius_spread_at_z(mesh, mid, (pitch * 0.06).clamp(0.06, 0.10));
    if spread <= 0.25 {
        return Err(format!(
            "ISO crest must still be helical at a thin z-slice; spread={spread}"
        ));
    }
    Ok(())
}

/// Circular mean yaw of the deepest groove vertices in a thin Z band.
/// Same helper as kernel `occt_geometry.rs` (#22) — more stable than a
/// 32-bin argmin that flips between U-flanks.
fn deep_groove_yaw_at_z(
    mesh: &MeshData,
    z: f64,
    band: f64,
    r_major: f64,
    pitch: f64,
) -> Option<f64> {
    let depth = kernel::thread::external_depth(pitch);
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() > band {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < r_major * 0.55 || r > r_major - 0.18 * depth {
            continue;
        }
        pts.push((y.atan2(x), r));
    }
    if pts.len() < 6 {
        return None;
    }
    pts.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let take = (pts.len() / 3).max(4);
    let mut sx = 0.0;
    let mut sy = 0.0;
    for (yaw, _) in pts.iter().take(take) {
        sx += yaw.cos();
        sy += yaw.sin();
    }
    Some(sy.atan2(sx))
}

fn helix_phase_from_yaw(z: f64, yaw: f64, pitch: f64) -> f64 {
    (z / pitch.max(1e-9) - yaw / (2.0 * std::f64::consts::PI)).rem_euclid(1.0)
}

/// Fail if instanced slabs meet with a visible helix step (mid-shank
/// horizontal jumps). Same thresholds as kernel #22: worst < 0.10 turn,
/// rms < 0.08 turn. Does not loosen the mid-shank helix / ISO checks.
pub fn helix_continuous_across_instance_windows(
    mesh: &MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) -> Result<(), String> {
    let step = (pitch * 0.40).clamp(0.35, 0.55);
    let mut phases: Vec<(f64, f64)> = Vec::new();
    let mut z = z0;
    while z <= z1 + 1e-9 {
        if let Some(yaw) = deep_groove_yaw_at_z(mesh, z, 0.14, r_major, pitch) {
            phases.push((z, helix_phase_from_yaw(z, yaw, pitch)));
        }
        z += step;
    }
    if phases.len() < 8 {
        return Err(format!(
            "too few deep-groove phase samples ({}) between {z0:.1} and {z1:.1} — \
             cannot inspect instance seams",
            phases.len()
        ));
    }
    let mut unwrapped = vec![phases[0].1];
    for i in 1..phases.len() {
        let mut p = phases[i].1;
        let prev = unwrapped[i - 1];
        while p - prev > 0.5 {
            p -= 1.0;
        }
        while p - prev < -0.5 {
            p += 1.0;
        }
        unwrapped.push(p);
    }
    let mut worst = 0.0_f64;
    let mut worst_at = phases[0].0;
    for i in 1..unwrapped.len() {
        let d = (unwrapped[i] - unwrapped[i - 1]).abs();
        if d > worst {
            worst = d;
            worst_at = 0.5 * (phases[i - 1].0 + phases[i].0);
        }
    }
    let mean = unwrapped.iter().sum::<f64>() / unwrapped.len() as f64;
    let var = unwrapped.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / unwrapped.len() as f64;
    let rms = var.sqrt();
    if worst < HELIX_PHASE_WORST_TURN && rms < HELIX_PHASE_RMS_TURN {
        Ok(())
    } else {
        Err(format!(
            "helix phase jumps {worst:.3} turn (rms {rms:.3}) near z={worst_at:.2} \
             (instance window seam / yaw discontinuity)"
        ))
    }
}

/// Fail if the first turn is leftover uncut cylinder / a triangular pipe-entry
/// notch (dead-height → thread start). Same probes as kernel #22.
pub fn clean_thread_entry(
    mesh: &MeshData,
    r_major: f64,
    pitch: f64,
    thread_z0: f64,
) -> Result<(), String> {
    let z_probe = thread_z0 + pitch * 0.55;
    let variation = radius_variation_at_z(mesh, z_probe, 0.16);
    if variation <= 0.08 {
        return Err(format!(
            "dead→thread start is uncut or notched (variation={variation:.4} at z={z_probe:.2})"
        ));
    }
    let yaw = groove_yaw_at_z(mesh, z_probe, 0.14);
    let deep = deep_groove_yaw_at_z(mesh, z_probe, 0.14, r_major, pitch);
    if yaw.is_none() && deep.is_none() {
        return Err(format!(
            "no groove just after thread start (z={z_probe:.2}) — leftover triangular notch"
        ));
    }
    // Phase compare uses the deep-root mean (#22): the 32-bin argmin flips
    // ~0.14 turn between U-flanks and is not a seam.
    let z_mid = thread_z0 + pitch * 10.0;
    let y0 = deep.or(yaw);
    let ym = deep_groove_yaw_at_z(mesh, z_mid, 0.14, r_major, pitch)
        .or_else(|| groove_yaw_at_z(mesh, z_mid, 0.14));
    if let (Some(y0), Some(ym)) = (y0, ym) {
        let expected = ym - 2.0 * std::f64::consts::PI * (z_mid - z_probe) / pitch.max(1e-9);
        let mut d = y0 - expected;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        if d.abs() >= 0.45 {
            return Err(format!(
                "thread-start groove yaw is off the helix by {d:.3} rad \
                 (entry notch or wrong first-rod phase)"
            ));
        }
    }
    no_vertical_uncut_strip(
        mesh,
        r_major,
        pitch,
        thread_z0 + pitch * 0.35,
        thread_z0 + pitch * 1.8,
    )
}

/// Dense ISO-ish helical shank used by unit tests (and as a contrast to a smooth rod).
pub fn synthetic_iso_helix_mesh(r_major: f64, pitch: f64, z0: f64, z1: f64) -> MeshData {
    synthetic_iso_helix_mesh_res(r_major, pitch, z0, z1, 80, 48)
}

fn synthetic_iso_helix_mesh_res(
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
    n_z: usize,
    n_yaw: usize,
) -> MeshData {
    let depth = kernel::thread::external_depth(pitch);
    let mut pts: Vec<[f64; 3]> = Vec::new();
    for iz in 0..=n_z {
        let z = z0 + (z1 - z0) * (iz as f64) / (n_z as f64);
        for iy in 0..n_yaw {
            let yaw = -std::f64::consts::PI + 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
            let phase = (z / pitch - yaw / (2.0 * std::f64::consts::PI)).rem_euclid(1.0);
            // ISO-ish: ~P/8 crest, sloped flanks, ~5H/8 depth.
            let t = if phase < 0.06 {
                0.0
            } else if phase < 0.50 {
                (phase - 0.06) / 0.44
            } else if phase < 0.94 {
                1.0 - (phase - 0.50) / 0.44
            } else {
                0.0
            };
            let r = r_major - depth * t;
            pts.push([r * yaw.cos(), r * yaw.sin(), z]);
        }
    }
    // Tiny triangles so sliver/ISO helpers that walk indices still have faces.
    let mut positions = Vec::new();
    let cols = n_yaw;
    let rows = n_z;
    let idx = |iz: usize, iy: usize| iz * cols + iy;
    for iz in 0..rows {
        for iy in 0..cols {
            let a = pts[idx(iz, iy)];
            let b = pts[idx(iz, (iy + 1) % cols)];
            let c = pts[idx(iz + 1, iy)];
            let d = pts[idx(iz + 1, (iy + 1) % cols)];
            for p in [a, b, c, a, c, d] {
                positions.extend_from_slice(&[p[0] as f32, p[1] as f32, p[2] as f32]);
            }
        }
    }
    MeshData {
        positions,
        normals: vec![],
        indices: vec![],
    }
}

/// Smooth Ø8 rod + AF13 hex — same family AABB as the golden, no grooves.
pub fn synthetic_smooth_rod_hex() -> MeshData {
    use crate::golden::{AF_MM, HEAD_HEIGHT_MM, LENGTH_MM, SHANK_R_MM};
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let r_hex = AF_MM / 3.0_f64.sqrt();
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::PI / 3.0;
        pts.push([r_hex * a.cos(), r_hex * a.sin(), 0.0]);
        pts.push([r_hex * a.cos(), r_hex * a.sin(), HEAD_HEIGHT_MM]);
    }
    let n_z = 40usize;
    let n_yaw = 36usize;
    for iz in 0..=n_z {
        let z = HEAD_HEIGHT_MM + (LENGTH_MM - HEAD_HEIGHT_MM) * (iz as f64) / (n_z as f64);
        for iy in 0..n_yaw {
            let yaw = 2.0 * std::f64::consts::PI * (iy as f64) / (n_yaw as f64);
            pts.push([SHANK_R_MM * yaw.cos(), SHANK_R_MM * yaw.sin(), z]);
        }
    }
    crate::mesh_util::mesh_from_points(&pts)
}

/// Full golden-length ISO helix (dead face → tip) for viewport extras.
/// Denser than the mid-band fixture so the #22 entry probe (z≈5.99, band 0.16)
/// and instance-window phase samples actually hit vertices.
pub fn synthetic_iso_helix_m8() -> MeshData {
    synthetic_iso_helix_mesh_res(SHANK_R_MM, PITCH_MM, THREAD_Z0_MM, 40.0, 220, 48)
}

/// Yaw-rotate vertices at/above `z_cut` — a ~50° generator step like the
/// pre-#22 fractional-turn instance stride.
pub fn yaw_rotate_above_z(mesh: &MeshData, z_cut: f64, extra_yaw: f64) -> MeshData {
    let (ct, st) = (extra_yaw.cos() as f32, extra_yaw.sin() as f32);
    let mut out = mesh.clone();
    let n = out.positions.len() / 3;
    for i in 0..n {
        if (out.positions[i * 3 + 2] as f64) < z_cut {
            continue;
        }
        let x = out.positions[i * 3];
        let y = out.positions[i * 3 + 1];
        out.positions[i * 3] = x * ct - y * st;
        out.positions[i * 3 + 1] = x * st + y * ct;
    }
    out
}

/// Push entry-band vertices out to the major — leftover uncut cylinder /
/// pipe-entry notch at dead-height → first thread.
pub fn flatten_entry_to_cylinder(mesh: &MeshData, z_until: f64, r_major: f64) -> MeshData {
    let mut out = mesh.clone();
    let n = out.positions.len() / 3;
    for i in 0..n {
        if (out.positions[i * 3 + 2] as f64) > z_until {
            continue;
        }
        let x = out.positions[i * 3] as f64;
        let y = out.positions[i * 3 + 1] as f64;
        let r = x.hypot(y);
        if r < 0.5 {
            continue;
        }
        let s = r_major / r;
        out.positions[i * 3] = (x * s) as f32;
        out.positions[i * 3 + 1] = (y * s) as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::golden::{PITCH_MM, SHANK_R_MM, SHANK_Z0_MM, SHANK_Z1_MM, THREAD_Z0_MM};
    use crate::mesh_util::bbox_from_mesh;
    use kernel::export::to_stl;

    #[test]
    fn helix_mesh_passes_look_right() {
        let mesh = synthetic_iso_helix_mesh(SHANK_R_MM, PITCH_MM, 8.0, 32.0);
        let lr = check_look_right(&mesh, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM);
        assert!(lr.ok, "{}", lr.detail);
    }

    #[test]
    fn continuous_helix_passes_viewport_look_right() {
        let mesh = synthetic_iso_helix_m8();
        let lr = check_viewport_look_right(&mesh);
        assert!(lr.ok, "{}", lr.detail);
        assert!(lr.continuous_ok, "{}", lr.detail);
        assert!(lr.entry_ok, "{}", lr.detail);
    }

    #[test]
    fn mid_shank_seam_fails_continuity_even_if_helix_bar_passes() {
        // Seam outside the mid-shank ISO band (12–28) so the old helix bar
        // still PASSes; inside the instance-window band (8–36) so 1b FAILs.
        // 90° is the pre-#22 ~50° generator step, made unmistakable.
        let seamed = yaw_rotate_above_z(&synthetic_iso_helix_m8(), 32.0, 90.0_f64.to_radians());
        let mid = check_look_right(&seamed, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM);
        assert!(
            mid.ok,
            "precondition: mid-shank helix/ISO/sliver bar must still pass: {}",
            mid.detail
        );
        let lr = check_viewport_look_right(&seamed);
        assert!(!lr.ok, "seamed helix must FAIL viewport look-right");
        assert!(
            !lr.continuous_ok,
            "instance-window continuity must FAIL, got {}",
            lr.detail
        );
        assert!(
            lr.detail.contains("phase jumps") || lr.detail.contains("seam"),
            "{}",
            lr.detail
        );
    }

    #[test]
    fn notched_entry_fails_even_if_mid_shank_helix_passes() {
        let z_until = THREAD_Z0_MM + PITCH_MM * 1.8;
        let notched = flatten_entry_to_cylinder(&synthetic_iso_helix_m8(), z_until, SHANK_R_MM);
        let mid = check_look_right(&notched, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM);
        assert!(
            mid.ok,
            "precondition: mid-shank helix/ISO/sliver bar must still pass: {}",
            mid.detail
        );
        let lr = check_viewport_look_right(&notched);
        assert!(!lr.ok, "notched entry must FAIL viewport look-right");
        assert!(!lr.entry_ok, "clean entry must FAIL, got {}", lr.detail);
    }

    #[test]
    fn seamed_stl_fails_even_when_aabb_matches() {
        let seamed = yaw_rotate_above_z(&synthetic_iso_helix_m8(), 32.0, 90.0_f64.to_radians());
        let bb = bbox_from_mesh(&seamed);
        let stl = to_stl(&seamed);
        let (ok, detail, _) = check_stl_look_right(&stl, bb, Some(&seamed));
        assert!(!ok, "AABB-matched seamed STL must FAIL: {detail}");
    }

    #[test]
    fn smooth_rod_fails_look_right_despite_golden_bbox() {
        let mesh = synthetic_smooth_rod_hex();
        let bb = bbox_from_mesh(&mesh);
        assert!(bb[4] - bb[1] > 12.0, "hex AF should set Y span, bb={bb:?}");
        assert!((bb[5] - bb[2] - 40.0).abs() < 1.0);
        let lr = check_look_right(&mesh, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM);
        assert!(!lr.ok, "smooth rod must FAIL look-right, got {}", lr.detail);
        let vp = check_viewport_look_right(&mesh);
        assert!(!vp.ok, "smooth rod must FAIL viewport extras, got {}", vp.detail);
    }

    #[test]
    fn aabb_only_stl_of_smooth_rod_does_not_pass() {
        let rod = synthetic_smooth_rod_hex();
        let rod_bb = bbox_from_mesh(&rod);
        let stl = to_stl(&rod);
        let (ok, detail, _) = check_stl_look_right(&stl, rod_bb, Some(&rod));
        assert!(
            !ok,
            "AABB-matched smooth-rod STL must FAIL look-right: {detail}"
        );
    }
}
