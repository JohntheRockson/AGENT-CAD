//! How the product agent should emit JSON IR.
//!
//! Golden example for the in-app agent — not a new Feature op. The kernel
//! already accepts `size: "M8"` with `diameter`/`pitch` null (ISO 261 coarse
//! Ø8 × 1.25). Across-flats is ISO 4014/4017 **13**, not head_width 10.
//!
//! Recipe owns emit + parse + keep-last-document. Helix/B-Rep/tessellation
//! honesty is Kernel. Do not "fix" tessellation crashes by teaching
//! thread-first in the prompt.

use kernel::ir::{CadDocument, Feature, FilletOp, Profile, ThreadKind, ThreadOp};

/// ISO M8 size table (ISO 261 coarse + ISO 4014/4017 hex).
pub const M8_MAJOR_DIAMETER: f64 = 8.0;
pub const M8_PITCH: f64 = 1.25;
pub const M8_ACROSS_FLATS: f64 = 13.0;
pub const M8_HEAD_HEIGHT: f64 = 5.3;
pub const M8_DEAD_HEIGHT: f64 = 8.0;

/// Golden M8 bolt document the product agent should emit.
///
/// Feature order (required default recipe):
/// 1. hex sketch + extrude (head, AF 13)
/// 2. overlapping cylinder (shank, unions into the head)
/// 3. under-head fillet (before thread — never `edges:"all"` after the helix)
/// 4. `thread` CUT (`kind: external`, `size: "M8"`, diameter/pitch unset)
///    starting after `dead_height` (unthreaded grip under the head)
/// 5. tip chamfer (`edges:"top"`)
///
/// Do not emit thread-first then fuse a hex/cylinder head as the default.
pub fn example_m8_bolt_json() -> serde_json::Value {
    serde_json::json!({
        "documentId": "m8_bolt",
        "units": "mm",
        "parameters": {
            "bolt_length": 40.0,
            "head_width": M8_ACROSS_FLATS,
            "head_height": M8_HEAD_HEIGHT,
            "dead_height": M8_DEAD_HEIGHT,
            "major_diameter": M8_MAJOR_DIAMETER,
            "pitch": M8_PITCH
        },
        "bodies": [{
            "bodyId": "body_m8_bolt",
            "name": "M8 Bolt",
            "visible": true,
            "features": [
                { "op": "sketch", "plane": "XY", "profile": { "hex": { "across_flats": "head_width" } } },
                { "op": "extrude", "depth": "head_height" },
                { "op": "cylinder", "diameter": "major_diameter",
                  "height": "bolt_length - head_height + 1",
                  "at": [0, 0, "head_height - 1"] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                { "op": "thread", "kind": "external", "size": "M8",
                  "length": "bolt_length - head_height - dead_height",
                  "at": [0, 0, "head_height + dead_height"] },
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ]
        }]
    })
}

/// Parse [`example_m8_bolt_json`] into a validated [`CadDocument`].
pub fn example_m8_bolt_document() -> CadDocument {
    let doc =
        CadDocument::from_json_value(example_m8_bolt_json()).expect("golden M8 bolt IR must parse");
    doc.validate().expect("golden M8 bolt IR must validate");
    doc
}

/// Last document to keep in chat/UI after a kernel failure.
///
/// Prefer the last IR that parsed this turn; otherwise keep the document the
/// client already had. Never replace a parsed document with `None` just
/// because the kernel rejected it.
pub fn keep_document_on_kernel_failure<'a>(
    last_parsed: Option<&'a CadDocument>,
    incoming: Option<&'a CadDocument>,
) -> Option<&'a CadDocument> {
    last_parsed.or(incoming)
}

/// Serialize a kept document for a chat `Result` event (`program` field).
pub fn program_json_for_chat(doc: Option<&CadDocument>) -> Option<serde_json::Value> {
    doc.and_then(|d| serde_json::to_value(d).ok())
}

/// Deterministic fastener-order judge used by verify/repair.
///
/// Returns `Some(reason)` when a hex-head / external-thread body is not
/// hex → overlapping cylinder → thread CUT, or when a fillet uses
/// `edges:"all"` after the thread (that rounds the helix).
///
/// Internal taps (plate + tap) are not bolts and are left alone.
pub fn fastener_recipe_violation(doc: &CadDocument) -> Option<String> {
    for body in &doc.bodies {
        if let Some(reason) = body_fastener_violation(body) {
            return Some(reason);
        }
    }
    None
}

