//! Locked ISO M8×40 caliper golden — AF 13, Ø8, P 1.25, L 40, head ~5.3.

use kernel::ir::CadDocument;

/// Wrench size (ISO hex-head across flats), mm.
pub const AF_MM: f64 = 13.0;
/// Shank / major diameter, mm.
pub const SHANK_D_MM: f64 = 8.0;
pub const SHANK_R_MM: f64 = SHANK_D_MM * 0.5;
/// ISO 261 coarse pitch for M8, mm.
pub const PITCH_MM: f64 = 1.25;
/// Overall length, mm.
pub const LENGTH_MM: f64 = 40.0;
/// ISO hex-cap head height, mm.
pub const HEAD_HEIGHT_MM: f64 = 5.3;
/// Thread start Z (bearing face), mm.
pub const THREAD_Z0_MM: f64 = 5.3;
/// Thread length on the golden, mm.
pub const THREAD_LEN_MM: f64 = 34.7;

/// Mid-shank band used for helix / ISO-V / sliver (avoids head and tip).
pub const SHANK_Z0_MM: f64 = 12.0;
pub const SHANK_Z1_MM: f64 = 28.0;

/// Instance-window continuity band — matches `occt_geometry.rs` (#22)
/// `assert_helix_continuous_across_instance_windows(..., zmin+8, zmin+36)`.
pub const HELIX_WINDOW_Z0_MM: f64 = 8.0;
pub const HELIX_WINDOW_Z1_MM: f64 = 36.0;

pub const FILLET_RADIUS_MM: f64 = 0.8;

/// Executed tip-to-top vs locked L=40. Crest tessellation at 40.095 must
/// still PASS. A window that overshoots L by more than this is FAIL.
/// Tip-to-top AABB only — not ISO 4017 under-head length.
pub const TIP_LENGTH_TOL_MM: f64 = 0.20;

pub fn load_golden_document(text: &str) -> Result<CadDocument, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("parse golden JSON: {e}"))?;
    CadDocument::from_json_value(value).map_err(|e| format!("CadDocument: {e}"))
}

/// Seconds-class budget for golden M8 execute (viewport-fast instance path).
/// Warmup is separate. Do not tessellate a 30-turn uncut host.
pub const EXECUTE_BUDGET_S: f64 = 40.0;

/// Assert the IR itself is the locked caliper (no OCCT required).
/// Parameters **and** feature geometry must both match — a golden
/// `head_width` must not hide an AF 10 hex sketch.
pub fn check_golden_ir(doc: &CadDocument) -> (bool, String) {
    let param_af = doc.parameters.get("head_width").copied();
    let feat_af = first_hex_af(doc);
    let param_head = doc.parameters.get("head_height").copied();
    let feat_head = first_extrude_depth(doc);
    let length = doc.parameters.get("bolt_length").copied();

    let mut fail: Vec<String> = Vec::new();
    lock_dim("param head_width", param_af, AF_MM, 1e-6, &mut fail);
    lock_dim("hex across_flats", feat_af, AF_MM, 1e-6, &mut fail);
    lock_dim("param head_height", param_head, HEAD_HEIGHT_MM, 0.15, &mut fail);
    lock_dim("extrude depth", feat_head, HEAD_HEIGHT_MM, 0.15, &mut fail);
    lock_dim("param bolt_length", length, LENGTH_MM, 1e-6, &mut fail);
    if let Some(&p) = doc.parameters.get("pitch") {
        if (p - PITCH_MM).abs() > 1e-6 {
            fail.push(format!("param pitch {p} (want {PITCH_MM})"));
        }
    }
    if let Some(&d) = doc
        .parameters
        .get("major_diameter")
        .or_else(|| doc.parameters.get("shank_diameter"))
    {
        if (d - SHANK_D_MM).abs() > 1e-6 {
            fail.push(format!("param major/shank Ø {d} (want {SHANK_D_MM})"));
        }
    }
    if let (Some(p), Some(f)) = (param_af, feat_af) {
        if (p - f).abs() > 1e-6 {
            fail.push(format!("head_width param {p} ≠ hex AF {f}"));
        }
    }
    if let (Some(p), Some(f)) = (param_head, feat_head) {
        if (p - f).abs() > 0.15 {
            fail.push(format!("head_height param {p} ≠ extrude {f}"));
        }
    }
    match thread_lock(doc) {
        Ok(()) => {}
        Err(e) => fail.push(e),
    }
    if !has_d8_shank(doc) {
        fail.push("missing Ø8 shank cylinder".into());
    }
    if fail.is_empty() {
        (
            true,
            format!(
                "locked ISO caliper: AF {AF_MM}, Ø{SHANK_D_MM}, P {PITCH_MM}, L {LENGTH_MM}, head ~{HEAD_HEIGHT_MM} (params + features)"
            ),
        )
    } else {
        (false, format!("golden IR drift: {}", fail.join("; ")))
    }
}

fn lock_dim(label: &str, got: Option<f64>, want: f64, tol: f64, fail: &mut Vec<String>) {
    match got {
        Some(v) if (v - want).abs() <= tol => {}
        other => fail.push(format!("{label} {other:?} (want {want})")),
    }
}

