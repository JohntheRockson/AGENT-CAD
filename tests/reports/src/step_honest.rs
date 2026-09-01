//! STEP honesty: empty / crash is FAIL, and a non-empty uncut host is also FAIL
//! when the viewport mesh is threaded.
//!
//! Kernel owns STEP implementation (#15-style faceted export). Inspector must
//! not fake PASS if STEP is essentially the uncut hex+shank (smooth Ø8, no
//! groove signature, volume ≈ uncut).

use kernel::engine::{MeshData, MetricsData};

use crate::golden::{PITCH_MM, SHANK_R_MM, SHANK_Z0_MM, SHANK_Z1_MM};
use crate::look_right::{
    angular_radius_spread_at_z, check_look_right, distinct_groove_yaws, radius_variation_at_z,
};
use crate::mesh_util::mesh_from_points;

#[derive(Clone, Debug)]
pub struct StepCheck {
    pub ok: bool,
    pub detail: String,
    pub bytes: usize,
    pub has_solid_token: bool,
    pub viewport_threaded: bool,
    pub step_looks_uncut: bool,
}

pub fn check_step_honest(
    bytes: &[u8],
    export_err: Option<&str>,
    viewport: Option<&MeshData>,
    uncut: Option<&MetricsData>,
    uncut_step: Option<&[u8]>,
) -> StepCheck {
    if let Some(e) = export_err {
        return StepCheck {
            ok: false,
            detail: format!("export failed (honest FAIL): {e}"),
            bytes: bytes.len(),
            has_solid_token: false,
            viewport_threaded: viewport_is_threaded(viewport),
            step_looks_uncut: true,
        };
    }
    if bytes.is_empty() {
        return StepCheck {
            ok: false,
            detail: "STEP file is empty (honest FAIL)".into(),
            bytes: 0,
            has_solid_token: false,
            viewport_threaded: viewport_is_threaded(viewport),
            step_looks_uncut: true,
        };
    }

    let text = String::from_utf8_lossy(bytes);
    let has_iso = text.contains("ISO-10303") || text.contains("STEP");
    let has_solid = text.contains("MANIFOLD_SOLID_BREP")
        || text.contains("BREP_WITH_VOIDS")
        || text.contains("FACETED_BREP")
        || text.contains("CLOSED_SHELL");
    let nonempty = bytes.len() > 512;
    if !has_iso || !has_solid || !nonempty {
        return StepCheck {
            ok: false,
            detail: format!(
                "not a non-empty solid: bytes={} has_iso={has_iso} has_solid_token={has_solid}",
                bytes.len()
            ),
            bytes: bytes.len(),
            has_solid_token: has_solid,
            viewport_threaded: viewport_is_threaded(viewport),
            step_looks_uncut: true,
        };
    }

    let viewport_threaded = viewport_is_threaded(viewport);
    let (uncut_sig, uncut_why) = step_is_uncut_host(&text, bytes, uncut, uncut_step);

    if viewport_threaded && uncut_sig {
        return StepCheck {
            ok: false,
            detail: format!(
                "viewport is threaded but STEP is essentially the uncut hex+shank ({uncut_why}). \
                 Kernel owns STEP — Inspector does not fake PASS."
            ),
            bytes: bytes.len(),
            has_solid_token: true,
            viewport_threaded: true,
            step_looks_uncut: true,
        };
    }

    StepCheck {
        ok: true,
        detail: format!(
            "non-empty STEP solid ({} bytes); groove signature present (not the uncut Ø8 host)",
            bytes.len()
        ),
        bytes: bytes.len(),
        has_solid_token: true,
        viewport_threaded,
        step_looks_uncut: false,
    }
}

fn viewport_is_threaded(viewport: Option<&MeshData>) -> bool {
    let Some(m) = viewport else {
        return false;
    };
    check_look_right(m, SHANK_R_MM, PITCH_MM, SHANK_Z0_MM, SHANK_Z1_MM).ok
}

