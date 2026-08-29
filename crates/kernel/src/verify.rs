//! Deterministic geometry verification — no LLM required.
//!
//! Used by the server repair loop and available for direct API calls. All
//! lengths are compared in **document units** ([`UnitContext`]).

use serde::{Deserialize, Serialize};

use crate::engine::{DocumentOutput, MetricsData};
use crate::ir::{CadDocument, CadProgram, Feature, Units};
use crate::units::UnitContext;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationReport {
    pub passed: bool,
    pub checks: Vec<VerificationCheck>,
}

impl VerificationReport {
    pub fn summary(&self) -> String {
        self.checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| format!("{}: {}", c.name, c.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

fn check(name: impl Into<String>, passed: bool, message: impl Into<String>) -> VerificationCheck {
    VerificationCheck {
        name: name.into(),
        passed,
        message: message.into(),
    }
}

/// Structural checks that every successful build should pass.
pub fn verify_structure(document: &CadDocument, output: &DocumentOutput) -> VerificationReport {
    let ctx = UnitContext::new(document.units.clone());
    let mut checks = Vec::new();

    let visible_bodies: Vec<_> = output
        .bodies
        .iter()
        .filter(|b| b.visible && !b.suppressed)
        .collect();

    checks.push(check(
        "has_visible_body",
        !visible_bodies.is_empty(),
        if visible_bodies.is_empty() {
            "No visible bodies were produced".into()
        } else {
            format!("{} visible bod(ies)", visible_bodies.len())
        },
    ));

    let m = &output.metrics;
    checks.push(check(
        "positive_volume",
        m.volume > 0.0,
        format!("combined volume = {:.3} {}", m.volume, ctx.units.volume_suffix()),
    ));

    checks.push(check(
        "is_solid",
        m.is_solid,
        if m.is_solid {
            "combined result is a solid".to_string()
        } else {
            "combined result is not a closed solid".to_string()
        },
    ));

    let [xmin, ymin, zmin, xmax, ymax, zmax] = m.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    let max_ext = dx.max(dy).max(dz);
    let min_ext = dx.min(dy).min(dz);

    checks.push(check(
        "not_planar",
        max_ext < 1e-6 || min_ext / max_ext >= 0.03,
        format!(
            "extents {:.2}×{:.2}×{:.2} {} — disk/washer if one axis ≈ 0",
            dx, dy, dz, ctx.units.length_suffix()
        ),
    ));

    let active_doc_bodies = document
        .bodies
        .iter()
        .filter(|b| !b.suppressed)
        .count();
    let produced = visible_bodies.len();
    checks.push(check(
        "body_count",
        produced >= active_doc_bodies.saturating_sub(count_consumed_tools(document)),
        format!(
            "document has {active_doc_bodies} active bod(ies), kernel produced {produced} mesh(es)"
        ),
    ));

    if let Some(declared) = first_declared_box_size(document) {
        let sorted_actual = sort3([dx, dy, dz]);
        let sorted_decl = sort3(declared);
        let ok = sorted_actual
            .iter()
            .zip(sorted_decl.iter())
            .all(|(a, d)| ctx.tolerant_eq(*d, *a));
        checks.push(check(
            "declared_box_bbox",
            ok,
            format!(
                "first box size [{:.1}, {:.1}, {:.1}] vs bbox [{:.1}, {:.1}, {:.1}] {}",
                declared[0],
                declared[1],
                declared[2],
                dx,
                dy,
                dz,
                ctx.units.length_suffix()
            ),
        ));
    }

    checks.extend(verify_parameters(document, output));

    let passed = checks.iter().all(|c| c.passed);
    VerificationReport { passed, checks }
}

/// Compare named `parameters` against measured geometry (no natural-language parsing).
pub fn verify_parameters(document: &CadDocument, output: &DocumentOutput) -> Vec<VerificationCheck> {
    if document.parameters.is_empty() {
        return Vec::new();
    }

    let ctx = UnitContext::new(document.units);
    let [xmin, ymin, zmin, xmax, ymax, zmax] = output.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    let extents = [dx, dy, dz];

    let mut checks = Vec::new();
    let mut unmatched_extents = extents.to_vec();

    for (name, expected) in &document.parameters {
        let matched_idx = unmatched_extents
            .iter()
            .position(|ext| ctx.tolerant_eq(*expected, *ext));

        let passed = matched_idx.is_some();
        let message = if passed {
            let ext = unmatched_extents[matched_idx.unwrap()];
            format!(
                "parameter `{name}` = {:.1} {} matches bbox extent {:.1} {}",
                expected,
                ctx.units.length_suffix(),
                ext,
                ctx.units.length_suffix()
            )
        } else {
            format!(
                "parameter `{name}` = {:.1} {} — no bbox extent in [{:.1}, {:.1}, {:.1}] {}",
                expected,
                ctx.units.length_suffix(),
                dx,
                dy,
                dz,
                ctx.units.length_suffix()
            )
        };

        checks.push(check(format!("param_{name}"), passed, message));

        if let Some(i) = matched_idx {
            unmatched_extents.remove(i);
        }
    }

    checks
}

/// Full verification used by the agent repair loop.
pub fn verify_document(
    user_message: &str,
    document: &CadDocument,
    output: &DocumentOutput,
) -> VerificationReport {
    let mut report = verify_structure(document, output);

    // Natural-language size hints only when the document has no parameter table.
    if document.parameters.is_empty() {
        let ctx = UnitContext::new(document.units.clone());

        if let Some(spec) = parse_size_triple(user_message, &document.units) {
            let [xmin, ymin, zmin, xmax, ymax, zmax] = output.metrics.bbox;
            let actual = sort3([(xmax - xmin).abs(), (ymax - ymin).abs(), (zmax - zmin).abs()]);
            let expected = sort3(spec);
            let ok = actual
                .iter()
                .zip(expected.iter())
                .all(|(a, e)| ctx.tolerant_eq(*e, *a));
            report.checks.push(check(
                "user_bbox_size",
                ok,
                format!(
                    "user asked for ~{:.1}×{:.1}×{:.1} {}, got {:.1}×{:.1}×{:.1} {}",
                    expected[0],
                    expected[1],
                    expected[2],
                    ctx.units.length_suffix(),
                    actual[0],
                    actual[1],
                    actual[2],
                    ctx.units.length_suffix()
                ),
            ));
        }

        if let Some(n) = parse_body_count_hint(user_message) {
            let visible = output
                .bodies
                .iter()
                .filter(|b| b.visible && !b.suppressed)
                .count();
            report.checks.push(check(
                "user_body_count",
                visible >= n,
                format!("user implied ≥{n} part(s), got {visible} visible bod(ies)"),
            ));
        }
    }

    report.passed = report.checks.iter().all(|c| c.passed);
    report
}

/// Verify a single-body program (tests / legacy `/api/run` with CadProgram).
pub fn verify_program(
    user_message: &str,
    program: &CadProgram,
    metrics: &MetricsData,
) -> VerificationReport {
    let doc = CadDocument::from_program(program.clone());
    let output = DocumentOutput {
        bodies: vec![crate::engine::BodyOutput {
            body_id: "body_main".into(),
            name: "Body".into(),
            visible: true,
            suppressed: false,
            mesh: crate::engine::MeshData {
                positions: vec![],
                normals: vec![],
                indices: vec![],
            },
            metrics: metrics.clone(),
        }],
        metrics: metrics.clone(),
    };
    verify_document(user_message, &doc, &output)
}

fn sort3(v: [f64; 3]) -> [f64; 3] {
    let mut a = v;
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    a
}

fn first_declared_box_size(document: &CadDocument) -> Option<[f64; 3]> {
    for body in &document.bodies {
        for feat in &body.features {
            if let Feature::Box(b) = feat {
                return Some(b.size);
            }
        }
    }
    None
}

fn count_consumed_tools(document: &CadDocument) -> usize {
    document
        .bodies
        .iter()
        .filter(|b| b.references.iter().any(|r| r.consume))
        .count()
}

/// Convert a length expressed in `from` units into document units.
fn to_document_units(value: f64, from: Units, document: &Units) -> f64 {
    let mm = from.linear_to_mm(value);
    document.linear_from_mm(mm)
}

/// Parse `80 x 40 x 10 mm` or `2 x 2 x 1 in` style triples from user text.
fn parse_size_triple(text: &str, document_units: &Units) -> Option<[f64; 3]> {
    let lower = text.to_ascii_lowercase().replace('×', "x");
    let unit_hint = if lower.contains("inch") || lower.contains('"') || lower.contains(" in ") {
        Units::Inch
    } else if lower.contains("mm") || lower.contains("millimeter") {
        Units::Mm
    } else {
        document_units.clone()
    };

    let bytes = lower.as_bytes();
    let mut found = None;
    let mut i = 0;
    while i < bytes.len() {
        let Some((a, mut j)) = parse_float_at(&lower, i) else {
            i += 1;
            continue;
        };
        j = skip_unit_suffix(bytes, skip_ws(bytes, j));
        j = skip_ws(bytes, j);
        if j >= bytes.len() || bytes[j] != b'x' {
            i += 1;
            continue;
        }
        j = skip_ws(bytes, j + 1);
        let Some((b, mut k)) = parse_float_at(&lower, j) else {
            i += 1;
            continue;
        };
        k = skip_unit_suffix(bytes, skip_ws(bytes, k));
        k = skip_ws(bytes, k);
        if k >= bytes.len() || bytes[k] != b'x' {
            i += 1;
            continue;
        }
        k = skip_ws(bytes, k + 1);
        let Some((c, mut end)) = parse_float_at(&lower, k) else {
            i += 1;
            continue;
        };
        end = skip_unit_suffix(bytes, end);
        found = Some([a, b, c]);
        i = end;
    }

    found.map(|[a, b, c]| {
        [
            to_document_units(a, unit_hint.clone(), document_units),
            to_document_units(b, unit_hint.clone(), document_units),
            to_document_units(c, unit_hint, document_units),
        ]
    })
}

fn skip_unit_suffix(bytes: &[u8], i: usize) -> usize {
    if i + 1 < bytes.len() && bytes[i] == b'm' && bytes[i + 1] == b'm' {
        return i + 2;
    }
    if i + 4 < bytes.len()
        && bytes[i] == b'i'
        && bytes[i + 1] == b'n'
        && bytes[i + 2] == b'c'
        && bytes[i + 3] == b'h'
        && bytes[i + 4] == b'e'
    {
        return i + 5;
    }
    if i + 10 < bytes.len() && &bytes[i..i + 11] == b"millimeter" {
        return i + 11;
    }
    if i + 10 < bytes.len() && &bytes[i..i + 11] == b"millimetre" {
        return i + 11;
    }
    if i < bytes.len() && bytes[i] == b'"' {
        return i + 1;
    }
    if i + 1 < bytes.len() && bytes[i] == b'i' && bytes[i + 1] == b'n' {
        return i + 2;
    }
    i
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn parse_float_at(s: &str, mut i: usize) -> Option<(f64, usize)> {
    let bytes = s.as_bytes();
    if i >= bytes.len() {
        return None;
    }
    if !(bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        return None;
    }
    let start = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    s[start..i].parse().ok().map(|v| (v, i))
}

fn parse_body_count_hint(text: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let words = ["bodies", "body", "parts", "part", "components", "component"];
    for w in words {
        if let Some(idx) = lower.find(w) {
            let prefix = &lower[..idx];
            if let Some(n) = last_integer(prefix) {
                if n >= 2 {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn last_integer(prefix: &str) -> Option<usize> {
    let mut nums = Vec::new();
    let mut i = 0;
    let bytes = prefix.as_bytes();
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if let Ok(n) = prefix[start..i].parse::<usize>() {
                nums.push(n);
            }
        } else {
            i += 1;
        }
    }
    nums.pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Engine;
    use crate::ir::*;
    use std::collections::BTreeMap;

    fn box_doc(w: f64, h: f64, d: f64, units: Units) -> CadDocument {
        CadDocument {
            document_id: "t".into(),
            units,
            parameters: BTreeMap::new(),
            bodies: vec![CadBody {
                body_id: "b".into(),
                name: "B".into(),
                visible: true,
                suppressed: false,
                transform: BodyTransform::default(),
                features: vec![Feature::Box(BoxOp {
                    size: [w, h, d],
                    at: [0.0; 3],
                    centered: true,
                })],
                references: vec![],
            }],
        }
    }

    #[test]
    fn structure_passes_for_simple_box() {
        let doc = box_doc(20.0, 10.0, 5.0, Units::Mm);
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_structure(&doc, &out);
        assert!(report.passed, "{}", report.summary());
    }

    #[test]
    fn user_size_triple_mm() {
        let doc = box_doc(80.0, 40.0, 10.0, Units::Mm);
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_document(
            "Make a plate 80 x 40 x 10 mm",
            &doc,
            &out,
        );
        assert!(report.passed, "{}", report.summary());
    }

    #[test]
    fn parameters_match_bbox() {
        let mut params = BTreeMap::new();
        params.insert("plate_width".into(), 80.0);
        params.insert("plate_depth".into(), 40.0);
        params.insert("plate_thickness".into(), 10.0);
        let doc = CadDocument {
            document_id: "t".into(),
            units: Units::Mm,
            parameters: params,
            bodies: vec![CadBody {
                body_id: "b".into(),
                name: "B".into(),
                visible: true,
                suppressed: false,
                transform: BodyTransform::default(),
                features: vec![Feature::Box(BoxOp {
                    size: [80.0, 40.0, 10.0],
                    at: [0.0; 3],
                    centered: true,
                })],
                references: vec![],
            }],
        };
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_document("", &doc, &out);
        assert!(report.passed, "{}", report.summary());
        assert!(report.checks.iter().any(|c| c.name == "param_plate_width"));
    }

    #[test]
    fn parameter_mismatch_fails() {
        let mut params = BTreeMap::new();
        params.insert("plate_width".into(), 100.0);
        let doc = CadDocument {
            document_id: "t".into(),
            units: Units::Mm,
            parameters: params,
            bodies: vec![CadBody {
                body_id: "b".into(),
                name: "B".into(),
                visible: true,
                suppressed: false,
                transform: BodyTransform::default(),
                features: vec![Feature::Box(BoxOp {
                    size: [80.0, 40.0, 10.0],
                    at: [0.0; 3],
                    centered: true,
                })],
                references: vec![],
            }],
        };
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_document("", &doc, &out);
        assert!(!report.passed);
        assert!(report.summary().contains("param_plate_width"));
    }

    #[test]
    fn user_size_triple_inches() {
        let doc = box_doc(2.0, 2.0, 1.0, Units::Inch);
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_document(
            "Block 2 x 2 x 1 inches",
            &doc,
            &out,
        );
        assert!(report.passed, "{}", report.summary());
    }

    #[test]
    fn user_size_mismatch_fails() {
        let doc = box_doc(50.0, 50.0, 10.0, Units::Mm);
        let out = Engine::default().execute_document(&doc).unwrap();
        let report = verify_document(
            "plate 80 x 40 x 10 mm",
            &doc,
            &out,
        );
        assert!(!report.passed);
        assert!(report.summary().contains("user_bbox_size"));
    }

    #[test]
    fn parse_triple_extracts_values() {
        let t = parse_size_triple("80mm x 40mm x 10mm", &Units::Mm).unwrap();
        assert!((t[0] - 80.0).abs() < 1e-6);
        assert!((t[2] - 10.0).abs() < 1e-6);
    }
}