/// Viewport-fast execute: FAIL if golden M8 took longer than [`EXECUTE_BUDGET_S`].
pub fn check_execute_seconds(secs: f64) -> (bool, String) {
    if !secs.is_finite() || secs < 0.0 {
        return (false, format!("execute time not usable ({secs})"));
    }
    if secs > EXECUTE_BUDGET_S {
        (
            false,
            format!(
                "golden execute {secs:.1}s exceeds {EXECUTE_BUDGET_S}s class \
                 (viewport-fast instance path; do not tessellate a long uncut host)"
            ),
        )
    } else {
        (
            true,
            format!("golden execute {secs:.2}s (budget {EXECUTE_BUDGET_S}s)"),
        )
    }
}

/// Executed AABB tip-to-top vs locked L=40. Overshoot or a short span
/// beyond [`TIP_LENGTH_TOL_MM`] is FAIL. Does not interpret under-head ISO 4017.
pub fn check_tip_to_top_length(bbox: [f64; 6]) -> (bool, String) {
    let zmin = bbox[2];
    let zmax = bbox[5];
    let span = zmax - zmin;
    let overshoot = zmax - LENGTH_MM;
    let span_err = span - LENGTH_MM;
    if !zmin.is_finite() || !zmax.is_finite() || span <= 0.0 {
        return (
            false,
            format!("no usable Z bbox {bbox:?} — cannot verify tip-to-top L={LENGTH_MM}"),
        );
    }
    if overshoot > TIP_LENGTH_TOL_MM {
        return (
            false,
            format!(
                "tip overshoots locked L={LENGTH_MM}: zmax={zmax:.4} (Δ={overshoot:.4} mm, \
                 tol {TIP_LENGTH_TOL_MM} mm tip-to-top; not ISO 4017 under-head)"
            ),
        );
    }
    if span_err > TIP_LENGTH_TOL_MM {
        return (
            false,
            format!(
                "tip-to-top span overshoots L={LENGTH_MM}: span={span:.4} \
                 (z=[{zmin:.4}, {zmax:.4}], Δ={span_err:.4} mm, tol {TIP_LENGTH_TOL_MM} mm)"
            ),
        );
    }
    if span < LENGTH_MM - TIP_LENGTH_TOL_MM {
        return (
            false,
            format!(
                "tip-to-top shorter than locked L={LENGTH_MM}: span={span:.4} \
                 (z=[{zmin:.4}, {zmax:.4}], tol {TIP_LENGTH_TOL_MM} mm)"
            ),
        );
    }
    (
        true,
        format!(
            "tip-to-top zmax={zmax:.4} span={span:.4} vs L={LENGTH_MM} \
             (tol {TIP_LENGTH_TOL_MM} mm; 40.095-class crest is ok)"
        ),
    )
}

fn first_hex_af(doc: &CadDocument) -> Option<f64> {
    use kernel::ir::{Feature, Profile};
    for body in &doc.bodies {
        for f in &body.features {
            if let Feature::Sketch(sk) = f {
                if let Profile::Hex(h) = &sk.profile {
                    return Some(h.across_flats);
                }
            }
        }
    }
    None
}

fn first_extrude_depth(doc: &CadDocument) -> Option<f64> {
    use kernel::ir::Feature;
    for body in &doc.bodies {
        for f in &body.features {
            if let Feature::Extrude(ex) = f {
                return Some(ex.depth);
            }
        }
    }
    None
}

/// Coarse M8 only: size token plus optional pitch/Ø overrides, cut length,
/// and start Z. `M8x1` (fine) or a `pitch: 2` override must not PASS.
fn thread_lock(doc: &CadDocument) -> Result<(), String> {
    use kernel::ir::{Feature, ThreadKind};
    let mut found = None;
    for body in &doc.bodies {
        for f in &body.features {
            if let Feature::Thread(t) = f {
                found = Some(t);
                break;
            }
        }
    }
    let t = found.ok_or_else(|| "missing thread feature".to_string())?;
    if t.kind != ThreadKind::External {
        return Err(format!("thread kind {:?} (want external)", t.kind));
    }
    let size = t
        .size
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase()
        .replace('×', "X");
    let coarse = size == "M8" || size == "M8X1.25";
    if !coarse {
        return Err(format!(
            "thread size {size:?} (want M8 coarse P={PITCH_MM}; M8x1 fine is not the golden)"
        ));
    }
    if let Some(p) = t.pitch {
        if (p - PITCH_MM).abs() > 1e-6 {
            return Err(format!("thread pitch override {p} (want {PITCH_MM})"));
        }
    }
    if let Some(d) = t.diameter {
        if (d - SHANK_D_MM).abs() > 1e-6 {
            return Err(format!("thread diameter override {d} (want {SHANK_D_MM})"));
        }
    }
    if (t.length - THREAD_LEN_MM).abs() > 0.15 {
        return Err(format!(
            "thread length {} (want {THREAD_LEN_MM})",
            t.length
        ));
    }
    if (t.at[2] - THREAD_Z0_MM).abs() > 0.15 {
        return Err(format!("thread at.z {} (want {THREAD_Z0_MM})", t.at[2]));
    }
    Ok(())
}