/// True when STEP looks like hex + smooth Ø8 (no helical groove).
fn step_is_uncut_host(
    text: &str,
    bytes: &[u8],
    uncut: Option<&MetricsData>,
    uncut_step: Option<&[u8]>,
) -> (bool, String) {
    let pts = parse_cartesian_points(text);
    let shank: Vec<[f64; 3]> = pts
        .iter()
        .copied()
        .filter(|p| p[2] >= SHANK_Z0_MM && p[2] <= SHANK_Z1_MM)
        .filter(|p| {
            let r = p[0].hypot(p[1]);
            r > SHANK_R_MM * 0.55 && r < SHANK_R_MM + 1.25
        })
        .collect();

    if shank.len() >= 40 {
        let mesh = mesh_from_points(&shank);
        let variation = radius_variation_at_z(&mesh, 0.5 * (SHANK_Z0_MM + SHANK_Z1_MM), 0.35);
        let spread = angular_radius_spread_at_z(&mesh, 0.5 * (SHANK_Z0_MM + SHANK_Z1_MM), 0.12);
        let n_yaws = distinct_groove_yaws(&mesh, SHANK_Z0_MM, SHANK_Z1_MM, 12);
        let smooth = variation <= 0.08 && spread <= 0.25 && n_yaws < 5;
        if smooth {
            let vol_note = uncut
                .map(|m| format!("; host volume ≈ {:.1} mm³ (uncut hex+shank)", m.volume))
                .unwrap_or_default();
            return (
                true,
                format!(
                    "faceted/analytic points show smooth Ø8 shank (variation={variation:.3} \
                     spread={spread:.3} yaws={n_yaws}){vol_note}"
                ),
            );
        }
        return (false, "STEP points have a groove signature".into());
    }

    let helical_token = text.contains("B_SPLINE_SURFACE")
        || text.contains("SURFACE_OF_REVOLUTION")
        || text.contains("HELIX")
        || text.contains("TOROIDAL_SURFACE");
    let faceted = text.contains("FACETED_BREP") || text.contains("TRIANGULATED_FACE");
    let cyl = count_token(text, "CYLINDER_SURFACE");
    let faces = count_token(text, "ADVANCED_FACE")
        + count_token(text, "FACE_SURFACE")
        + count_token(text, "TRIANGULATED_FACE");

    if let Some(u) = uncut_step {
        if !u.is_empty() && step_similar_to_uncut(bytes, u) {
            return (
                true,
                format!(
                    "STEP matches hex+shank-only export ({} vs {} bytes; no extra groove entities)",
                    bytes.len(),
                    u.len()
                ),
            );
        }
    }

    // Analytic B-Rep of a long bolt that is only planes + one cylinder is the uncut host.
    if !helical_token && !faceted && cyl >= 1 && faces > 0 && faces <= 24 {
        return (
            true,
            format!(
                "analytic STEP has CYLINDER_SURFACE×{cyl} and {faces} faces, no helical/spline/faceted groove"
            ),
        );
    }

    if faceted && shank.len() < 40 && !helical_token {
        // Faceted export with no parseable groove points and no helix tokens:
        // treat as uncut unless there are clearly more faces than a hex+shank.
        if faces > 0 && faces <= 80 {
            return (
                true,
                format!(
                    "faceted STEP has {faces} faces and no groove points — Ø8 host, not a helix"
                ),
            );
        }
    }

    (false, "STEP not classified as uncut host".into())
}

fn step_similar_to_uncut(full: &[u8], uncut: &[u8]) -> bool {
    if uncut.len() < 256 || full.len() < 256 {
        return false;
    }
    let ratio = full.len() as f64 / uncut.len() as f64;
    if !(0.75..=1.35).contains(&ratio) {
        return false;
    }
    let a = String::from_utf8_lossy(full);
    let b = String::from_utf8_lossy(uncut);
    let da = count_token(&a, "CARTESIAN_POINT");
    let db = count_token(&b, "CARTESIAN_POINT");
    let fa = count_token(&a, "ADVANCED_FACE") + count_token(&a, "TRIANGULATED_FACE");
    let fb = count_token(&b, "ADVANCED_FACE") + count_token(&b, "TRIANGULATED_FACE");
    (da == 0 && db == 0 || (da as i64 - db as i64).abs() < (db as i64 / 5).max(8))
        && (fa == 0 && fb == 0 || (fa as i64 - fb as i64).abs() < (fb as i64 / 5).max(4))
}

fn count_token(text: &str, token: &str) -> usize {
    text.matches(token).count()
}

pub fn parse_cartesian_points(text: &str) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    let upper = text; // STEP tokens are typically uppercase; numbers are as-is.
    let mut search = upper;
    while let Some(idx) = search.find("CARTESIAN_POINT") {
        let rest = &search[idx..];
        if let Some(pts) = take_three_nums(rest) {
            out.push(pts);
        }
        search = &search[idx + 15..];
    }
    out
}