fn body_fastener_violation(body: &kernel::ir::CadBody) -> Option<String> {
    let hex_i = body.features.iter().position(is_hex_head);
    let cyl_i = body
        .features
        .iter()
        .position(|f| matches!(f, Feature::Cylinder(_)));
    let thread_i = body.features.iter().position(is_external_thread);

    if let Some(t) = thread_i {
        for f in &body.features[t + 1..] {
            if let Feature::Fillet(FilletOp { edges, .. }) = f {
                if edges.is_all() {
                    return Some(
                        "fillet edges:\"all\" after thread rounds the helix; \
                         fillet under-head before thread, chamfer the tip with edges:\"top\""
                            .into(),
                    );
                }
            }
        }
    }

    let looks_like_bolt = body_name_is_bolt(&body.name) || hex_i.is_some();
    let Some(t) = thread_i else {
        return None;
    };
    if !looks_like_bolt {
        return None;
    }

    match (hex_i, cyl_i) {
        (Some(h), Some(c)) if h < c && c < t => None,
        (Some(h), Some(c)) if t < h || t < c => Some(
            "thread-first then fuse a head is rejected; \
             hex extrude → overlapping cylinder → thread CUT"
                .into(),
        ),
        _ => Some(
            "hex-head bolt must be hex extrude → overlapping cylinder → thread CUT \
             (not thread-first, not a missing shank)"
                .into(),
        ),
    }
}

fn is_hex_head(f: &Feature) -> bool {
    matches!(f, Feature::Sketch(op) if matches!(op.profile, Profile::Hex(_)))
}

fn is_external_thread(f: &Feature) -> bool {
    matches!(
        f,
        Feature::Thread(ThreadOp {
            kind: ThreadKind::External,
            ..
        })
    )
}