fn has_d8_shank(doc: &CadDocument) -> bool {
    use kernel::ir::Feature;
    doc.bodies.iter().any(|b| {
        b.features.iter().any(|f| match f {
            Feature::Cylinder(c) => (c.diameter - SHANK_D_MM).abs() < 1e-6,
            _ => false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_m8_x40_json_is_iso_af13() {
        let text = include_str!("../m8_x40.json");
        let doc = load_golden_document(text).expect("golden document");
        let (ok, detail) = check_golden_ir(&doc);
        assert!(ok, "{detail}");
        assert!(
            (doc.parameters["head_width"] - 13.0).abs() < 1e-12,
            "must not silently accept AF 10"
        );
        assert!((doc.parameters["head_height"] - 5.3).abs() < 1e-12);
        assert!((doc.parameters["bolt_length"] - 40.0).abs() < 1e-12);
    }

    #[test]
    fn tip_40_095_passes_length_tol() {
        let (ok, detail) = check_tip_to_top_length([-7.5, -6.5, 0.0, 7.5, 6.5, 40.095]);
        assert!(ok, "{detail}");
    }

    #[test]
    fn tip_exact_40_passes_length() {
        let (ok, detail) = check_tip_to_top_length([-7.5, -6.5, 0.0, 7.5, 6.5, 40.0]);
        assert!(ok, "{detail}");
    }

    #[test]
    fn tip_overshoot_half_mm_fails() {
        let (ok, detail) = check_tip_to_top_length([-7.5, -6.5, 0.0, 7.5, 6.5, 40.5]);
        assert!(!ok, "0.5 mm overshoot must FAIL: {detail}");
        assert!(detail.contains("overshoot"), "{detail}");
    }

    #[test]
    fn tip_short_span_fails() {
        let (ok, detail) = check_tip_to_top_length([-7.5, -6.5, 0.0, 7.5, 6.5, 38.0]);
        assert!(!ok, "38 mm span must FAIL: {detail}");
        assert!(detail.contains("shorter"), "{detail}");
    }

    #[test]
    fn param_af13_with_hex_af10_fails() {
        let text = include_str!("../m8_x40.json");
        let mut doc = load_golden_document(text).expect("golden document");
        use kernel::ir::{Feature, Profile};
        for body in &mut doc.bodies {
            for f in &mut body.features {
                if let Feature::Sketch(sk) = f {
                    if let Profile::Hex(h) = &mut sk.profile {
                        h.across_flats = 10.0;
                    }
                }
            }
        }
        let (ok, detail) = check_golden_ir(&doc);
        assert!(!ok, "AF10 hex behind head_width=13 must FAIL: {detail}");
        assert!(
            detail.contains("across_flats") || detail.contains("hex AF"),
            "{detail}"
        );
    }

    #[test]
    fn param_head_with_wrong_extrude_fails() {
        let text = include_str!("../m8_x40.json");
        let mut doc = load_golden_document(text).expect("golden document");
        use kernel::ir::Feature;
        for body in &mut doc.bodies {
            for f in &mut body.features {
                if let Feature::Extrude(ex) = f {
                    ex.depth = 4.0;
                }
            }
        }
        let (ok, detail) = check_golden_ir(&doc);
        assert!(!ok, "extrude 4.0 behind head_height=5.3 must FAIL: {detail}");
        assert!(detail.contains("extrude"), "{detail}");
    }

    #[test]
    fn m8x1_fine_pitch_size_fails() {
        let text = include_str!("../m8_x40.json");
        let mut doc = load_golden_document(text).expect("golden document");
        use kernel::ir::Feature;
        for body in &mut doc.bodies {
            for f in &mut body.features {
                if let Feature::Thread(t) = f {
                    t.size = Some("M8x1".into());
                }
            }
        }
        let (ok, detail) = check_golden_ir(&doc);
        assert!(!ok, "M8x1 fine must FAIL ISO lock: {detail}");
        assert!(detail.contains("M8X1") || detail.contains("fine"), "{detail}");
    }

    #[test]
    fn pitch_override_2_fails() {
        let text = include_str!("../m8_x40.json");
        let mut doc = load_golden_document(text).expect("golden document");
        use kernel::ir::Feature;
        for body in &mut doc.bodies {
            for f in &mut body.features {
                if let Feature::Thread(t) = f {
                    t.pitch = Some(2.0);
                }
            }
        }
        let (ok, detail) = check_golden_ir(&doc);
        assert!(!ok, "pitch override 2.0 must FAIL: {detail}");
        assert!(detail.contains("pitch"), "{detail}");
    }

    #[test]
    fn execute_8s_passes_budget() {
        let (ok, detail) = check_execute_seconds(8.4);
        assert!(ok, "{detail}");
    }

    #[test]
    fn execute_41s_fails_budget() {
        let (ok, detail) = check_execute_seconds(41.0);
        assert!(!ok, "41s must FAIL 40s class: {detail}");
        assert!(detail.contains("exceeds"), "{detail}");
    }
}