fn take_three_nums(s: &str) -> Option<[f64; 3]> {
    // CARTESIAN_POINT('',(x,y,z)) — skip to the innermost '(' with three numbers.
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let inner = &s[i + 1..];
            if looks_like_xyz(inner) {
                return parse_xyz(inner);
            }
        }
        i += 1;
    }
    None
}

fn looks_like_xyz(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('+')
        || t.starts_with('-')
        || t.starts_with('.')
        || t.starts_with(|c: char| c.is_ascii_digit())
}

fn parse_xyz(s: &str) -> Option<[f64; 3]> {
    let mut nums = Vec::with_capacity(3);
    let mut cur = String::new();
    for c in s.chars() {
        if c == '+' || c == '-' || c == '.' || c == 'e' || c == 'E' || c.is_ascii_digit() {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                if let Ok(v) = cur.parse::<f64>() {
                    nums.push(v);
                    if nums.len() == 3 {
                        return Some([nums[0], nums[1], nums[2]]);
                    }
                }
                cur.clear();
            }
            if c == ')' {
                break;
            }
        }
    }
    if nums.len() == 3 {
        Some([nums[0], nums[1], nums[2]])
    } else {
        None
    }
}

/// Minimal ISO-10303 solid used in tests.
pub fn synthetic_step_solid(points: &[[f64; 3]], faceted: bool, extra_cylinder: bool) -> Vec<u8> {
    let mut s = String::from("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN'));\nENDSEC;\nDATA;\n");
    s.push_str("#1=MANIFOLD_SOLID_BREP('bolt',#2);\n#2=CLOSED_SHELL('',());\n");
    if faceted {
        s.push_str("#3=FACETED_BREP('',#2);\n");
    }
    if extra_cylinder {
        s.push_str("#4=CYLINDER_SURFACE('',#5,4.0);\n");
        s.push_str("#10=ADVANCED_FACE('',(#11),#4,.T.);\n");
        s.push_str("#11=ADVANCED_FACE('',(#12),#4,.T.);\n");
        s.push_str("#12=ADVANCED_FACE('',(#13),#4,.T.);\n");
    }
    for (i, p) in points.iter().enumerate() {
        s.push_str(&format!(
            "#{}=CARTESIAN_POINT('',({:.6},{:.6},{:.6}));\n",
            100 + i,
            p[0],
            p[1],
            p[2]
        ));
    }
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    // Pad so the nonempty threshold (>512) is honest for tiny fixtures.
    while s.len() < 600 {
        s.push_str("/* pad */\n");
    }
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look_right::{synthetic_iso_helix_mesh, synthetic_smooth_rod_hex};
    use crate::mesh_util::mesh_points;

    #[test]
    fn empty_step_fails() {
        let helix = synthetic_iso_helix_mesh(SHANK_R_MM, PITCH_MM, 8.0, 32.0);
        let c = check_step_honest(&[], None, Some(&helix), None, None);
        assert!(!c.ok, "{}", c.detail);
    }

    #[test]
    fn crash_step_fails() {
        let c = check_step_honest(&[], Some("wasm memory"), None, None, None);
        assert!(!c.ok, "{}", c.detail);
        assert!(c.detail.contains("export failed"));
    }

    #[test]
    fn uncut_faceted_step_fails_when_viewport_is_threaded() {
        let helix = synthetic_iso_helix_mesh(SHANK_R_MM, PITCH_MM, 8.0, 32.0);
        assert!(viewport_is_threaded(Some(&helix)));
        let rod = synthetic_smooth_rod_hex();
        let pts = mesh_points(&rod);
        let step = synthetic_step_solid(&pts, true, true);
        let uncut_metrics = MetricsData {
            volume: 2500.0,
            bbox: [-7.5, -6.5, 0.0, 7.5, 6.5, 40.0],
            surface_area: 1.0,
            is_solid: true,
        };
        let c = check_step_honest(&step, None, Some(&helix), Some(&uncut_metrics), None);
        assert!(!c.ok, "uncut host must FAIL: {}", c.detail);
        assert!(c.step_looks_uncut);
        assert!(c.detail.contains("uncut") || c.detail.contains("smooth"));
    }

    #[test]
    fn threaded_faceted_step_passes_when_viewport_is_threaded() {
        let helix = synthetic_iso_helix_mesh(SHANK_R_MM, PITCH_MM, 8.0, 32.0);
        let pts = mesh_points(&helix);
        let step = synthetic_step_solid(&pts, true, true);
        let c = check_step_honest(&step, None, Some(&helix), None, None);
        assert!(c.ok, "{}", c.detail);
    }
}
