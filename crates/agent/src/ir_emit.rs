//! How the product agent should emit JSON IR.
//!
//! Golden example for the in-app agent — not a new Feature op. The kernel
//! already accepts `size: "M8"` with `diameter`/`pitch` null (ISO 261 coarse
//! Ø8 × 1.25). Across-flats is ISO 4014/4017 **13**, not head_width 10.
//!
//! Recipe owns emit + parse + keep-last-document. Helix/B-Rep/tessellation
//! honesty is Kernel. Do not "fix" tessellation crashes by teaching
//! thread-first in the prompt.

use kernel::ir::{
    CadDocument, ChamferOp, EdgeSelection, Feature, FilletOp, LoftOp, Profile, SweepOp, SweepPath,
    ThreadKind, ThreadOp,
};

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

/// Last document to keep in chat/UI after a kernel / repair-loop failure.
///
/// Prefer the last IR that parsed this turn **and** passes
/// [`fastener_recipe_violation`]; otherwise keep the document the client
/// already had. Never replace a parsed document with `None` just because the
/// kernel rejected it. A recipe-breaking parse (thread-first, AF 10, …) must
/// not become leftover — Cycle 11 already refused those as verify fixes.
pub fn keep_document_on_kernel_failure<'a>(
    last_parsed: Option<&'a CadDocument>,
    incoming: Option<&'a CadDocument>,
) -> Option<&'a CadDocument> {
    match last_parsed {
        Some(d) if fastener_recipe_violation(d).is_none() && d.validate().is_ok() => Some(d),
        _ => incoming,
    }
}

/// Serialize a kept document for a chat `Result` event (`program` field).
pub fn program_json_for_chat(doc: Option<&CadDocument>) -> Option<serde_json::Value> {
    doc.and_then(|d| serde_json::to_value(d).ok())
}

/// Deterministic fastener-order judge used by verify/repair.
///
/// Returns `Some(reason)` when a hex-head / external-thread body is not
/// hex → overlapping cylinder → thread CUT, when a fillet after the
/// thread (any edges, including `all` / `longest` / tip) or a chamfer
/// uses `edges:"all"` after the thread (that wrecks the helix), when a
/// second external thread is present, when the
/// ISO size table does not actually drive the hex / unthreaded grip
/// (fully-threaded from the head, `head_width` ≠ hex AF, ISO M8 ≠ AF 13,
/// or `major_diameter` / ISO size token ≠ shank cylinder), when a body
/// named M8 (or an Ø8 shank) keeps the old AF 10 / Ø10 table with size
/// omitted, when helix/torus fakes a thread, when draft/thicken/common (like
/// shell/offset) follows the helix, when the helix
/// runs past the tip, or when the bolt is missing an under-head fillet
/// before thread or a tip chamfer.
///
/// Internal taps (plate + tap) are not bolts and are left alone.
/// A body named bolt *or* screw with tap/internal is rejected.
pub fn fastener_recipe_violation(doc: &CadDocument) -> Option<String> {
    for body in &doc.bodies {
        if let Some(reason) =
            body_fastener_violation(body, &doc.parameters, &doc.document_id)
        {
            return Some(reason);
        }
    }
    None
}

