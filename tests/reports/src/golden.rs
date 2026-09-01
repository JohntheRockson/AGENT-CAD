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

pub const FILLET_RADIUS_MM: f64 = 0.8;

pub fn load_golden_document(text: &str) -> Result<CadDocument, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("parse golden JSON: {e}"))?;
    CadDocument::from_json_value(value).map_err(|e| format!("CadDocument: {e}"))
}

/// Assert the IR itself is the locked caliper (no OCCT required).
pub fn check_golden_ir(doc: &CadDocument) -> (bool, String) {
    let af = doc
        .parameters
        .get("head_width")
        .copied()
        .or_else(|| first_hex_af(doc));
    let head = doc
        .parameters
        .get("head_height")
        .copied()
        .or_else(|| first_extrude_depth(doc));
    let length = doc.parameters.get("bolt_length").copied();

    let mut fail: Vec<String> = Vec::new();
    match af {
        Some(v) if (v - AF_MM).abs() < 1e-6 => {}
        other => fail.push(format!("across_flats {other:?} (want {AF_MM})")),
    }
    match head {
        Some(v) if (v - HEAD_HEIGHT_MM).abs() < 0.15 => {}
        other => fail.push(format!("head_height {other:?} (want ~{HEAD_HEIGHT_MM})")),
    }
    match length {
        Some(v) if (v - LENGTH_MM).abs() < 1e-6 => {}
        other => fail.push(format!("bolt_length {other:?} (want {LENGTH_MM})")),
    }
    if !has_m8_thread(doc) {
        fail.push("missing external M8 thread".into());
    }
    if !has_d8_shank(doc) {
        fail.push("missing Ø8 shank cylinder".into());
    }
    if fail.is_empty() {
        (
            true,
            format!(
                "locked ISO caliper: AF {AF_MM}, Ø{SHANK_D_MM}, P {PITCH_MM}, L {LENGTH_MM}, head ~{HEAD_HEIGHT_MM}"
            ),
        )
    } else {
        (false, format!("golden IR drift: {}", fail.join("; ")))
    }
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

fn has_m8_thread(doc: &CadDocument) -> bool {
    use kernel::ir::{Feature, ThreadKind};
    doc.bodies.iter().any(|b| {
        b.features.iter().any(|f| match f {
            Feature::Thread(t) => {
                t.kind == ThreadKind::External
                    && t.size
                        .as_deref()
                        .map(|s| s.to_ascii_uppercase().contains("M8"))
                        .unwrap_or(false)
            }
            _ => false,
        })
    })
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
}
