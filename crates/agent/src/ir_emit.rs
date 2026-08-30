//! How the product agent should emit JSON IR.
//!
//! This is documentation and a golden example for the in-app agent — not a
//! new Feature op. The kernel already accepts `size: "M8"` with
//! `diameter`/`pitch` null (ISO 261 coarse Ø8 × 1.25).
//!
//! Recipe owns emit + parse + keep-last-document. Helix/B-Rep/tessellation
//! honesty (a real helical groove vs stacked ticks, wasm traps) is Kernel.
//!
//! Probe (2026-08-30, OCCT wasm, not a Recipe fix):
//! - thread-first `{ thread size:M8 }` builds (helical).
//! - hex extrude + overlapping cylinder builds.
//! - this golden hex→cylinder→thread CUT **tessellate-crashes**.
//! - cylinder then thread CUT fails (`revolve-ring fallback is disabled`).
//! Do not "fix" that by teaching thread-first in the prompt.

use kernel::ir::CadDocument;

/// Golden M8 bolt document the product agent should emit.
///
/// Feature order (required default recipe):
/// 1. hex sketch + extrude (head)
/// 2. overlapping cylinder (shank, unions into the head)
/// 3. `thread` CUT (`kind: external`, `size: "M8"`, diameter/pitch unset)
///
/// Do not emit thread-first then fuse a hex/cylinder head as the default.
pub fn example_m8_bolt_json() -> serde_json::Value {
    serde_json::json!({
        "documentId": "m8_bolt",
        "units": "mm",
        "parameters": {
            "head_width": 13.0,
            "head_height": 5.3,
            "bolt_length": 24.0
        },
        "bodies": [{
            "bodyId": "body_m8_bolt",
            "name": "M8 Bolt",
            "visible": true,
            "features": [
                { "op": "sketch", "plane": "XY", "profile": { "hex": { "across_flats": "head_width" } } },
                { "op": "extrude", "depth": "head_height" },
                { "op": "cylinder", "diameter": 8, "height": "bolt_length", "at": [0, 0, 3] },
                { "op": "thread", "kind": "external", "size": "M8", "length": 20, "at": [0, 0, 5.3] }
            ]
        }]
    })
}

/// Parse [`example_m8_bolt_json`] into a validated [`CadDocument`].
pub fn example_m8_bolt_document() -> CadDocument {
    let doc = CadDocument::from_json_value(example_m8_bolt_json())
        .expect("golden M8 bolt IR must parse");
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

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ir::{Feature, ThreadKind, ThreadOp};

    #[test]
    fn m8_bolt_is_hex_then_cylinder_then_thread_cut() {
        let doc = example_m8_bolt_document();
        let ops: Vec<&str> = doc.bodies[0]
            .features
            .iter()
            .map(Feature::op_name)
            .collect();
        assert_eq!(ops, ["sketch", "extrude", "cylinder", "thread"]);
        match &doc.bodies[0].features[3] {
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
    }

    #[test]
    fn m8_thread_json_omits_or_nulls_diameter_pitch() {
        let raw = example_m8_bolt_json();
        let thread = &raw["bodies"][0]["features"][3];
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
}