fn body_fastener_violation(
    body: &kernel::ir::CadBody,
    params: &std::collections::BTreeMap<String, f64>,
    doc_id: &str,
) -> Option<String> {
    let hex_i = body.features.iter().position(is_hex_head);
    let cyl_i = body
        .features
        .iter()
        .position(|f| matches!(f, Feature::Cylinder(_)));
    let thread_i = body.features.iter().position(is_external_thread);

    if let Some(t) = thread_i {
        for f in &body.features[t + 1..] {
            match f {
                Feature::Fillet(FilletOp { edges, .. }) if edges_all_or_longest(edges) => {
                    return Some(
                        "fillet edges:\"all\" or \"longest\" after thread rounds the helix; \
                         fillet under-head before thread, chamfer the tip with edges:\"top\""
                            .into(),
                    );
                }
                Feature::Fillet(_) => {
                    return Some(
                        "fillet after thread is not an under-head fillet; \
                         fillet before thread, chamfer the tip with edges:\"top\""
                            .into(),
                    );
                }
                Feature::Chamfer(ChamferOp { edges, .. }) if !edges_named(edges, "top") => {
                    return Some(
                        "chamfer after thread must use edges:\"top\"; \
                         edges:\"all\" or \"longest\" wreck the helix"
                            .into(),
                    );
                }
                Feature::Shell(_) | Feature::Offset(_) => {
                    return Some(
                        "shell or offset after thread wrecks the helix; \
                         chamfer the tip with edges:\"top\" only"
                            .into(),
                    );
                }
                Feature::Draft(_) | Feature::Thicken(_) => {
                    return Some(
                        "draft or thicken after thread wrecks the helix; \
                         chamfer the tip with edges:\"top\" only"
                            .into(),
                    );
                }
                Feature::Common(_) => {
                    return Some(
                        "common after thread wrecks the helix; \
                         chamfer the tip with edges:\"top\" only"
                            .into(),
                    );
                }
                other if is_fake_thread_feature(other) => {
                    return Some(
                        "do not fake threads with helix, torus, or revolve after a thread CUT; \
                         one external thread only"
                            .into(),
                    );
                }
                Feature::Pattern(_) => {
                    return Some(
                        "hex-head bolt must have one thread CUT; \
                         do not pattern after thread"
                            .into(),
                    );
                }
                _ => {}
            }
        }
    }

    let looks_like_bolt = body_name_is_bolt(&body.name) || hex_i.is_some();
    // Name-gated: hex-plate taps are not named "bolt" / "screw". A hex+tap
    // named "M8 hex cap screw" used to skip every recipe check
    // (thread_i is external-only; Cycle 23 only matched "bolt").
    if thread_i.is_none()
        && body_name_is_bolt(&body.name)
        && body.features.iter().any(is_internal_thread)
    {
        return Some(
            "hex-head bolt or screw must use external thread CUT, not tap/internal"
                .into(),
        );
    }
    // Prompt already forbids tori / helix fakes; the judge still shipped
    // hex→cyl→helix (no Thread op) named "Body" or "M8 Bolt".
    let has_thread_op = body
        .features
        .iter()
        .any(|f| matches!(f, Feature::Thread(_)));
    if !has_thread_op
        && body.features.iter().any(is_fake_thread_feature)
        && (body_name_is_bolt(&body.name) || (hex_i.is_some() && cyl_i.is_some()))
    {
        return Some(
            "do not fake threads with helix, torus, or revolve; use thread CUT \
             (kind external, size M8)"
                .into(),
        );
    }
    // Named bolt + hex + shank with no Thread op used to fall through
    // (Cycle 22 only caught tap). Cycle 32 required bolt/screw in the
    // name; a body named only "M8" / "M8x40" still shipped a blank shank.
    // Hex-plate taps stay unjudged (plate in the name, or an internal tap).
    if thread_i.is_none()
        && body_name_needs_thread_cut(&body.name)
        && hex_i.is_some()
        && cyl_i.is_some()
        && !body.features.iter().any(is_internal_thread)
    {
        return Some("hex-head bolt or screw must use external thread CUT".into());
    }
    let Some(t) = thread_i else {
        return None;
    };
    if !looks_like_bolt {
        return None;
    }
    // `.position` only sees the first helix. A legal first thread plus a
    // second CUT (from the head, past the tip, …) used to ship.
    if body
        .features
        .iter()
        .filter(|f| is_external_thread(f))
        .count()
        > 1
    {
        return Some(
            "hex-head bolt must have one thread CUT; \
             do not emit a second external thread"
                .into(),
        );
    }

    match (hex_i, cyl_i) {
        (Some(h), Some(c)) if h < c && c < t => {
            bolt_params_drive_hex_and_grip(body, params, h, c, t, doc_id)
        }
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

/// SW/Fusion mental model: the size table drives the feature tree.
/// `head_width` must match hex AF; `major_diameter` must match the shank;
/// thread start must leave `dead_height`. Explicit thread Ø/pitch must
/// match the size table when both are set (ISO `size:"M8"` may stay null).
/// When `major_diameter` is omitted, the ISO size token still drives the
/// shank (M8 → Ø8) — omitting the param is not a license to hard-code Ø10.
/// When `pitch` is omitted, an explicit `thread.pitch` must still match
/// the ISO token (M8 → 1.25).
/// After those checks, the helix must not run past the tip (`bolt_length`,
/// or the cylinder end when that param is omitted). Then require under-head
/// fillet before thread and a tip chamfer *after* thread (still reject
/// fillet-`all` / chamfer-`all` after the helix). A chamfer on the hex
/// before thread does not count as the tip.
fn bolt_params_drive_hex_and_grip(
    body: &kernel::ir::CadBody,
    params: &std::collections::BTreeMap<String, f64>,
    hex_i: usize,
    cyl_i: usize,
    thread_i: usize,
    doc_id: &str,
) -> Option<String> {
    let hex_af = hex_across_flats(&body.features[hex_i]);
    let cyl_d = match &body.features[cyl_i] {
        Feature::Cylinder(op) => Some(op.diameter),
        _ => None,
    };
    let head_from_feat = match &body.features[hex_i] {
        Feature::Fuse(op) => Some(op.depth),
        Feature::Common(op) => Some(op.depth),
        Feature::Loft(op) => loft_head_depth(op),
        Feature::Sweep(op) => sweep_head_depth(op),
        _ => body.features[hex_i + 1..thread_i]
            .iter()
            .find_map(|f| match f {
                Feature::Extrude(op) => Some(op.depth),
                Feature::DraftExtrude(op) => Some(op.depth),
                Feature::Sweep(op) => sweep_head_depth(op),
                Feature::Loft(op) => loft_head_depth(op),
                _ => None,
            }),
    };
    let thread = match &body.features[thread_i] {
        Feature::Thread(op) => op,
        _ => return None,
    };

    let head_width = first_param(params, &["head_width", "hex_width", "across_flats"]);
    if let (Some(hw), Some(af)) = (head_width, hex_af) {
        if (hw - af).abs() > 0.2 {
            return Some(
                "hex across_flats must be driven by head_width; \
                 do not hard-code a different wrench size than the size table"
                    .into(),
            );
        }
    }
    let iso_spec = thread_iso_spec(
        thread,
        params,
        &[&body.name, &body.body_id, doc_id],
        cyl_d,
    );
    // Cycle 1 only compared head_width to hex when the param was present.
    // size:"M8" + head_width 10 + AF 10 (the old table) still passed — a
    // consistent wrench-size lie. ISO 4014/4017 M8 is AF 13.
    // Cycle 15: omitting size and writing Ø8 × 1.25 is still M8 — not a
    // license to keep the old AF 10 table.
    if let Some(iso_af) = iso_hex_across_flats_for_spec(iso_spec.as_ref()) {
        if let Some(af) = hex_af {
            if (af - iso_af).abs() > 0.2 {
                return Some(
                    "hex across_flats must match the ISO size token; \
                     M8 is AF 13 — not 10, even if head_width is omitted or also 10"
                        .into(),
                );
            }
        }
        if let Some(hw) = head_width {
            if (hw - iso_af).abs() > 0.2 {
                return Some(
                    "head_width must match the ISO size token; \
                     M8 is AF 13 — do not keep the old AF 10 table next to size:\"M8\""
                        .into(),
                );
            }
        }
    }

    let major = first_param(
        params,
        &["major_diameter", "shank_diameter", "thread_diameter"],
    );
    let iso_major = iso_spec.as_ref().map(|spec| spec.major_diameter);
    let iso_pitch = iso_spec.as_ref().map(|spec| spec.pitch);
    if let (Some(md), Some(iso)) = (major, iso_major) {
        if (md - iso).abs() > 0.2 {
            return Some(
                "major_diameter must match the ISO size token; \
                 M8 is Ø8 — do not keep a size-table lie next to size:\"M8\""
                    .into(),
            );
        }
    }
    let expected_major = major.or(iso_major);
    if let (Some(md), Some(d)) = (expected_major, cyl_d) {
        if (md - d).abs() > 0.2 {
            return Some(
                "cylinder diameter must be driven by major_diameter \
                 (or the ISO size token when major_diameter is omitted); \
                 do not hard-code a different shank than the size table"
                    .into(),
            );
        }
    }
    if let (Some(md), Some(td)) = (expected_major, thread.diameter) {
        if (md - td).abs() > 0.2 {
            return Some(
                "thread diameter must match major_diameter when both are set; \
                 for M8 leave diameter/pitch null (ISO 261)"
                    .into(),
            );
        }
    }
    let pitch_param = first_param(params, &["pitch", "thread_pitch"]);
    if let (Some(p), Some(iso)) = (pitch_param, iso_pitch) {
        if (p - iso).abs() > 0.05 {
            return Some(
                "pitch must match the ISO size token; \
                 M8 is 1.25 — do not keep a size-table lie next to size:\"M8\""
                    .into(),
            );
        }
    }
    // When the pitch param is omitted, an explicit thread.pitch still has to
    // match the ISO token (M8 → 1.25). Both-set stays the existing check.
    if let (Some(p), Some(tp)) = (pitch_param.or(iso_pitch), thread.pitch) {
        if (p - tp).abs() > 0.05 {
            return Some(
                "thread pitch must match the size-table pitch \
                 (or the ISO size token when pitch is omitted); \
                 for M8 leave diameter/pitch null (ISO 261 coarse 1.25)"
                    .into(),
            );
        }
    }

    let head = first_param(params, &["head_height", "hex_height"]).or(head_from_feat);
    let dead = first_param(
        params,
        &[
            "dead_height",
            "dead_length",
            "unthreaded_length",
            "unthreaded_height",
        ],
    );
    let thread_z = thread.at[2];

    if let Some(h) = head {
        if let Some(d) = dead {
            if (thread_z - (h + d)).abs() > 0.51 {
                return Some(
                    "thread must start at head_height + dead_height \
                     so the unthreaded grip is parameter-driven"
                        .into(),
                );
            }
        } else if thread_z <= h + 0.51 {
            return Some(
                "hex-head bolt must leave an unthreaded grip (dead_height) under the head; \
                 do not fully-thread from the head"
                    .into(),
            );
        }
    }

    // Start can be legal (head + dead) while length still overshoots the tip.
    // Kernel auto-length (0) is 2×D — that can run past a short remaining shank.
    if let Some(reason) = thread_runs_past_tip(body, params, cyl_i, thread, thread_z, doc_id)
    {
        return Some(reason);
    }

    bolt_requires_underhead_fillet_and_tip_chamfer(body, thread_i)
}

/// Size-table overall length (or the shank cylinder end when `bolt_length`
/// is omitted) is the tip. Do not let the helix continue past it.
fn thread_runs_past_tip(
    body: &kernel::ir::CadBody,
    params: &std::collections::BTreeMap<String, f64>,
    cyl_i: usize,
    thread: &ThreadOp,
    thread_z: f64,
    doc_id: &str,
) -> Option<String> {
    let tip =
        first_param(params, &["bolt_length", "overall_length", "total_length"]).or_else(|| {
            match &body.features[cyl_i] {
                Feature::Cylinder(op) => {
                    let tip_z = op.at[2] + op.height;
                    (tip_z.is_finite() && tip_z > 0.0).then_some(tip_z)
                }
                _ => None,
            }
        })?;

    let shank_d = match &body.features[cyl_i] {
        Feature::Cylinder(op) => Some(op.diameter),
        _ => None,
    };
    let iso_spec = thread_iso_spec(
        thread,
        params,
        &[&body.name, &body.body_id, doc_id],
        shank_d,
    );
    let major = first_param(
        params,
        &["major_diameter", "shank_diameter", "thread_diameter"],
    )
    .or_else(|| iso_spec.as_ref().map(|spec| spec.major_diameter));
    let pitch = first_param(params, &["pitch", "thread_pitch"])
        .or(thread.pitch)
        .or_else(|| iso_spec.as_ref().map(|spec| spec.pitch));

    // Kernel `external_thread_length`: explicit length, else max(2×D, 4×pitch).
    let effective_len = if thread.length > 0.0 {
        thread.length
    } else {
        let d = major.unwrap_or(0.0);
        let p = pitch.unwrap_or(0.0);
        (d * 2.0).max(p * 4.0)
    };
    if effective_len > 0.0 && thread_z + effective_len > tip + 0.51 {
        return Some(
            "thread must not run past the tip; \
             length is bolt_length - head_height - dead_height"
                .into(),
        );
    }
    None
}

/// Golden recipe finishing: fillet the under-head junction *before* the
/// helix, then chamfer the tip. Fillet-`all` / chamfer-`all` after thread
/// are rejected earlier (they wreck the groove).
fn bolt_requires_underhead_fillet_and_tip_chamfer(
    body: &kernel::ir::CadBody,
    thread_i: usize,
) -> Option<String> {
    let fillet_before = body.features[..thread_i]
        .iter()
        .any(|f| matches!(f, Feature::Fillet(_)));
    if !fillet_before {
        return Some(
            "hex-head bolt must fillet under the head before thread; \
             never fillet edges:\"all\" after the helix"
                .into(),
        );
    }
    let has_tip_chamfer = body.features[thread_i + 1..].iter().any(|f| match f {
        Feature::Chamfer(op) => edges_named(&op.edges, "top"),
        _ => false,
    });
    if !has_tip_chamfer {
        return Some("hex-head bolt must chamfer the tip (edges:\"top\") after the thread".into());
    }
    None
}

/// ISO 4014/4017 hex across-flats for sizes the recipe already teaches.
/// M8 only — do not invent a full hex catalog here.
fn iso_hex_across_flats_for_spec(spec: Option<&kernel::thread::ThreadSpec>) -> Option<f64> {
    let spec = spec?;
    if (spec.major_diameter - M8_MAJOR_DIAMETER).abs() < 0.2 {
        Some(M8_ACROSS_FLATS)
    } else {
        None
    }
}

/// `size:"M8"` / `size:"M8x1.25"` parse to the same ISO 261 coarse spec.
/// When the token is omitted, numeric Ø8 (on the op, in the table, or
/// on the shank cylinder) is still M8 — a pitch lie must not unlock
/// the old AF 10 table.
/// A body named / id'd M8, or documentId M8 (not M80), is the same bind.
fn thread_iso_spec(
    thread: &ThreadOp,
    params: &std::collections::BTreeMap<String, f64>,
    name_hints: &[&str],
    shank_d: Option<f64>,
) -> Option<kernel::thread::ThreadSpec> {
    if let Some(spec) = thread
        .size
        .as_deref()
        .and_then(|s| kernel::thread::parse_size(s).ok())
    {
        return Some(spec);
    }
    if name_hints.iter().any(|s| text_implies_m8(s)) {
        return kernel::thread::parse_size("M8").ok();
    }
    let major = thread
        .diameter
        .or_else(|| {
            first_param(
                params,
                &["major_diameter", "shank_diameter", "thread_diameter"],
            )
        })
        .or(shank_d);
    match major {
        Some(d) if (d - M8_MAJOR_DIAMETER).abs() < 0.2 => kernel::thread::parse_size("M8").ok(),
        _ => None,
    }
}

/// `M8`, `M8x1.25`, `body_m8_bolt` — not `M80` or `arm8`.
fn text_implies_m8(s: &str) -> bool {
    let n = s.to_ascii_lowercase();
    let bytes = n.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'm' && bytes[i + 1] == b'8' {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after = n.get(i + 2..).unwrap_or("");
            let after_ok = after.is_empty()
                || after.starts_with('x')
                || after
                    .chars()
                    .next()
                    .is_some_and(|c| !c.is_ascii_digit());
            if prev_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn first_param(params: &std::collections::BTreeMap<String, f64>, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|n| {
        params
            .get(*n)
            .copied()
            .filter(|v| v.is_finite() && *v > 0.0)
    })
}

fn edges_named(edges: &EdgeSelection, name: &str) -> bool {
    matches!(edges, EdgeSelection::Named(s) if s.eq_ignore_ascii_case(name))
}

/// Kernel `is_all` is case-sensitive (`"all"` only). Gemini often emits
/// `"ALL"` / `"All"`; those still wreck the helix after thread.
fn edges_all_or_longest(edges: &EdgeSelection) -> bool {
    edges_named(edges, "all") || edges_named(edges, "longest")
}

fn hex_across_flats(f: &Feature) -> Option<f64> {
    match f {
        Feature::Sketch(op) => profile_across_flats(&op.profile),
        // Catalog fuse joins a boss. Models sometimes emit the hex head as
        // fuse instead of sketch+extrude; that still has to be ISO AF 13.
        Feature::Fuse(op) => profile_across_flats(&op.profile),
        Feature::Common(op) => profile_across_flats(&op.profile),
        Feature::Loft(op) => op
            .sections
            .iter()
            .find_map(|s| profile_across_flats(&s.profile)),
        Feature::Sweep(op) => op.profile.as_ref().and_then(profile_across_flats),
        _ => None,
    }
}

fn loft_head_depth(op: &LoftOp) -> Option<f64> {
    let zs: Vec<f64> = op.sections.iter().map(|s| s.at[2]).collect();
    let min = zs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let d = max - min;
    (d.is_finite() && d > 0.2).then_some(d)
}

fn sweep_head_depth(op: &SweepOp) -> Option<f64> {
    match &op.path {
        SweepPath::Polyline { points } if points.len() >= 2 => {
            let d = (points[points.len() - 1][2] - points[0][2]).abs();
            (d > 0.2).then_some(d)
        }
        SweepPath::Helix { height, .. } if *height > 0.2 => Some(*height),
        _ => None,
    }
}

/// Hex AF from a profile, including a `hex` wrapped as `compound.outer`
/// and a catalog polyline that is a regular 6-gon (hard-coded hex points).
/// Cycle 21 caught fuse+hex; a Body-named polyline / compound hex still
/// skipped `is_hex_head`, so size M8 + AF 10 shipped.
fn profile_across_flats(profile: &Profile) -> Option<f64> {
    match profile {
        Profile::Hex(h) => Some(h.across_flats),
        Profile::Compound(c) => profile_across_flats(&c.outer),
        Profile::Polyline(p) => polyline_regular_hex_af(&p.points),
        _ => None,
    }
}

/// Regular hexagon → across-flats. `hex_vertices` uses R = AF / √3, so
/// AF = R × √3. Drop a duplicated close point. Irregular 6-gons stay None
/// so a wishbone outline is not judged as a hex head.
fn polyline_regular_hex_af(points: &[[f64; 2]]) -> Option<f64> {
    let mut pts = points.to_vec();
    if pts.len() >= 2 {
        let a = pts[0];
        let b = pts[pts.len() - 1];
        if (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6 {
            pts.pop();
        }
    }
    if pts.len() != 6 {
        return None;
    }
    let cx = pts.iter().map(|p| p[0]).sum::<f64>() / 6.0;
    let cy = pts.iter().map(|p| p[1]).sum::<f64>() / 6.0;
    let rs: Vec<f64> = pts
        .iter()
        .map(|p| ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt())
        .collect();
    let r = rs.iter().sum::<f64>() / 6.0;
    if !r.is_finite() || r < 0.2 {
        return None;
    }
    if rs.iter().any(|ri| (ri - r).abs() > 0.2) {
        return None;
    }
    for i in 0..6 {
        let a = pts[i];
        let b = pts[(i + 1) % 6];
        let side = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        if (side - r).abs() > 0.2 {
            return None;
        }
    }
    Some(r * 3.0_f64.sqrt())
}

fn is_hex_head(f: &Feature) -> bool {
    hex_across_flats(f).is_some()
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

fn is_internal_thread(f: &Feature) -> bool {
    matches!(
        f,
        Feature::Thread(ThreadOp {
            kind: ThreadKind::Internal,
            ..
        })
    )
}

fn body_name_is_bolt(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("bolt")
        || n.contains("hex head")
        || n.contains("hex-head")
        || n.contains("screw")
}

/// Cycle 32 required bolt/screw. A body named only M8 / M8x40 with hex+shank
/// is the same blank-shank skip. Do not treat "M8 hex plate" as a bolt.
fn body_name_needs_thread_cut(name: &str) -> bool {
    if body_name_is_bolt(name) {
        return true;
    }
    let n = name.to_ascii_lowercase();
    if n.contains("plate") || n.contains("sheet") || n.contains("bracket") {
        return false;
    }
    text_implies_m8(name)
}

fn is_fake_thread_feature(f: &Feature) -> bool {
    match f {
        Feature::Helix(_) | Feature::Torus(_) | Feature::Revolve(_) => true,
        Feature::Pipe(op) => matches!(op.path, kernel::ir::SweepPath::Helix { .. }),
        Feature::Sweep(op) => matches!(op.path, kernel::ir::SweepPath::Helix { .. }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ir::{hex_vertices, Feature, ThreadKind, ThreadOp};

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

        // Cycle 11 stopped a recipe-breaking verify *fix* from replacing
        // last_document. The main parse path still overwrote leftover with
        // thread-first / AF-10 IR; exhaust then shipped that as success-shaped
        // program JSON. Fall back to the incoming document instead.
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
        assert!(
            fastener_recipe_violation(&thread_first).is_some(),
            "fixture must be a recipe violation"
        );
        let kept = keep_document_on_kernel_failure(Some(&thread_first), Some(&incoming)).unwrap();
        assert_eq!(
            kept.bodies[0].body_id, "body_old",
            "recipe-breaking last_parsed must not become leftover"
        );
        assert!(
            keep_document_on_kernel_failure(Some(&thread_first), None).is_none(),
            "recipe-breaking last_parsed with no incoming must not be kept"
        );

        let tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "plate",
                "features": [
                    { "op": "box", "size": [40, 40, 12], "centered": true },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        let kept = keep_document_on_kernel_failure(Some(&tap), Some(&incoming)).unwrap();
        assert_eq!(
            kept.bodies[0].body_id, "body_plate",
            "internal tap must still be a keepable last_parsed"
        );

        // Recipe-ok but invalid (tap missing size) must not become leftover.
        let invalid_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_invalid",
                "name": "plate",
                "features": [
                    { "op": "box", "size": [40, 40, 12], "centered": true },
                    { "op": "thread", "kind": "tap", "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&invalid_tap).is_none(),
            "unsized tap is not a bolt recipe violation"
        );
        assert!(
            invalid_tap.validate().is_err(),
            "unsized tap must fail document.validate"
        );
        let kept = keep_document_on_kernel_failure(Some(&invalid_tap), Some(&incoming)).unwrap();
        assert_eq!(
            kept.bodies[0].body_id, "body_old",
            "invalid last_parsed must not become leftover"
        );
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

        // Cycle 3 requires a chamfer; EdgeSelection defaults to "all", so a
        // bare chamfer after thread wrecks the helix the same way fillet-all does.
        let chamfer_all = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5 }
                ]
            }]
        }))
        .unwrap();
        let reason =
            fastener_recipe_violation(&chamfer_all).expect("chamfer-all after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && l.contains("all"),
            "reason should name chamfer-all after thread: {reason}"
        );

        // Kernel is_all() is `"all"` only. edges:"ALL" still cuts the helix.
        let chamfer_all_caps = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "ALL" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&chamfer_all_caps)
            .expect("chamfer edges:ALL after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && l.contains("all"),
            "reason should name chamfer-all after thread: {reason}"
        );

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

    /// Inspector golden / pre-#18 emit: hex→cyl→thread with no dead_height.
    /// Order is legal; the size table does not drive an unthreaded grip.
    #[test]
    fn fastener_rules_reject_fully_threaded_and_undriven_params() {
        let fully_threaded = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8", "length": 34.7,
                      "at": [0, 0, 5.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fully_threaded)
            .expect("fully-threaded inspector-golden bolt must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("unthreaded") || l.contains("dead_height") || l.contains("grip"),
            "reason should name the missing grip: {reason}"
        );

        let undriven_hex = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&undriven_hex)
            .expect("head_width 13 with hex AF 10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("head_width") || l.contains("across_flats") || l.contains("wrench"),
            "reason should name the undriven hex: {reason}"
        );

        // Omit bolt_length so kernel bind_independent_bolt_dims cannot rewrite
        // thread start; the judge must still see the undriven grip.
        let undriven_grip = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 34.7, "at": [0, 0, 5.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&undriven_grip)
            .expect("dead_height 8 with thread at the head must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("dead_height") || l.contains("parameter"),
            "reason should name the undriven grip: {reason}"
        );

        // Size table Ø8 while the shank is Ø10 — same class of lie as AF 10.
        let undriven_shank = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 10, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&undriven_shank)
            .expect("major_diameter 8 with cylinder Ø10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("major_diameter") || l.contains("cylinder") || l.contains("shank"),
            "reason should name the undriven shank: {reason}"
        );

        // Explicit thread pitch that fights the size table (ISO M8 stays null).
        let undriven_pitch = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "pitch": 2.0, "length": 26.7, "at": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&undriven_pitch)
            .expect("pitch 1.25 with thread pitch 2 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("pitch"),
            "reason should name the undriven pitch: {reason}"
        );
    }

    /// Order + size table can be legal and still omit the golden finishing
    /// ops. Verify must require under-head fillet before thread and a tip
    /// chamfer (fillet-all after thread stays rejected above).
    #[test]
    fn fastener_rules_require_underhead_fillet_and_tip_chamfer() {
        let legal_params = serde_json::json!({
            "bolt_length": 40.0,
            "head_height": 5.3,
            "head_width": 13.0,
            "dead_height": 8.0,
            "major_diameter": 8.0,
            "pitch": 1.25
        });
        let hex_cyl_thread = serde_json::json!([
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 13 } } },
            { "op": "extrude", "depth": 5.3 },
            { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
            { "op": "thread", "kind": "external", "size": "M8",
              "length": 26.7, "at": [0, 0, 13.3] }
        ]);

        let missing_both = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": legal_params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_thread
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&missing_both)
            .expect("legal order without fillet/chamfer must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("fillet") && (l.contains("before") || l.contains("under")),
            "reason should require under-head fillet: {reason}"
        );

        let fillet_only = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason =
            fastener_recipe_violation(&fillet_only).expect("fillet without tip chamfer must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && l.contains("tip"),
            "reason should require tip chamfer: {reason}"
        );

        let chamfer_only = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&chamfer_only)
            .expect("chamfer without under-head fillet must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("fillet") && (l.contains("before") || l.contains("under")),
            "reason should require under-head fillet: {reason}"
        );

        let fillet_after_not_all = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fillet_after_not_all)
            .expect("fillet after thread is not an under-head fillet");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("fillet") && (l.contains("before") || l.contains("under")),
            "fillet after thread must not satisfy under-head: {reason}"
        );

        // A chamfer on the hex before thread is not a tip chamfer.
        let chamfer_before = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&chamfer_before)
            .expect("chamfer before thread is not a tip chamfer");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && (l.contains("tip") || l.contains("after")),
            "reason should require tip chamfer after thread: {reason}"
        );

        assert!(
            fastener_recipe_violation(&example_m8_bolt_document()).is_none(),
            "golden hex→fillet→thread→chamfer must still pass"
        );
    }

    /// Cycle 2 only compared major_diameter when the param was present.
    /// Omitting it and hard-coding a Ø10 shank next to size:"M8" still
    /// passed — a size-table lie by null. ISO token must drive the shank.
    #[test]
    fn fastener_rules_iso_size_drives_shank_when_major_omitted() {
        let hex_cyl_finish = |cyl_d: f64| {
            serde_json::json!([
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": 13 } } },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": cyl_d, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                { "op": "thread", "kind": "external", "size": "M8",
                  "length": 26.7, "at": [0, 0, 13.3] },
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let omitted_wrong = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(10.0)
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&omitted_wrong)
            .expect("size M8 with omitted major_diameter and Ø10 shank must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("major_diameter") || l.contains("cylinder") || l.contains("iso"),
            "reason should name the undriven / ISO shank: {reason}"
        );

        let omitted_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(8.0)
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&omitted_ok).is_none(),
            "size M8 with omitted major_diameter and Ø8 shank must still pass"
        );

        let table_lie = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 10.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(10.0)
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&table_lie)
            .expect("size M8 with major_diameter 10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("major_diameter") || l.contains("iso") || l.contains("m8"),
            "reason should name the token/table conflict: {reason}"
        );
    }

    /// Cycle 2 only compared thread.pitch to the size-table pitch when the
    /// param was present. Omitting it and hard-coding thread.pitch 2.0 next
    /// to size:"M8" still passed — a size-table lie by null. ISO token
    /// (M8 → 1.25) must drive an explicit thread.pitch.
    #[test]
    fn fastener_rules_iso_size_drives_explicit_pitch_when_param_omitted() {
        let hex_cyl_finish = |thread_pitch: Option<f64>| {
            let mut thread = serde_json::json!({
                "op": "thread", "kind": "external", "size": "M8",
                "length": 26.7, "at": [0, 0, 13.3]
            });
            if let Some(p) = thread_pitch {
                thread["pitch"] = serde_json::json!(p);
            }
            serde_json::json!([
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": 13 } } },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                thread,
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let omitted_wrong = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(Some(2.0))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&omitted_wrong)
            .expect("omitted pitch + M8 + thread.pitch 2.0 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("pitch")
                && (l.contains("iso") || l.contains("omitted") || l.contains("1.25")),
            "reason should name the undriven / ISO pitch: {reason}"
        );

        let omitted_null = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(None)
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&omitted_null).is_none(),
            "size M8 with omitted pitch param and null thread.pitch must still pass"
        );

        let omitted_matches = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(Some(1.25))
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&omitted_matches).is_none(),
            "explicit thread.pitch 1.25 next to M8 with omitted pitch param must pass"
        );

        // Cycle 4 analog: pitch param 2.0 next to size M8 with null thread.pitch.
        let table_lie = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 2.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(None)
            }]
        }))
        .unwrap();
        let reason =
            fastener_recipe_violation(&table_lie).expect("size M8 with pitch param 2.0 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("pitch") && (l.contains("iso") || l.contains("1.25") || l.contains("m8")),
            "reason should name the token/table pitch lie: {reason}"
        );
    }

    /// Cycle 1 only compared head_width to hex when both were set. The old
    /// AF 10 table still passed if the param and the sketch agreed, and an
    /// omitted head_width + hard-coded AF 10 next to size:"M8" also passed.
    /// ISO 4014/4017 M8 is AF 13.
    #[test]
    fn fastener_rules_iso_m8_drives_hex_af_when_head_width_omitted_or_10() {
        let hex_cyl_finish = |af: f64| {
            serde_json::json!([
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": af } } },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                { "op": "thread", "kind": "external", "size": "M8",
                  "length": 26.7, "at": [0, 0, 13.3] },
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let old_table = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 10.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(10.0)
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&old_table)
            .expect("size M8 with head_width 10 and AF 10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the AF 10 / ISO 13 lie: {reason}"
        );

        let omitted_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(10.0)
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&omitted_af10)
            .expect("omitted head_width + M8 + AF 10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso"),
            "reason should name the omitted-head_width / ISO AF lie: {reason}"
        );

        let omitted_af13 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(13.0)
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&omitted_af13).is_none(),
            "size M8 with omitted head_width and AF 13 must still pass"
        );
    }

    /// Cycle 1–9 check thread *start*. Kernel bind clamps length when
    /// `bolt_length` is present — omit it (same trick as undriven grip)
    /// and a legal start with length 50 still ran past the cylinder tip.
    #[test]
    fn fastener_rules_reject_thread_past_tip() {
        let hex_cyl_finish = |thread_len: f64| {
            CadDocument::from_json_value(serde_json::json!({
                "units": "mm",
                "parameters": {
                    "head_height": 5.3,
                    "head_width": 13.0,
                    "dead_height": 8.0,
                    "major_diameter": 8.0,
                    "pitch": 1.25
                },
                "bodies": [{
                    "bodyId": "body_m8_bolt",
                    "name": "M8 Bolt",
                    "features": [
                        { "op": "sketch", "plane": "XY",
                          "profile": { "hex": { "across_flats": 13 } } },
                        { "op": "extrude", "depth": 5.3 },
                        { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                        { "op": "fillet", "radius": 0.4, "edges": "longest" },
                        { "op": "thread", "kind": "external", "size": "M8",
                          "length": thread_len, "at": [0, 0, 13.3] },
                        { "op": "chamfer", "distance": 0.5, "edges": "top" }
                    ]
                }]
            }))
            .unwrap()
        };

        let past = hex_cyl_finish(50.0);
        let reason = fastener_recipe_violation(&past)
            .expect("omitted bolt_length + length 50 past the cylinder tip must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("tip") && (l.contains("past") || l.contains("length")),
            "reason should name thread past the tip: {reason}"
        );

        let ok = hex_cyl_finish(26.7);
        assert!(
            fastener_recipe_violation(&ok).is_none(),
            "omitted bolt_length with thread ending at the cylinder tip must still pass"
        );

        // bind no-ops without head_height, so bolt_length 40 cannot clamp length 50.
        let no_head_param = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 50.0, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&no_head_param)
            .expect("omitted head_height + length 50 past bolt_length must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("tip") || l.contains("length"),
            "reason should name the overshoot when bind cannot clamp: {reason}"
        );

        // Kernel auto-length (0) is 2×D = 16. Omit bolt_length so bind
        // cannot rewrite; short cylinder tip is 14.3.
        let auto_overshoot = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 10.0, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 0, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&auto_overshoot)
            .expect("auto thread length 2×D must not run past a short cylinder tip");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("tip") || l.contains("length"),
            "reason should name auto-length past the tip: {reason}"
        );

        assert!(
            fastener_recipe_violation(&example_m8_bolt_document()).is_none(),
            "golden thread ending at the tip must still pass"
        );

        // Cycle 5 rejected chamfer-all; edges:"longest" after thread still
        // counted as a tip chamfer and can cut the helix (the under-head
        // fillet teaches "longest", so models copy it onto the tip).
        let chamfer_longest = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "longest" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&chamfer_longest)
            .expect("chamfer-longest after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && (l.contains("longest") || l.contains("top")),
            "reason should name chamfer-longest / require edges:top: {reason}"
        );

        // Hexagonal plate + internal tap is not a bolt (no past-tip / recipe judge).
        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "internal tap on a hex plate must not be judged as a hex-head bolt"
        );
    }

    /// Cycle 8 bound AF 13 to size:"M8". size:"M8x1.25" is the same ISO 261
    /// coarse token and already failed AF 10. Omitting size and writing
    /// diameter 8 / pitch 1.25 still skipped the ISO AF bind — the old
    /// AF 10 table shipped. Numeric Ø8×1.25 is still M8.
    #[test]
    fn fastener_rules_omitted_size_token_still_drives_m8_af13() {
        let hex_cyl_finish = |size: Option<&str>, af: f64| {
            let mut thread = serde_json::json!({
                "op": "thread", "kind": "external",
                "length": 26.7, "at": [0, 0, 13.3]
            });
            if let Some(s) = size {
                thread["size"] = serde_json::json!(s);
            } else {
                thread["diameter"] = serde_json::json!(8.0);
                thread["pitch"] = serde_json::json!(1.25);
            }
            serde_json::json!([
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": af } } },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                thread,
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let omitted_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 10.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(None, 10.0)
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&omitted_af10)
            .expect("omitted size + Ø8×1.25 + AF 10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the omitted-token / AF 10 lie: {reason}"
        );

        let omitted_af13 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(None, 13.0)
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&omitted_af13).is_none(),
            "omitted size + Ø8×1.25 + AF 13 must still pass"
        );

        let m8x125_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0,
                "pitch": 1.25
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish(Some("M8x1.25"), 13.0)
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&m8x125_ok).is_none(),
            "size M8x1.25 is the same ISO coarse token as M8"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8x1.25",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "internal tap on a hex plate (M8x1.25) must not be judged as a bolt"
        );
    }

    /// Cycle 25 VERIFY already said an Ø8 shank is M8 AF 13. The judge only
    /// read thread.diameter / major_diameter / the name — a Body-named hex
    /// with a Ø8 cylinder, omitted size, and AF 10 still shipped.
    #[test]
    fn fastener_rules_shank_cylinder_o8_still_drives_m8_af13() {
        let shank_only_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&shank_only_af10)
            .expect("Ø8 shank + AF 10 with size/name omitted must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the Ø8-shank / AF 10 lie: {reason}"
        );

        // Size omitted + diameter/pitch on the op so it is a real M8 CUT.
        let shank_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&shank_ok).is_none(),
            "Ø8 shank + AF 13 with size omitted must still pass"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "cylinder", "diameter": 8, "height": 4, "at": [0, 0, 12] },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "hex-plate tap with an Ø8 boss must stay unjudged"
        );
    }

    /// Cycle 20 fuzz: first-thread-only judge, fillet-after-tip, expression
    /// AF lie, empty parameters. Hex-plate taps stay unjudged.
    #[test]
    fn fastener_rules_reject_second_thread_fillet_after_tip_and_af_expr_lie() {
        let hex_cyl_finish = serde_json::json!([
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 13 } } },
            { "op": "extrude", "depth": 5.3 },
            { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
            { "op": "fillet", "radius": 0.4, "edges": "longest" },
            { "op": "thread", "kind": "external", "size": "M8",
              "length": 26.7, "at": [0, 0, 13.3] },
            { "op": "chamfer", "distance": 0.5, "edges": "top" }
        ]);

        let double_thread = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 34.7, "at": [0, 0, 5.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&double_thread)
            .expect("second external thread after a legal first CUT must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("second") || l.contains("one thread"),
            "reason should name the extra thread: {reason}"
        );

        let fillet_after_tip = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "fillet", "radius": 0.3, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fillet_after_tip)
            .expect("fillet edges:top after the tip chamfer must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("fillet") && l.contains("after thread"),
            "reason should name fillet after thread: {reason}"
        );

        let af_expr_lie = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": "major_diameter" } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&af_expr_lie)
            .expect("head_width 13 with across_flats bound to major_diameter 8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("head_width") || l.contains("across_flats") || l.contains("13"),
            "reason should name the AF expression lie: {reason}"
        );

        let empty_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            empty_af10.parameters.is_empty(),
            "fixture is an empty parameters map"
        );
        let reason = fastener_recipe_violation(&empty_af10)
            .expect("empty parameters + size M8 + AF 10 must still fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso"),
            "empty parameters must not skip the ISO AF bind: {reason}"
        );

        let empty_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": hex_cyl_finish
            }]
        }))
        .unwrap();
        assert!(empty_ok.parameters.is_empty());
        assert!(
            fastener_recipe_violation(&empty_ok).is_none(),
            "empty parameters with ISO-correct literals must still pass"
        );

        let chamfer_before_cyl = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "chamfer", "distance": 0.4, "edges": "top" },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&chamfer_before_cyl).is_none(),
            "extra hex chamfer before the cylinder is not a missing tip chamfer"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "internal tap on a hex plate must not become a bolt"
        );

        // Fuse-hex head (catalog boss) used to skip is_hex_head, so a Body
        // named AF-10 M8 shipped. Kernel already treats fuse hex as a hex.
        let fuse_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "fuse", "profile": { "hex": { "across_flats": 10 } },
                      "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fuse_af10)
            .expect("fuse-hex AF 10 next to size M8 must fail even when named Body");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the fuse-hex AF lie: {reason}"
        );

        let fuse_af13 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "fuse", "profile": { "hex": { "across_flats": 13 } },
                      "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&fuse_af13).is_none(),
            "fuse-hex AF 13 with the golden shank/thread must still pass"
        );

        let named_bolt_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&named_bolt_tap)
            .expect("a body named bolt with tap/internal must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("external") && (l.contains("tap") || l.contains("internal")),
            "reason should require external CUT, not tap: {reason}"
        );

        let chamfer_bottom_then_top = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.4, "edges": "bottom" },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&chamfer_bottom_then_top)
            .expect("non-top chamfer after thread must fail even if a later tip chamfer exists");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("chamfer") && (l.contains("top") || l.contains("after thread")),
            "reason should require chamfer edges:top after thread: {reason}"
        );

        // draft_extrude is in the catalog. Omitting head_height used to skip
        // the grip check because head_from_feat only read Extrude.
        let draft_fully_threaded = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "draft_extrude", "depth": 5.3, "angle": 1 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 34.7, "at": [0, 0, 5.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&draft_fully_threaded)
            .expect("draft_extrude head + thread at the head must fail without head_height");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("unthreaded") || l.contains("dead_height") || l.contains("grip"),
            "reason should name the missing grip: {reason}"
        );

        let draft_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "draft_extrude", "depth": 5.3, "angle": 1 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&draft_ok).is_none(),
            "draft_extrude head with a parameter-driven grip must still pass"
        );
    }

    /// Cycle 25: name-gated tap missed "screw"; omitting size + AF 10 + Ø10
    /// next to a body named M8 skipped the ISO bind; Ø8 + a pitch lie
    /// unlocked AF 10; helix/torus faked a thread. Hex-plate taps stay.
    #[test]
    fn fastener_rules_named_screw_m8_name_and_fake_thread() {
        assert!(text_implies_m8("M8 Bolt"));
        assert!(text_implies_m8("body_m8_bolt"));
        assert!(text_implies_m8("M8x1.25 cap"));
        assert!(!text_implies_m8("M80 Bolt"));
        assert!(!text_implies_m8("arm8"));

        let hex_cyl_finish = |thread: serde_json::Value| {
            serde_json::json!([
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": 13 } } },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                thread,
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let named_screw_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 hex cap screw",
                "features": hex_cyl_finish(serde_json::json!({
                    "op": "thread", "kind": "tap", "size": "M8",
                    "center": [0, 0], "through": true
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&named_screw_tap)
            .expect("a body named screw with tap/internal must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("external") && (l.contains("tap") || l.contains("internal")) && l.contains("screw"),
            "reason should require external CUT, not tap, on a named screw: {reason}"
        );

        let named_m8_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 10, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&named_m8_af10)
            .expect("named M8 + omitted size + AF 10 + Ø10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("8"),
            "reason should name the named-M8 / AF 10 / Ø10 lie: {reason}"
        );

        let pitch_lie_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 10.0,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external",
                      "diameter": 8.0, "pitch": 2.0,
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&pitch_lie_af10)
            .expect("Ø8 + pitch 2.0 must still be M8 — not a license for AF 10");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("pitch") || l.contains("1.25"),
            "reason should name the Ø8 / pitch-lie / AF 10 escape: {reason}"
        );

        let helix_fake = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "helix", "pitch": 1.25, "height": 26.7, "radius": 4,
                      "diameter": 0.8, "center": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&helix_fake)
            .expect("hex→cyl→helix with no Thread op must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("helix") || l.contains("torus") || l.contains("fake"),
            "reason should name the helix/torus fake: {reason}"
        );

        let torus_fake = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "torus", "major": 4, "minor": 0.6, "at": [0, 0, 20] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&torus_fake)
            .expect("named bolt + torus rings with no Thread op must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("torus") || l.contains("helix") || l.contains("fake"),
            "reason should name the torus fake: {reason}"
        );

        let revolve_fake = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "revolve", "axis": "Z", "angle": 360 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&revolve_fake)
            .expect("hex→cyl→revolve with no Thread op must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("revolve") || l.contains("fake") || l.contains("helix"),
            "reason should name the revolve fake: {reason}"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "internal tap on a hex plate must not become a bolt"
        );

        let spring = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_spring",
                "name": "coilover spring",
                "features": [
                    { "op": "helix", "pitch": 8, "height": 40, "radius": 12,
                      "diameter": 3, "center": [0, 0, 0] }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&spring).is_none(),
            "a helix spring that is not a hex-head bolt must stay unjudged"
        );

        let lathe = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_tube",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XZ",
                      "profile": { "polyline": { "points": [[8,0],[12,0],[12,20],[8,20]],
                                                 "closed": true } } },
                    { "op": "revolve", "axis": "Z", "angle": 360 }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&lathe).is_none(),
            "a lathe/revolve tube that is not a hex-head bolt must stay unjudged"
        );

        assert!(
            fastener_recipe_violation(&example_m8_bolt_document()).is_none(),
            "golden recipe must still pass"
        );

        // Cycle 26: leftover IR often keeps documentId m8_bolt while the
        // body is named Body / body_main — that used to skip the ISO bind.
        let docid_m8_af10 = CadDocument::from_json_value(serde_json::json!({
            "documentId": "m8_bolt",
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 10 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 10, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&docid_m8_af10)
            .expect("documentId m8_bolt + Body + AF 10 + Ø10 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("8"),
            "reason should name the documentId-M8 / AF 10 lie: {reason}"
        );

        let named_screw_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 hex cap screw",
                "features": hex_cyl_finish(serde_json::json!({
                    "op": "thread", "kind": "external", "size": "M8",
                    "length": 26.7, "at": [0, 0, 13.3]
                }))
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&named_screw_ok).is_none(),
            "a named screw with the golden external recipe must still pass"
        );

        let pipe_helix_fake = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "pipe", "diameter": 0.8,
                      "path": { "helix": { "pitch": 1.25, "height": 26.7,
                        "radius": 4, "center": [0, 0, 13.3] } } }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&pipe_helix_fake)
            .expect("hex→cyl→pipe-helix with no Thread op must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("helix") || l.contains("torus") || l.contains("fake"),
            "reason should name the pipe-helix fake: {reason}"
        );

        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "hex-plate tap must still pass after documentId / pipe-helix checks"
        );
    }

    /// Cycle 21 caught fuse+hex. A catalog polyline of hex_vertices, or a
    /// compound-wrapped hex, still skipped is_hex_head — legacy program
    /// body name "Body" then shipped size M8 + AF 10.
    #[test]
    fn fastener_rules_reject_polyline_and_compound_hex_af10() {
        let hex10 = hex_vertices(10.0, [0.0, 0.0]);
        let hex13 = hex_vertices(13.0, [0.0, 0.0]);
        assert!(
            (polyline_regular_hex_af(&hex10).unwrap() - 10.0).abs() < 0.05,
            "detector must recover AF 10 from hex_vertices"
        );
        assert!(
            (polyline_regular_hex_af(&hex13).unwrap() - 13.0).abs() < 0.05,
            "detector must recover AF 13 from hex_vertices"
        );
        let closed: Vec<[f64; 2]> = hex10
            .iter()
            .copied()
            .chain(std::iter::once(hex10[0]))
            .collect();
        assert!(
            (polyline_regular_hex_af(&closed).unwrap() - 10.0).abs() < 0.05,
            "duplicated close point must still be a hex"
        );
        assert!(
            polyline_regular_hex_af(&[
                [0.0, 0.0],
                [10.0, 0.0],
                [10.0, 8.0],
                [5.0, 12.0],
                [0.0, 8.0],
                [-2.0, 3.0]
            ])
            .is_none(),
            "irregular 6-gon is not a hex head"
        );

        let params = serde_json::json!({
            "bolt_length": 40.0,
            "head_height": 5.3,
            "head_width": 13.0,
            "dead_height": 8.0,
            "major_diameter": 8.0
        });
        let finish = |profile: serde_json::Value| {
            serde_json::json!([
                { "op": "sketch", "plane": "XY", "profile": profile },
                { "op": "extrude", "depth": 5.3 },
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                { "op": "thread", "kind": "external", "size": "M8",
                  "length": 26.7, "at": [0, 0, 13.3] },
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let polyline_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "polyline": { "points": hex10, "closed": true }
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&polyline_af10)
            .expect("Body-named polyline hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the polyline AF lie: {reason}"
        );

        let polyline_af13 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "polyline": { "points": hex13, "closed": true }
                }))
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&polyline_af13).is_none(),
            "Body-named polyline hex AF 13 with the golden shank/thread must still pass"
        );

        let compound_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "compound": {
                        "outer": { "hex": { "across_flats": 10 } },
                        "holes": []
                    }
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&compound_af10)
            .expect("compound-wrapped hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the compound AF lie: {reason}"
        );

        let fuse_polyline_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "fuse",
                      "profile": { "polyline": { "points": hex10, "closed": true } },
                      "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&fuse_polyline_af10)
            .expect("fuse polyline hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the fuse-polyline AF lie: {reason}"
        );

        let hex_plate_polyline_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "polyline": { "points": hex_vertices(40.0, [0.0, 0.0]),
                                                 "closed": true } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_polyline_tap).is_none(),
            "polyline hex-plate tap must stay unjudged"
        );
    }

    /// Cycle 27: loft / sweep / common hex skipped is_hex_head (same AF 10
    /// hide as polyline). Shell / offset after thread wrecks the helix.
    #[test]
    fn fastener_rules_reject_loft_sweep_hex_and_shell_after_thread() {
        let params = serde_json::json!({
            "bolt_length": 40.0,
            "head_height": 5.3,
            "head_width": 13.0,
            "dead_height": 8.0,
            "major_diameter": 8.0
        });
        let finish = |head: serde_json::Value| {
            serde_json::json!([
                head,
                { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                { "op": "fillet", "radius": 0.4, "edges": "longest" },
                { "op": "thread", "kind": "external", "size": "M8",
                  "length": 26.7, "at": [0, 0, 13.3] },
                { "op": "chamfer", "distance": 0.5, "edges": "top" }
            ])
        };

        let loft_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "op": "loft",
                    "sections": [
                        { "profile": { "hex": { "across_flats": 10 } }, "at": [0, 0, 0] },
                        { "profile": { "hex": { "across_flats": 10 } }, "at": [0, 0, 5.3] }
                    ]
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&loft_af10)
            .expect("loft hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the loft-hex AF lie: {reason}"
        );

        let loft_af13 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "op": "loft",
                    "sections": [
                        { "profile": { "hex": { "across_flats": 13 } }, "at": [0, 0, 0] },
                        { "profile": { "hex": { "across_flats": 13 } }, "at": [0, 0, 5.3] }
                    ]
                }))
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&loft_af13).is_none(),
            "loft hex AF 13 with the golden shank/thread must still pass"
        );

        let sweep_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "op": "sweep",
                    "profile": { "hex": { "across_flats": 10 } },
                    "path": { "polyline": { "points": [[0, 0, 0], [0, 0, 5.3]] } }
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&sweep_af10)
            .expect("sweep hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the sweep-hex AF lie: {reason}"
        );

        let common_af10 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": finish(serde_json::json!({
                    "op": "common",
                    "profile": { "hex": { "across_flats": 10 } },
                    "depth": 5.3
                }))
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&common_af10)
            .expect("common hex AF 10 next to size M8 must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("13") || l.contains("af") || l.contains("iso") || l.contains("head_width"),
            "reason should name the common-hex AF lie: {reason}"
        );

        let shell_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "shell", "thickness": 1, "faces": "largest" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&shell_after)
            .expect("shell after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("shell") && l.contains("after thread"),
            "reason should name shell after thread: {reason}"
        );

        let offset_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "offset", "distance": 0.2 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&offset_after)
            .expect("offset after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("offset") && l.contains("after thread"),
            "reason should name offset after thread: {reason}"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        let helix_after_cut = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "helix", "pitch": 1.25, "height": 26.7, "radius": 4,
                      "diameter": 0.8, "center": [0, 0, 13.3] }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&helix_after_cut)
            .expect("helix after a legal thread CUT must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("helix") || l.contains("torus") || l.contains("fake"),
            "reason should name helix/torus after thread: {reason}"
        );

        let revolve_after_cut = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "revolve", "axis": "Z", "angle": 360 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&revolve_after_cut)
            .expect("revolve after a legal thread CUT must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("revolve") || l.contains("fake") || l.contains("helix"),
            "reason should name revolve after thread: {reason}"
        );

        let pattern_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "pattern", "kind": "linear", "count": 2,
                      "spacing": 10, "direction": [0, 0, 1], "scope": "feature" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&pattern_after)
            .expect("pattern after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("pattern") && l.contains("thread"),
            "reason should name pattern after thread: {reason}"
        );

        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "hex-plate tap must still pass"
        );
        assert!(
            fastener_recipe_violation(&example_m8_bolt_document()).is_none(),
            "golden recipe must still pass"
        );
    }

    /// Cycle 23 read DraftExtrude. Loft/sweep hex AF is judged, but
    /// head_from_feat still missed sweep path / loft Z-span, so a
    /// fully-threaded M8 shipped when head_height was omitted.
    #[test]
    fn fastener_rules_sweep_and_loft_head_still_drive_grip() {
        let sweep_fully = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "sweep",
                      "path": { "polyline": { "points": [[0,0,0],[0,0,5.3]] } } },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 34.7, "at": [0, 0, 5.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&sweep_fully)
            .expect("sketch+sweep head + thread at the head must fail without head_height");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("unthreaded") || l.contains("dead_height") || l.contains("grip"),
            "reason should name the missing grip: {reason}"
        );

        let sweep_ok = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "sweep",
                      "path": { "polyline": { "points": [[0,0,0],[0,0,5.3]] } } },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&sweep_ok).is_none(),
            "sketch+sweep head with a parameter-driven grip must still pass"
        );

        let loft_fully = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "loft", "sections": [
                        { "profile": { "hex": { "across_flats": 13 } }, "at": [0, 0, 0] },
                        { "profile": { "hex": { "across_flats": 13 } }, "at": [0, 0, 5.3] }
                    ] },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 34.7, "at": [0, 0, 5.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&loft_fully)
            .expect("loft hex head + thread at the head must fail without head_height");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("unthreaded") || l.contains("dead_height") || l.contains("grip"),
            "reason should name the missing loft grip: {reason}"
        );
    }

    /// Cycle 22 caught named bolt + tap. Hex + shank named bolt with no
    /// Thread op still skipped every recipe check.
    #[test]
    fn fastener_rules_named_bolt_without_thread_fails() {
        let blank = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&blank)
            .expect("named bolt + hex + shank with no thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("external") && l.contains("thread"),
            "reason should require external CUT: {reason}"
        );

        let hex_post = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_main",
                "name": "Body",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_post).is_none(),
            "a hex post named Body is not a named bolt"
        );

        let named_m8 = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "head_width": 13.0,
                "dead_height": 8.0,
                "major_diameter": 8.0
            },
            "bodies": [{
                "bodyId": "body_main",
                "name": "M8",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&named_m8)
            .expect("named M8 + hex + shank with no thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("external") && l.contains("thread"),
            "reason should require external CUT on named M8: {reason}"
        );

        let m8_hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "M8 hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "cylinder", "diameter": 16, "height": 4, "at": [0, 0, 12] },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&m8_hex_plate_tap).is_none(),
            "M8 hex plate + boss + tap must stay unjudged"
        );

        let hex_plate = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate).is_none(),
            "hex plate without a tap must stay unjudged"
        );
    }

    /// Shell/offset after thread already fail. Catalog draft / thicken still
    /// silently wrecked the helix. Cut / transform after thread stay legal
    /// (drive slot, place the bolt). Hex-plate+tap stays unjudged.
    #[test]
    fn fastener_rules_reject_draft_and_thicken_after_thread() {
        let params = serde_json::json!({
            "bolt_length": 40.0,
            "head_height": 5.3,
            "head_width": 13.0,
            "dead_height": 8.0,
            "major_diameter": 8.0
        });
        let draft_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "draft", "faces": "side", "angle": 2 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&draft_after)
            .expect("draft after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("draft") && l.contains("after thread"),
            "reason should name draft after thread: {reason}"
        );

        let thicken_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "thicken", "thickness": 0.4, "face": "largest" }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&thicken_after)
            .expect("thicken after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("thicken") && l.contains("after thread"),
            "reason should name thicken after thread: {reason}"
        );

        let common_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "common",
                      "profile": { "circle": { "d": 6 } },
                      "depth": 40 }
                ]
            }]
        }))
        .unwrap();
        let reason = fastener_recipe_violation(&common_after)
            .expect("common after thread must fail");
        let l = reason.to_ascii_lowercase();
        assert!(
            l.contains("common") && l.contains("after thread"),
            "reason should name common after thread: {reason}"
        );

        let cut_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "cut",
                      "profile": { "rect": { "w": 1.2, "h": 8, "centered": true } },
                      "depth": 2, "at": [0, 0, 5.3] }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&cut_after).is_none(),
            "cut after thread (drive slot) must still pass"
        );

        let transform_after = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "parameters": params,
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 5.3 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "fillet", "radius": 0.4, "edges": "longest" },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 26.7, "at": [0, 0, 13.3] },
                    { "op": "chamfer", "distance": 0.5, "edges": "top" },
                    { "op": "transform", "translate": [10, 0, 0] }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&transform_after).is_none(),
            "transform after thread (place the bolt) must still pass"
        );

        let hex_plate_tap = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "bodies": [{
                "bodyId": "body_plate",
                "name": "hex plate",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 40 } } },
                    { "op": "extrude", "depth": 12 },
                    { "op": "thread", "kind": "tap", "size": "M8",
                      "center": [0, 0], "through": true },
                    { "op": "draft", "faces": "side", "angle": 2 },
                    { "op": "thicken", "thickness": 0.4, "face": "largest" },
                    { "op": "common",
                      "profile": { "circle": { "d": 30 } },
                      "depth": 12 },
                    { "op": "revolve", "axis": "Z", "angle": 360 }
                ]
            }]
        }))
        .unwrap();
        assert!(
            fastener_recipe_violation(&hex_plate_tap).is_none(),
            "hex-plate tap with draft/thicken/common/revolve after tap must stay unjudged"
        );
        assert!(
            fastener_recipe_violation(&example_m8_bolt_document()).is_none(),
            "golden recipe must still pass"
        );
    }

}