fn body_name_is_bolt(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("bolt") || n.contains("hex head") || n.contains("hex-head")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ir::{Feature, ThreadKind, ThreadOp};

    fn param(doc: &CadDocument, name: &str) -> f64 {
        *doc.parameters
            .get(name)
            .unwrap_or_else(|| panic!("missing parameter {name}"))
    }

    #[test]
    fn m8_bolt_is_hex_then_cylinder_then_thread_cut() {
        let doc = example_m8_bolt_document();
        assert!(
            !doc.parameters.is_empty(),
            "golden bolt must emit a parameters map"
        );
        let ops: Vec<&str> = doc.bodies[0]
            .features
            .iter()
            .map(Feature::op_name)
            .collect();
        assert!(
            ops.windows(3)
                .any(|w| w == ["sketch", "extrude", "cylinder"]),
            "expected hex sketch+extrude then cylinder, got {ops:?}"
        );
        let hex_pos = ops.iter().position(|o| *o == "sketch").unwrap();
        let cyl_pos = ops.iter().position(|o| *o == "cylinder").unwrap();
        let thread_pos = ops.iter().position(|o| *o == "thread").unwrap();
        assert!(
            hex_pos < cyl_pos && cyl_pos < thread_pos,
            "order must be hex then cylinder then thread, got {ops:?}"
        );
        match &doc.bodies[0].features[thread_pos] {
            Feature::Thread(ThreadOp {
                kind,
                size,
                diameter,
                pitch,
                ..
            }) => {
                assert_eq!(*kind, ThreadKind::External);
                assert_eq!(size.as_deref(), Some("M8"));
                assert_eq!(*diameter, None, "M8 must not force numeric diameter");
                assert_eq!(*pitch, None, "M8 must not force numeric pitch");
            }
            other => panic!("expected thread cut, got {other:?}"),
        }
        assert!(
            fastener_recipe_violation(&doc).is_none(),
            "golden recipe must pass fastener-order rules"
        );
    }

    #[test]
    fn m8_size_table_is_iso_af13_not_head_width_10() {
        let raw = example_m8_bolt_json();
        let p = &raw["parameters"];
        assert_eq!(p["head_width"], 13.0, "ISO 4014/4017 AF is 13, not 10");
        assert_eq!(p["major_diameter"], 8.0);
        assert_eq!(p["pitch"], 1.25);

        let doc = example_m8_bolt_document();
        assert!((param(&doc, "head_width") - M8_ACROSS_FLATS).abs() < 1e-9);
        assert!((param(&doc, "major_diameter") - M8_MAJOR_DIAMETER).abs() < 1e-9);
        assert!((param(&doc, "pitch") - M8_PITCH).abs() < 1e-9);
        match &doc.bodies[0].features[0] {
            Feature::Sketch(op) => match &op.profile {
                Profile::Hex(h) => assert!(
                    (h.across_flats - 13.0).abs() < 1e-9,
                    "hex AF should be 13, got {}",
                    h.across_flats
                ),
                other => panic!("expected hex, got {other:?}"),
            },
            other => panic!("expected sketch, got {other:?}"),
        }
    }

    #[test]
    fn m8_emits_unthreaded_grip() {
        let raw = example_m8_bolt_json();
        assert!(
            raw["parameters"].get("dead_height").is_some()
                || raw["parameters"].get("unthreaded_length").is_some(),
            "parameters must expose dead_height / unthreaded_length"
        );
        let thread = &raw["bodies"][0]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["op"] == "thread")
            .unwrap();
        let length = thread["length"].as_str().unwrap_or("");
        let at = format!("{}", thread["at"]);
        assert!(
            length.contains("dead_height") || length.contains("unthreaded"),
            "thread length must leave an unthreaded grip, got {length}"
        );
        assert!(
            at.contains("dead_height") || at.contains("unthreaded"),
            "thread at must start after the unthreaded grip, got {at}"
        );

        let doc = example_m8_bolt_document();
        let dead = param(&doc, "dead_height");
        assert!(dead > 0.0, "dead_height must be a positive grip");
        let thread = doc.bodies[0]
            .features
            .iter()
            .find(|f| matches!(f, Feature::Thread(_)))
            .unwrap();
        match thread {
            Feature::Thread(op) => {
                let head = param(&doc, "head_height");
                assert!(
                    (op.at[2] - (head + dead)).abs() < 1e-9,
                    "thread should start at head+dead, at.z={} head={head} dead={dead}",
                    op.at[2]
                );
                assert!(
                    (op.length - (param(&doc, "bolt_length") - head - dead)).abs() < 1e-9,
                    "thread length should be bolt - head - dead, got {}",
                    op.length
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn m8_finishing_ops_are_safe() {
        let raw = example_m8_bolt_json();
        let feats = raw["bodies"][0]["features"].as_array().unwrap();
        let ops: Vec<&str> = feats.iter().map(|f| f["op"].as_str().unwrap()).collect();
        let fillet_pos = ops
            .iter()
            .position(|o| *o == "fillet")
            .expect("under-head fillet");
        let thread_pos = ops.iter().position(|o| *o == "thread").expect("thread");
        let chamfer_pos = ops
            .iter()
            .position(|o| *o == "chamfer")
            .expect("tip chamfer");
        assert!(
            fillet_pos < thread_pos,
            "under-head fillet must come before thread"
        );
        assert_ne!(
            feats[fillet_pos]["edges"], "all",
            "do not fillet edges:all next to the helix"
        );
        assert_eq!(feats[chamfer_pos]["edges"], "top");

        let doc = example_m8_bolt_document();
        let mut saw_thread = false;
        for f in &doc.bodies[0].features {
            if matches!(f, Feature::Thread(_)) {
                saw_thread = true;
            }
            if saw_thread {
                if let Feature::Fillet(op) = f {
                    assert!(
                        !op.edges.is_all(),
                        "never emit fillet edges:all after thread"
                    );
                }
            }
        }
    }

    #[test]
    fn m8_thread_json_omits_or_nulls_diameter_pitch() {
        let raw = example_m8_bolt_json();
        let thread = raw["bodies"][0]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["op"] == "thread")
            .unwrap();
        assert_eq!(thread["size"], "M8");
        assert!(thread.get("diameter").is_none() || thread["diameter"].is_null());
        assert!(thread.get("pitch").is_none() || thread["pitch"].is_null());
    }

    #[test]
    fn kernel_failure_keeps_last_parsed_then_incoming() {
        let parsed = example_m8_bolt_document();
        let incoming = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_old",
                "features": [{ "op": "box", "size": [10, 10, 10], "centered": true }]
            }]
        }))
        .unwrap();

        let kept = keep_document_on_kernel_failure(Some(&parsed), Some(&incoming)).unwrap();
        assert_eq!(kept.document_id, "m8_bolt");

        let kept = keep_document_on_kernel_failure(None, Some(&incoming)).unwrap();
        assert_eq!(kept.bodies[0].body_id, "body_old");

        assert!(keep_document_on_kernel_failure(None, None).is_none());
        assert!(program_json_for_chat(Some(&parsed)).is_some());
        assert!(program_json_for_chat(None).is_none());
    }

    #[test]
    fn fastener_rules_reject_thread_first_and_fillet_all_after_thread() {
        let thread_first = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_bad",
                "name": "M8 Bolt",
                "features": [
                    { "op": "thread", "kind": "external", "size": "M8", "length": 24 },
                    { "op": "cylinder", "diameter": 13, "height": 5.3, "at": [0, 0, 24] },
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&thread_first).expect("thread-first must fail");
        assert!(
            reason.to_ascii_lowercase().contains("thread-first")
                || reason.to_ascii_lowercase().contains("hex extrude"),
            "reason should name the order bug: {reason}"
        );

        let fillet_all = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_bad",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 36, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8", "length": 30,
                      "at": [0, 0, 10] },
                    { "op": "fillet", "radius": 0.5, "edges": "all" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fillet_all).expect("fillet-all after thread");
        assert!(reason.contains("all"), "{reason}");

        let tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "plate",
                "features": [
                    { "op": "box", "size": [40, 40, 12], "centered": true },
                    { "op": "thread", "kind": "tap", "size": "M8", "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&tap).is_none(),
            "internal tap must not be judged as a hex-head bolt"
        );
    }
}
