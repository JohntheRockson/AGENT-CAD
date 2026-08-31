//! ISO metric and unified-inch thread designations.
//!
//! Diameters and pitches are stored in millimetres. Convert to document
//! units at the call site (`mm` as-is, `in` ÷ 25.4).

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadSpec {
    pub designation: String,
    /// Major diameter in millimetres.
    pub major_diameter: f64,
    /// Pitch in millimetres.
    pub pitch: f64,
}

/// ISO 261 coarse pitches (mm).
const METRIC_COARSE: &[(f64, f64)] = &[
    (1.0, 0.25),
    (1.2, 0.25),
    (1.4, 0.3),
    (1.6, 0.35),
    (1.8, 0.35),
    (2.0, 0.4),
    (2.2, 0.45),
    (2.5, 0.45),
    (3.0, 0.5),
    (3.5, 0.6),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (7.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (33.0, 3.5),
    (36.0, 4.0),
    (39.0, 4.0),
    (42.0, 4.5),
    (45.0, 4.5),
    (48.0, 5.0),
    (52.0, 5.0),
    (56.0, 5.5),
    (60.0, 5.5),
    (64.0, 6.0),
    (68.0, 6.0),
    (72.0, 6.0),
    (80.0, 6.0),
    (90.0, 6.0),
    (100.0, 6.0),
];

/// Parse a designation such as `M8`, `M8x1`, `1/4-20`, or `#8-32`.
/// Result is always millimetres.
pub fn parse_size(raw: &str) -> Result<ThreadSpec, String> {
    let mut s = raw.trim().to_ascii_uppercase();
    s = s.replace('×', "X");
    s = s.replace('"', "");
    s.retain(|c| !c.is_whitespace());
    for prefix in ["UNC", "UNF", "UNS", "UN"] {
        if s.starts_with(prefix) && s.len() > prefix.len() {
            let rest = &s[prefix.len()..];
            if rest.starts_with('-') || rest.starts_with('X') || rest.starts_with('#') || rest.starts_with(char::is_numeric) {
                s = rest.trim_start_matches('-').to_string();
            }
            break;
        }
    }
    if s.ends_with("UNC") || s.ends_with("UNF") {
        s.truncate(s.len() - 3);
        s = s.trim_end_matches('-').to_string();
    }

    if let Some(rest) = s.strip_prefix('M') {
        return parse_metric(rest);
    }
    parse_inch(&s)
}

fn parse_metric(rest: &str) -> Result<ThreadSpec, String> {
    let rest = rest.replace(',', ".");
    let (d_str, p_str) = if let Some(i) = rest.find('X') {
        (&rest[..i], Some(&rest[i + 1..]))
    } else {
        (rest.as_str(), None)
    };
    let d: f64 = d_str.parse().map_err(|_| {
        format!("could not parse metric size 'M{rest}' (try M8 or M8x1)")
    })?;
    if d <= 0.0 {
        return Err("metric thread diameter must be positive".into());
    }
    let pitch = if let Some(p) = p_str {
        let p: f64 = p.parse().map_err(|_| format!("could not parse pitch in 'M{rest}'"))?;
        if p <= 0.0 {
            return Err("thread pitch must be positive".into());
        }
        p
    } else {
        metric_coarse_pitch(d).ok_or_else(|| {
            format!("no ISO coarse pitch for M{d}; give pitch explicitly (e.g. M{d}x1)")
        })?
    };
    Ok(ThreadSpec {
        designation: if p_str.is_some() {
            format!("M{d}x{pitch}")
        } else {
            format!("M{d}")
        },
        major_diameter: d,
        pitch,
    })
}

fn metric_coarse_pitch(d: f64) -> Option<f64> {
    METRIC_COARSE
        .iter()
        .find(|(dia, _)| (*dia - d).abs() < 1e-6)
        .map(|(_, p)| *p)
        .or_else(|| {
            METRIC_COARSE
                .iter()
                .min_by(|a, b| {
                    (a.0 - d)
                        .abs()
                        .partial_cmp(&(b.0 - d).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .filter(|(dia, _)| (dia - d).abs() / d.max(1.0) < 0.05)
                .map(|(_, p)| *p)
        })
}

fn parse_inch(s: &str) -> Result<ThreadSpec, String> {
    let s = s.trim_start_matches('#');
    let (dia_str, tpi_str) = s
        .rsplit_once('-')
        .or_else(|| s.rsplit_once('X'))
        .ok_or_else(|| {
            format!("could not parse thread size '{s}' (try M8, M8x1, 1/4-20, or #8-32)")
        })?;
    let tpi: f64 = tpi_str.parse().map_err(|_| {
        format!("could not parse threads-per-inch in '{s}'")
    })?;
    if tpi <= 0.0 {
        return Err("threads-per-inch must be positive".into());
    }
    let dia_in = parse_inch_diameter(dia_str)?;
    if dia_in <= 0.0 {
        return Err("inch thread diameter must be positive".into());
    }
    Ok(ThreadSpec {
        designation: format!("{dia_str}-{tpi_str}"),
        major_diameter: dia_in * 25.4,
        pitch: 25.4 / tpi,
    })
}

/// Numbered machine screws use D = 0.060 + N×0.013 inches (#0–#12).
fn parse_inch_diameter(s: &str) -> Result<f64, String> {
    if let Some(i) = s.find('/') {
        let n: f64 = s[..i]
            .parse()
            .map_err(|_| format!("bad fraction '{s}'"))?;
        let d: f64 = s[i + 1..]
            .parse()
            .map_err(|_| format!("bad fraction '{s}'"))?;
        if d == 0.0 {
            return Err("fraction denominator cannot be 0".into());
        }
        return Ok(n / d);
    }
    let n: f64 = s.parse().map_err(|_| format!("bad diameter '{s}'"))?;
    if (0.0..=12.0).contains(&n) && (n - n.round()).abs() < 1e-9 {
        Ok(0.060 + n * 0.013)
    } else {
        Ok(n)
    }
}

/// ISO 68-1 fundamental triangle height H = (√3/2)·P.
pub fn triangle_height(pitch: f64) -> f64 {
    0.866_025_403_784_438_6 * pitch
}

/// External thread depth of engagement (5H/8), millimetres.
pub fn external_depth(pitch: f64) -> f64 {
    0.625 * triangle_height(pitch)
}

/// Typical 75% tap-drill diameter ≈ major − pitch (shop rule for coarse metric).
pub fn tap_drill_diameter(major: f64, pitch: f64) -> f64 {
    (major - pitch).max(major * 0.5)
}

/// ISO 68-1 external minor radius: major/2 − 5H/8.
pub fn external_minor_radius(major: f64, pitch: f64) -> f64 {
    major / 2.0 - external_depth(pitch)
}

/// Flat crest width of an ISO 68-1 external thread (H/8 truncation) = P/8.
pub fn iso_crest_width(pitch: f64) -> f64 {
    pitch / 8.0
}

/// Extra turns swept before θ = 0 and after the last turn.
///
/// The cutter helix used to start and end on +X — the same generator as a
/// hex-head vertex. OCCT's pipe then leaves that meridian uncut, a vertical
/// sliver of the original cylinder. Overlapping the start/end azimuth closes
/// the 360° sweep without adding a full extra turn (WASM budget).
pub const CUTTER_SEAM_OVERLAP_TURNS: f64 = 0.15;

/// Polyline helix for a thread cutter. `z0` is the Z of the first *nominal*
/// turn (t = 0); samples extend `CUTTER_SEAM_OVERLAP_TURNS` before and after
/// so the groove overlaps its start/end meridian.
pub fn cutter_helix_path(
    radius: f64,
    pitch: f64,
    height: f64,
    z0: f64,
    pts_per_turn: u32,
) -> Vec<[f64; 3]> {
    cutter_helix_path_phased(radius, pitch, height, z0, pts_per_turn, 0.0)
}

/// `phase_samples` shifts the polyline by a fraction of a step so a second
/// bead can cover C0 vertex generators the first path leaves behind.
pub fn cutter_helix_path_phased(
    radius: f64,
    pitch: f64,
    height: f64,
    z0: f64,
    pts_per_turn: u32,
    phase_samples: f64,
) -> Vec<[f64; 3]> {
    let pitch = pitch.max(1e-9);
    let turns = (height / pitch).max(0.25);
    let ppt = f64::from(pts_per_turn.max(8));
    let t0 = -CUTTER_SEAM_OVERLAP_TURNS + phase_samples / ppt;
    let t1 = turns + CUTTER_SEAM_OVERLAP_TURNS + phase_samples / ppt;
    let n = (((t1 - t0) * ppt).ceil() as usize).max(8);
    (0..=n)
        .map(|i| {
            let t = t0 + (t1 - t0) * (i as f64) / (n as f64);
            let a = t * 2.0 * std::f64::consts::PI;
            [radius * a.cos(), radius * a.sin(), z0 + t * pitch]
        })
        .collect()
}

/// Point and tangent (unnormalized) on the cutter helix at turn parameter `t`
/// (`t = 0` is the first nominal turn, `z = z0`).
pub fn cutter_helix_frame(radius: f64, pitch: f64, t: f64, z0: f64) -> ([f64; 3], [f64; 3]) {
    let a = t * 2.0 * std::f64::consts::PI;
    let (c, s) = (a.cos(), a.sin());
    let p = [radius * c, radius * s, z0 + t * pitch];
    let tang = [
        -radius * 2.0 * std::f64::consts::PI * s,
        radius * 2.0 * std::f64::consts::PI * c,
        pitch,
    ];
    (p, tang)
}

/// Square bead in the meridian plane at `yaw` (axis-through-point). Matches
/// the historical XZ square when `yaw == 0`.
pub fn cutter_meridian_square(radius: f64, sec_r: f64, yaw: f64, z: f64) -> [[f64; 3]; 4] {
    let (c, s) = (yaw.cos(), yaw.sin());
    let r0 = radius - sec_r;
    let r1 = radius + sec_r;
    [
        [r0 * c, r0 * s, z - sec_r],
        [r1 * c, r1 * s, z - sec_r],
        [r1 * c, r1 * s, z + sec_r],
        [r0 * c, r0 * s, z + sec_r],
    ]
}

/// Compact V-groove in the meridian plane at `yaw`.
pub fn cutter_meridian_vee(r_out: f64, r_in: f64, half: f64, yaw: f64, z: f64) -> [[f64; 3]; 3] {
    let (c, s) = (yaw.cos(), yaw.sin());
    [
        [r_out * c, r_out * s, z],
        [r_out * c, r_out * s, z + 2.0 * half],
        [r_in * c, r_in * s, z + half],
    ]
}

/// Orthonormal frame at a helix sample: outward normal `n` (in the plane ⊥
/// tangent), binormal `b = t × n`, unit tangent `t`.
pub fn cutter_helix_normal_frame(
    origin: [f64; 3],
    tang: [f64; 3],
) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let t = normalize(tang);
    let rxy = (origin[0] * origin[0] + origin[1] * origin[1]).sqrt();
    let radial = if rxy < 1e-12 {
        [1.0, 0.0, 0.0]
    } else {
        [origin[0] / rxy, origin[1] / rxy, 0.0]
    };
    let n = normalize([
        radial[0] - t[0] * dot(radial, t),
        radial[1] - t[1] * dot(radial, t),
        radial[2] - t[2] * dot(radial, t),
    ]);
    let b = normalize(cross(t, n));
    (n, b, t)
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-15);
    [v[0] / n, v[1] / n, v[2] / n]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Frenet-friendly 60° ISO 68-1 V in the plane ⊥ helix tangent.
///
/// A meridian-plane V that spans ~1 pitch in Z Frenet-pipes into ring-like
/// troughs. This section lives in the normal plane (same idea as the circular
/// bead), so the groove yaws with the helix instead of wrapping into rings.
///
/// External: apex at the theoretical sharp root (7H/8 below major) so a
/// cylinder boolean leaves H/8 crests and 60° flanks. Internal: inverted V
/// cutting out from the tap-drill wall.
pub fn cutter_iso_vee_normal(
    origin: [f64; 3],
    tang: [f64; 3],
    major: f64,
    pitch: f64,
    internal: bool,
) -> [[f64; 3]; 3] {
    let h = triangle_height(pitch);
    let r_major = major / 2.0;
    let (r_in, r_out, half) = if internal {
        let r_hole = tap_drill_diameter(major, pitch) / 2.0;
        (
            (r_hole - 0.12 * pitch).max(0.05),
            r_major + 0.12 * pitch,
            pitch * 0.45,
        )
    } else {
        // Fundamental triangle: sharp root 7H/8 in, sharp crest H/8 out,
        // plus a hair of overshoot so the boolean clears the major cylinder.
        (
            r_major - 0.875 * h,
            r_major + 0.125 * h,
            pitch * 0.50,
        )
    };

    let (n, b, _t) = cutter_helix_normal_frame(origin, tang);
    let r_path = (origin[0] * origin[0] + origin[1] * origin[1]).sqrt();
    let u_in = r_in - r_path;
    let u_out = r_out - r_path;
    let pt = |u: f64, v: f64| -> [f64; 3] {
        [
            origin[0] + n[0] * u + b[0] * v,
            origin[1] + n[1] * u + b[1] * v,
            origin[2] + n[2] * u + b[2] * v,
        ]
    };
    [pt(u_out, -half), pt(u_in, 0.0), pt(u_out, half)]
}

/// Circular bead that matches ISO crest (P/8 leftover) and 5H/8 root as
/// closely as a circle can. MakePipeShell only accepts an in-place disk;
/// this is the fallback when a lofted V solid cannot be built.
///
/// Returns `(path_radius, section_radius)` in the meridian plane: the
/// circumcircle of the minor-radius root and the two crest-edge points
/// at the major.
pub fn cutter_iso_circle(major: f64, pitch: f64) -> (f64, f64) {
    let r_major = major / 2.0;
    let r_root = external_minor_radius(major, pitch);
    let half_open = (pitch - iso_crest_width(pitch)) * 0.5;
    let dr = (r_major - r_root).max(1e-9);
    let mid_r = 0.5 * (r_root + r_major);
    let t = -(half_open * 0.5) / dr;
    let r_h = mid_r - t * half_open;
    let sec_r = ((r_major - r_h).powi(2) + half_open.powi(2)).sqrt();
    (r_h.max(r_root + 1e-3), sec_r.max(pitch * 0.15))
}

/// Path radius through the middle of [`cutter_iso_vee_normal`].
pub fn cutter_iso_path_radius(major: f64, pitch: f64, internal: bool) -> f64 {
    if internal {
        tap_drill_diameter(major, pitch) / 2.0
    } else {
        let h = triangle_height(pitch);
        // Midway between sharp root (major/2 − 7H/8) and overshooting crest.
        major / 2.0 - 0.375 * h
    }
}

/// ISO 68-1 60° V in the axial (meridian) plane — the standard thread section.
/// Pair with a FixedUp (+Z) pipe so the V does not Frenet-roll into rings.
pub fn cutter_iso_vee_meridian(
    major: f64,
    pitch: f64,
    yaw: f64,
    z_apex: f64,
    internal: bool,
) -> [[f64; 3]; 3] {
    let h = triangle_height(pitch);
    let r_major = major / 2.0;
    let (r_in, r_out, half) = if internal {
        let r_hole = tap_drill_diameter(major, pitch) / 2.0;
        (
            (r_hole - 0.12 * pitch).max(0.05),
            r_major + 0.12 * pitch,
            pitch * 0.45,
        )
    } else {
        (
            r_major - 0.875 * h,
            r_major + 0.125 * h,
            pitch * 0.50,
        )
    };
    cutter_meridian_vee(r_out, r_in, half, yaw, z_apex - half)
}

/// ISO V in the XY plane (radial = X, along-thread = Y). Transform onto
/// [`cutter_helix_normal_frame`] so OCCT sees a real planar face ⊥ the spine.
pub fn cutter_iso_vee_xy(
    major: f64,
    pitch: f64,
    path_radius: f64,
    internal: bool,
) -> [[f64; 3]; 3] {
    let h = triangle_height(pitch);
    let r_major = major / 2.0;
    let (r_in, r_out, half) = if internal {
        let r_hole = tap_drill_diameter(major, pitch) / 2.0;
        (
            (r_hole - 0.12 * pitch).max(0.05),
            r_major + 0.12 * pitch,
            pitch * 0.45,
        )
    } else {
        (
            r_major - 0.875 * h,
            r_major + 0.125 * h,
            pitch * 0.50,
        )
    };
    let u_in = r_in - path_radius;
    let u_out = r_out - path_radius;
    [
        [u_out, -half, 0.0],
        [u_in, 0.0, 0.0],
        [u_out, half, 0.0],
    ]
}

/// 3×4 row-major rigid map: e_x→n, e_y→b, e_z→t, origin→`origin`.
pub fn frame_to_matrix3x4(
    n: [f64; 3],
    b: [f64; 3],
    t: [f64; 3],
    origin: [f64; 3],
) -> [f64; 12] {
    [
        n[0], b[0], t[0], origin[0], n[1], b[1], t[1], origin[1], n[2], b[2], t[2], origin[2],
    ]
}

/// Convert a millimetre spec into document units.
pub fn to_units(spec: &ThreadSpec, inch: bool) -> ThreadSpec {
    if !inch {
        return spec.clone();
    }
    ThreadSpec {
        designation: spec.designation.clone(),
        major_diameter: spec.major_diameter / 25.4,
        pitch: spec.pitch / 25.4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m8_coarse() {
        let s = parse_size("M8").unwrap();
        assert!((s.major_diameter - 8.0).abs() < 1e-9);
        assert!((s.pitch - 1.25).abs() < 1e-9);
    }

    #[test]
    fn m8_fine() {
        let s = parse_size("M8x1").unwrap();
        assert!((s.pitch - 1.0).abs() < 1e-9);
    }

    #[test]
    fn quarter_twenty() {
        let s = parse_size("1/4-20").unwrap();
        assert!((s.major_diameter - 6.35).abs() < 1e-6);
        assert!((s.pitch - 1.27).abs() < 0.01);
    }

    #[test]
    fn numbered_eight_thirty_two() {
        let s = parse_size("#8-32").unwrap();
        assert!((s.major_diameter - 0.164 * 25.4).abs() < 1e-6);
    }

    #[test]
    fn cutter_helix_path_overlaps_plus_x_seam() {
        let path = cutter_helix_path(4.0, 1.25, 8.0, 0.0, 24);
        assert!(path.len() >= 16, "path too short: {}", path.len());

        let start_yaw = path[0][1].atan2(path[0][0]);
        assert!(
            start_yaw < -0.2,
            "cutter must start before the +X hex-vertex seam, yaw={start_yaw}"
        );

        let mut total = 0.0;
        for w in path.windows(2) {
            let a0 = w[0][1].atan2(w[0][0]);
            let a1 = w[1][1].atan2(w[1][0]);
            let mut d = a1 - a0;
            if d < -std::f64::consts::PI {
                d += 2.0 * std::f64::consts::PI;
            }
            if d > std::f64::consts::PI {
                d -= 2.0 * std::f64::consts::PI;
            }
            assert!(d > -1e-9, "helix path walked backwards: {d}");
            total += d;
        }
        let nominal = 2.0 * std::f64::consts::PI * (8.0 / 1.25);
        assert!(
            total > nominal + 0.5,
            "angular sweep {total:.3} must overlap past {nominal:.3} (close 360°)"
        );

        let mut bins = [false; 32];
        for p in &path {
            let yaw = p[1].atan2(p[0]);
            let mut bin = (((yaw + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * 32.0)
                .floor() as isize;
            if bin < 0 {
                bin = 0;
            }
            bins[(bin as usize).min(31)] = true;
        }
        assert!(
            bins.iter().all(|&hit| hit),
            "cutter helix missed a yaw sector — that is the uncut sliver"
        );
    }

    #[test]
    fn cutter_meridian_square_matches_xz_at_zero_yaw() {
        let sq = cutter_meridian_square(4.0, 0.4, 0.0, 0.0);
        assert!((sq[0][0] - 3.6).abs() < 1e-12 && sq[0][1].abs() < 1e-12);
        assert!((sq[1][0] - 4.4).abs() < 1e-12 && sq[1][1].abs() < 1e-12);
    }

    #[test]
    fn cutter_helix_frame_at_zero_is_plus_x() {
        let (p, t) = cutter_helix_frame(4.0, 1.25, 0.0, 0.0);
        assert!((p[0] - 4.0).abs() < 1e-12 && p[1].abs() < 1e-12);
        assert!(t[1] > 0.0 && t[0].abs() < 1e-12);
    }

    #[test]
    fn iso_m8_crest_and_depth() {
        let p = 1.25;
        assert!((iso_crest_width(p) - 0.15625).abs() < 1e-12);
        let depth = external_depth(p);
        // 5H/8 = 5/8 * √3/2 * P ≈ 0.5413 P
        assert!((depth - 0.541266 * p).abs() < 1e-4);
        assert!((external_minor_radius(8.0, p) - (4.0 - depth)).abs() < 1e-12);
    }

    #[test]
    fn cutter_iso_circle_leaves_p8_crest() {
        let p = 1.25;
        let (r_h, sec_r) = cutter_iso_circle(8.0, p);
        let r_major = 4.0;
        let r_root = external_minor_radius(8.0, p);
        assert!(
            (r_h - sec_r - r_root).abs() < 1e-6,
            "circle must reach 5H/8 root: r_h={r_h} sec_r={sec_r} root={r_root}"
        );
        let du = r_major - r_h;
        assert!(du < sec_r, "circle must cross the major, du={du} r={sec_r}");
        let half_open = (sec_r * sec_r - du * du).sqrt();
        let leftover = p - 2.0 * half_open;
        assert!(
            (leftover - iso_crest_width(p)).abs() < 1e-6,
            "leftover crest {leftover:.4} must be P/8, not a wide bead"
        );
        // A circle centered mid-triangle with r≈0.37 left ~0.50 mm of crest.
        assert!(leftover < 0.22, "crest {leftover:.3} is still boxy");
    }

    #[test]
    fn cutter_iso_vee_is_sixty_degree_not_square() {
        let r = cutter_iso_path_radius(8.0, 1.25, false);
        let (p0, tang) = cutter_helix_frame(r, 1.25, -CUTTER_SEAM_OVERLAP_TURNS, 0.0);
        let v = cutter_iso_vee_normal(p0, tang, 8.0, 1.25, false);
        let apex = v[1];
        let a = v[0];
        let c = v[2];
        let d0 = [
            a[0] - apex[0],
            a[1] - apex[1],
            a[2] - apex[2],
        ];
        let d1 = [
            c[0] - apex[0],
            c[1] - apex[1],
            c[2] - apex[2],
        ];
        let cos = dot(d0, d1) / (dot(d0, d0).sqrt() * dot(d1, d1).sqrt());
        let deg = cos.clamp(-1.0, 1.0).acos() * 180.0 / std::f64::consts::PI;
        assert!(
            (deg - 60.0).abs() < 8.0,
            "ISO V included angle should be ~60°, got {deg:.1}° (square bead is 90°)"
        );

        let r_apex = (apex[0] * apex[0] + apex[1] * apex[1]).sqrt();
        let r_a = (a[0] * a[0] + a[1] * a[1]).sqrt();
        let r_c = (c[0] * c[0] + c[1] * c[1]).sqrt();
        assert!(
            r_apex < 3.3,
            "apex must sit at the theoretical root, r={r_apex}"
        );
        assert!(
            r_a > 4.0 && r_c > 4.0,
            "outer corners must overshoot the major cylinder"
        );

        // A square/rectangular bead has a wide flat at the major. The ISO V
        // opening at the major radius is ~0.875 P, leaving only P/8 crest.
        let opening = ((a[0] - c[0]).powi(2) + (a[1] - c[1]).powi(2) + (a[2] - c[2]).powi(2)).sqrt();
        assert!(
            opening > 1.0,
            "V opening {opening:.3} is too narrow — that is the boxy crest"
        );

        let mer = cutter_iso_vee_meridian(8.0, 1.25, 0.0, 0.0, false);
        let apex = mer[2];
        let a = mer[0];
        let c = mer[1];
        let d0 = [a[0] - apex[0], a[1] - apex[1], a[2] - apex[2]];
        let d1 = [c[0] - apex[0], c[1] - apex[1], c[2] - apex[2]];
        let cos = dot(d0, d1) / (dot(d0, d0).sqrt() * dot(d1, d1).sqrt());
        let deg = cos.clamp(-1.0, 1.0).acos() * 180.0 / std::f64::consts::PI;
        assert!(
            (deg - 60.0).abs() < 2.0,
            "meridian ISO V must be 60°, got {deg:.1}°"
        );
    }
}
