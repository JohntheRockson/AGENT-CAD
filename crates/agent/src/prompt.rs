//! Product-agent system prompt (the single in-app chat agent).
//!
//! Keep this prompt short. The verify loop must use [`VERIFY_SYSTEM_PROMPT`],
//! not [`SYSTEM_PROMPT`] — re-sending the op catalog on every verify is the
//! size/cost leak. Do not paper over tessellation/wasm crashes here; that is
//! Kernel's job.

/// System instruction sent to Gemini on `/api/chat` generation.
pub const SYSTEM_PROMPT: &str = r#"You are AgentCAD's product agent. Emit valid JSON IR the kernel can run.

## Output
ONLY one JSON object. No markdown.
New design:
{ "say": "<2–4 sentences, plain English>", "document": {
    "documentId": "<slug>", "units": "mm",
    "parameters": { "plate_width": 80, "plate_thickness": 10 },
    "bodies": [{ "bodyId": "<slug>", "name": "<label>", "visible": true,
      "transform": { "position": [0,0,0], "rotation": [0,0,0] },
      "features": [ { "op": "box", "size": ["plate_width", 40, "plate_thickness"] } ],
      "references": [] }] } }
Edit one body (targetBodyId given): { "say": "...", "body": { "bodyId": "<same>", ... } }.
Legacy { "say", "program": { "units", "features" } } is one body.
Feature tag is "op". Sizes > 0. Coordinates may be negative.

## Parameters
ALWAYS emit a "parameters" map (never omit it). Put overall dims there
(`bolt_length`, `head_width`, `head_height`, `dead_height`, `major_diameter`, `pitch`).
Reference by name or expression: "size": ["w","d","t"], "depth": "head_height".
Hex heads: { "hex": { "across_flats": "head_width" } } — never hard-code hex points.

## Default bolt recipe
Recipe: hex extrude → overlapping cylinder → thread CUT.
NEVER default to thread-first then fuse a head (helical caps cannot union with a prism).
M8 table (ISO 261/4014/4017): Ø8, pitch 1.25, AF 13 — emit head_width 13, NOT 10. Parameters: major_diameter 8, pitch 1.25, head_width 13, dead_height. Thread size:"M8"; diameter/pitch stay null.
1) sketch { "hex": { "across_flats": "head_width" } }, extrude "head_height".
2) overlapping cylinder: "diameter":"major_diameter", "height":"bolt_length - head_height + 1", "at":[0,0,"head_height - 1"].
3) Unthreaded grip — do not fully-thread head-to-tip:
   "length":"bolt_length - head_height - dead_height", "at":[0,0,"head_height + dead_height"].
4) Under-head fillet BEFORE thread (small r, edges:"longest"). Tip chamfer edges:"top". NEVER fillet/chamfer edges:"all" or "longest" after thread.
5) { "op":"thread", "kind":"external", "size":"M8" } on an existing solid CUTS the groove.
size is M8 / M8x1 / 1/4-20. Do not fake threads with tori, rings, or revolved grooves.

## Multi-body
Assemblies = separate bodies, not one fused blob. Holes live on the body they pierce.
Optional cross-body boolean on the TOOL: "references": [{ "op": "cut"|"fuse", "target": "<bodyId>", "consume": false }].
"transform": { "position": [x,y,z], "rotation": [rx,ry,rz] } (Euler degrees). Y rotation is valid.
Start every body with a solid: box, cylinder, sphere, cone, torus, ellipsoid, helix, sketch then extrude/revolve/sweep, or fuse.
Do not start a body with cut, hole, fillet, chamfer, transform, offset, thicken, draft, common, or internal thread.
External thread may start a shank-only body; a full bolt uses the hex→cylinder→thread recipe above.

## Coordinates
Z is up. Ground is XY. Parts sit on XY and grow +Z. Default plane XY; omit it.
Stack by changing Z in at. World origin is the XY center.
Rects/boxes are centered on at (centered:true). A 50×50 at [0,0] spans [-25,25].
A center hole on a centered plate is "center": [0,0].
Revolve/lathe/tube: plane XZ, points [radius, height], axis Z. Never revolve around the plane normal.

## Profiles
{ "rect": { "w": <w>, "h": <h>, "at": [x,y], "centered": true } }
{ "circle": { "d": <diameter>, "at": [x,y] } }
{ "polyline": { "points": [[x,y],...], "closed": true } }
{ "arc": { "center": [x,y], "radius": <r>, "start_angle": <deg>, "end_angle": <deg> } }
{ "compound": { "outer": <Profile>, "holes": [<Profile>] } }
{ "ellipse": { "major": <d1>, "minor": <d2>, "at": [x,y] } }
{ "hex": { "across_flats": <wrench>, "at": [x,y] } }

Wishbones/brackets: sketch a SIMPLE outer outline, then CUT the window. Do not trace a return path. Bosses must overlap the plate. Later primitives JOIN the current solid.

## Feature ops
sketch { "op":"sketch", "plane":"XY"|"XZ"|"YZ", "profile": <Profile>, "origin":[x,y], "face":"largest"|"top"|"bottom"|<i> }
extrude { "op":"extrude", "depth": <n>, "symmetric": false }
draft_extrude { "op":"draft_extrude", "depth": <n>, "angle": <deg> }
revolve { "op":"revolve", "axis":"X"|"Y"|"Z", "angle":360, "origin":[x,y,z] }
loft { "op":"loft", "ruled": true, "sections": [{"profile":<P>,"at":[x,y,z]}], "apex": [x,y,z] }
sweep { "op":"sweep", "profile":<P>, "path": <Path>, "fuse": true }
pipe { "op":"pipe", "diameter":<d>, "path": <Path>, "fuse": true }
  Path: { "polyline": { "points": [[x,y,z],...] } } or { "helix": { "pitch":<p>, "height":<h>, "radius":<r>, "center":[x,y,z], "axis":"Z" } }
helix { "op":"helix", "pitch":<p>, "height":<h>, "radius":<r>, "diameter":<wire>, "center":[x,y,z], "axis":"Z", "fuse": true }
thicken { "op":"thicken", "thickness":<t>, "face":"largest"|<i>, "fuse": true }
box { "op":"box", "size":[dx,dy,dz], "at":[x,y,z], "centered": true }
cylinder { "op":"cylinder", "diameter":<d>, "height":<h>, "at":[x,y,z], "axis":"Z"|"X"|"Y" }
  at is the BOTTOM. Later primitives JOIN (bosses, shank on a hex).
sphere { "op":"sphere", "diameter":<d>, "at":[x,y,z] }
cone { "op":"cone", "d1":<base>, "d2":<top>, "height":<h>, "at":[x,y,z] }
torus { "op":"torus", "major":<R>, "minor":<r>, "at":[x,y,z] }
ellipsoid { "op":"ellipsoid", "radii":[rx,ry,rz], "at":[x,y,z] }
thread { "op":"thread", "kind":"external"|"internal"|"die"|"tap", "size":"M8", "length":<mm>, "at":[x,y,z], "axis":"Z" }
  On an existing solid, external CUTS a helical groove. Internal/tap needs a solid; "center":[x,y], "through": true.
  size "M8" is enough — diameter/pitch may be null (ISO 261).
hole { "op":"hole", "diameter":<d>, "depth":<h>, "center":[x,y], "plane":"XY", "face":"largest"|"top"|<i> }
cut { "op":"cut", "profile":<P>, "depth":<h>, "at":[x,y,z], "plane":"XY", "face":"largest"|<i>, "through": true }
fuse { "op":"fuse", "profile":<P>, "depth":<h>, "at":[x,y,z], "plane":"XY", "face":"largest"|<i> }
common { "op":"common", "profile":<P>, "depth":<h>, "at":[x,y,z], "plane":"XY" }
fillet { "op":"fillet", "radius":<r>, "edges":"all"|"top"|"longest"|[i] } Never all/longest after thread
chamfer { "op":"chamfer", "distance":<d>, "angle":<deg>, "edges":"all"|"top"|[i] } Never all/longest after thread
transform { "op":"transform", "translate":[x,y,z], "rotate":{"axis":[x,y,z],"angle":<deg>,"origin":[x,y,z]}, "scale":<s> }
mirror { "op":"mirror", "plane":"YZ"|"XZ"|"XY", "origin":[x,y,z], "fuse": true }
pattern { "op":"pattern", "kind":"linear"|"circular", "count":<n≥2>, "spacing":<d>, "direction":[x,y,z], "axis":"Z", "angle":<deg>, "center":[x,y,z], "scope":"body"|"feature" }
shell { "op":"shell", "thickness":<t>, "faces":"all"|[i]|"largest" }
offset { "op":"offset", "distance":<d> }
draft { "op":"draft", "faces":"side"|[i], "angle":<deg>, "direction":[0,0,1] }

## Example — M8 bolt (hex → overlapping cylinder → thread CUT)
{
  "units": "mm",
  "parameters": { "bolt_length": 40, "head_width": 13, "head_height": 5.3,
    "dead_height": 8, "major_diameter": 8, "pitch": 1.25 },
  "features": [
    { "op": "sketch", "plane": "XY", "profile": { "hex": { "across_flats": "head_width" } } },
    { "op": "extrude", "depth": "head_height" },
    { "op": "cylinder", "diameter": "major_diameter", "height": "bolt_length - head_height + 1",
      "at": [0, 0, "head_height - 1"] },
    { "op": "fillet", "radius": 0.4, "edges": "longest" },
    { "op": "thread", "kind": "external", "size": "M8",
      "length": "bolt_length - head_height - dead_height", "at": [0, 0, "head_height + dead_height"] },
    { "op": "chamfer", "distance": 0.5, "edges": "top" }
  ]
}

## Example — plate with M8 tap (size only; no diameter/pitch)
{
  "units": "mm",
  "parameters": { "plate_size": 40, "plate_thickness": 12 },
  "features": [
    { "op": "box", "size": ["plate_size", "plate_size", "plate_thickness"], "centered": true },
    { "op": "thread", "kind": "tap", "size": "M8", "center": [0, 0], "plane": "XY", "through": true }
  ]
}
"#;

/// System instruction for the post-build verify Gemini call.
///
/// Must not include the Feature-op catalog. Generation already sent
/// [`SYSTEM_PROMPT`]; re-sending it on every verify is the size/cost leak.
/// Short fastener-order rules only — catch thread-first again.
pub const VERIFY_SYSTEM_PROMPT: &str = r#"You judge whether built CAD solids match the user's request.
Do not emit Feature ops. Do not list or repeat an op catalog or profile schemas.
Reply with JSON only:
{ "ok": true, "say": "<2-4 sentence description>" }
or
{ "ok": false, "reason": "<what's wrong>", "say": "...", "document": { ...fixed CadDocument } }
If you return a document, always include a "parameters" map.
Do not diagnose tessellation or wasm crashes; that is the kernel's job.

Fastener order (judge only):
A hex-head bolt must be hex extrude → overlapping cylinder → thread CUT.
Reject thread-first then fuse a head.
Reject fillet or chamfer edges:"all" or "longest" after thread (that wrecks the helix).
Reject a hex-head bolt fully threaded from the head (missing dead_height).
head_width must drive the hex wrench size; ISO M8 / M8x1.25 is AF 13 even if head_width or size is omitted (not 10).
dead_height must drive thread start.
major_diameter (or the ISO size token when that param is omitted) must drive the shank cylinder.
Explicit thread.pitch must match ISO when the pitch param is omitted (M8 is 1.25; prefer null).
pitch param must match the ISO token (M8 is 1.25) — no size-table lie.
Require under-head fillet before thread and a tip chamfer after thread (edges:"top").
Reject a thread that runs past the bolt tip.
Reject a second external thread on a hex-head bolt, or a pattern after thread.
Reject any fillet after thread (not only edges:"all" / "longest").
Chamfer after thread must be edges:"top" — not bottom/all/longest.
A body named bolt or screw must use external thread CUT, not tap/internal.
Reject helix, torus, or revolve in place of or after thread CUT.
A body named M8 (or documentId / an Ø8 shank) is still ISO AF 13 / Ø8 even if size is omitted or pitch is a lie.
Named M8 hex+shank still needs thread CUT (not a blank shank).
Reject shell, offset, draft, thicken, or common after thread (that wrecks the helix).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_is_at_most_about_8k_chars() {
        assert!(
            SYSTEM_PROMPT.len() <= 8_000,
            "generation prompt is {} chars; target is ≤ ~8k",
            SYSTEM_PROMPT.len()
        );
    }

    /// Golden recipe must stay explicit, and thread-first-then-fuse-head must
    /// not be the default (the old shipping example taught that).
    #[test]
    fn prompt_keeps_golden_bolt_recipe_not_thread_first() {
        let p = SYSTEM_PROMPT.to_ascii_lowercase();
        assert!(p.contains("hex extrude"), "recipe step 1 missing");
        assert!(p.contains("overlapping cylinder"), "recipe step 2 missing");
        assert!(
            p.contains("thread cut") || SYSTEM_PROMPT.contains("thread CUT"),
            "recipe step 3 missing"
        );
        assert!(
            SYSTEM_PROMPT.contains(r#""hex": { "across_flats": "head_width" }"#),
            "golden hex sketch missing"
        );
        assert!(SYSTEM_PROMPT.contains(r#""op": "cylinder""#));
        assert!(SYSTEM_PROMPT.contains(r#""op": "thread""#));
        assert!(SYSTEM_PROMPT.contains(r#""size": "M8""#));

        assert!(
            p.contains("never") && (p.contains("thread-first") || p.contains("thread first")),
            "must explicitly reject thread-first then fuse head as the default"
        );
        assert!(
            !SYSTEM_PROMPT.contains(r#""at": [0, 0, 24]"#),
            "old thread-first + fuse-head example (cylinder head at z=24) is still in the prompt"
        );
        assert!(
            !SYSTEM_PROMPT.contains("Example — M8 bolt shank"),
            "old thread-first shank example is still in the prompt"
        );

        let example = SYSTEM_PROMPT
            .split("## Example — M8 bolt")
            .nth(1)
            .expect("M8 bolt example section");
        let hex_pos = example
            .find(r#""hex": { "across_flats": "head_width" }"#)
            .expect("hex in example");
        let cyl_pos = example
            .find(r#""op": "cylinder""#)
            .expect("cylinder in example");
        let thread_pos = example
            .find(r#""op": "thread""#)
            .expect("thread in example");
        assert!(
            hex_pos < cyl_pos && cyl_pos < thread_pos,
            "example order must be hex then cylinder then thread, not thread-first"
        );
    }

    #[test]
    fn prompt_teaches_m8_iso_size_table_af13() {
        let p = SYSTEM_PROMPT.to_ascii_lowercase();
        assert!(p.contains("across-flats") || p.contains("across flats") || p.contains("af 13"));
        assert!(
            p.contains("head_width 13") || SYSTEM_PROMPT.contains(r#""head_width": 13"#),
            "must teach AF 13, not 10"
        );
        assert!(
            !SYSTEM_PROMPT.contains(r#""head_width": 10"#),
            "old AF 10 example is still in the prompt"
        );
        assert!(p.contains("1.25"));
        assert!(
            p.contains("major_diameter") && SYSTEM_PROMPT.contains("8"),
            "size table must expose Ø8"
        );
    }

    #[test]
    fn prompt_teaches_unthreaded_grip_and_safe_finishing() {
        let p = SYSTEM_PROMPT.to_ascii_lowercase();
        assert!(
            p.contains("dead_height") || p.contains("unthreaded"),
            "must teach unthreaded grip"
        );
        assert!(
            p.contains("fillet") && (p.contains("under-head") || p.contains("under head")),
            "must teach under-head fillet"
        );
        assert!(p.contains("chamfer"), "must teach tip chamfer");
        assert!(
            p.contains("never") && p.contains("edges:\"all\"") && p.contains("after thread"),
            "must forbid fillet edges:all after thread"
        );
        assert!(
            SYSTEM_PROMPT.contains(r#"fillet/chamfer edges:"all" or "longest" after thread"#),
            "must forbid chamfer/fillet edges:longest after thread, not only fillet-all"
        );
        assert!(
            SYSTEM_PROMPT.contains(r#"Never all/longest after thread"#),
            "op catalog must forbid all/longest after thread"
        );
        let fillet_line = SYSTEM_PROMPT
            .lines()
            .find(|l| l.contains(r#""op":"fillet""#))
            .expect("fillet catalog line");
        let chamfer_line = SYSTEM_PROMPT
            .lines()
            .find(|l| l.contains(r#""op":"chamfer""#))
            .expect("chamfer catalog line");
        assert!(
            fillet_line.contains("longest") && fillet_line.contains("after thread"),
            "op catalog fillet line must forbid longest after thread: {fillet_line}"
        );
        assert!(
            chamfer_line.contains("longest") && chamfer_line.contains("after thread"),
            "op catalog chamfer line must forbid longest after thread, not only list all|top: {chamfer_line}"
        );
    }

    #[test]
    fn prompt_always_emits_parameters_and_allows_null_m8_pitch() {
        let p = SYSTEM_PROMPT.to_ascii_lowercase();
        assert!(
            p.contains("always emit") && p.contains("parameters"),
            "must require a parameters map"
        );
        assert!(p.contains("iso 261"));
        assert!(
            p.contains("omit diameter and pitch")
                || p.contains("diameter/pitch may be null")
                || p.contains("diameter/pitch stay null")
                || p.contains("diameter and pitch (null")
        );
    }

    #[test]
    fn verify_prompt_does_not_resend_op_catalog() {
        assert!(
            VERIFY_SYSTEM_PROMPT.len() < 2_000,
            "verify prompt is {} chars; it must not carry the catalog",
            VERIFY_SYSTEM_PROMPT.len()
        );
        assert!(
            VERIFY_SYSTEM_PROMPT.len() * 4 < SYSTEM_PROMPT.len(),
            "verify prompt should be a small fraction of the generation prompt"
        );
        let v = VERIFY_SYSTEM_PROMPT.to_ascii_lowercase();
        for needle in [
            "draft_extrude",
            "ellipsoid",
            "across_flats",
            "## feature ops",
            r#""op":"sketch""#,
        ] {
            assert!(
                !v.contains(needle),
                "verify prompt must not re-send the op catalog (found {needle:?})"
            );
        }
    }

    #[test]
    fn verify_prompt_catches_thread_first_without_catalog() {
        let v = VERIFY_SYSTEM_PROMPT.to_ascii_lowercase();
        assert!(
            v.contains("hex extrude") && v.contains("overlapping cylinder") && v.contains("thread"),
            "verify must restate the golden fastener order"
        );
        assert!(
            v.contains("thread-first") || v.contains("thread first"),
            "verify must reject thread-first"
        );
        assert!(
            v.contains("edges:\"all\"") && v.contains("after thread") && v.contains("chamfer"),
            "verify must reject fillet-all and chamfer-all after thread"
        );
        assert!(
            v.contains("longest"),
            "verify must reject fillet/chamfer edges:longest after thread"
        );
        assert!(
            v.contains("dead_height") && (v.contains("fully threaded") || v.contains("unthreaded")),
            "verify must reject a fully-threaded hex bolt (missing grip)"
        );
        assert!(
            v.contains("head_width") && v.contains("drive"),
            "verify must require head_width to drive the hex"
        );
        assert!(
            (v.contains("af 13") || v.contains("af is 13")) && (v.contains("omitted") || v.contains("iso")),
            "verify must bind ISO M8 / M8x1.25 to AF 13 when head_width or size is omitted"
        );
        assert!(
            v.contains("m8x1.25") && v.contains("size"),
            "verify must treat M8x1.25 / omitted size as still M8 AF 13"
        );
        assert!(
            v.contains("major_diameter") && v.contains("cylinder"),
            "verify must require major_diameter to drive the shank"
        );
        assert!(
            v.contains("omitted") || v.contains("iso size"),
            "verify must bind the ISO size token when major_diameter is omitted"
        );
        assert!(
            v.contains("thread.pitch") || (v.contains("pitch") && v.contains("1.25")),
            "verify must bind explicit thread.pitch to ISO when pitch is omitted"
        );
        assert!(
            v.contains("pitch param") || (v.contains("pitch") && v.contains("size-table")),
            "verify must reject a pitch param that fights the ISO token"
        );
        assert!(
            v.contains("fillet") && v.contains("before thread"),
            "verify must require under-head fillet before thread"
        );
        assert!(
            v.contains("chamfer") && v.contains("tip") && v.contains("after thread"),
            "verify must require a tip chamfer after thread"
        );
        assert!(
            v.contains("past") && v.contains("tip"),
            "verify must reject a thread that runs past the tip"
        );
        assert!(
            v.contains("second") && v.contains("thread"),
            "verify must reject a second external thread"
        );
        assert!(
            v.contains("pattern") && v.contains("after thread"),
            "verify must reject a pattern after thread"
        );
        assert!(
            v.contains("any fillet after thread") || v.contains("fillet after thread"),
            "verify must reject any fillet after thread, not only all/longest"
        );
        assert!(
            v.contains("tap") && v.contains("external"),
            "verify must reject a named bolt that is a tap"
        );
        assert!(
            v.contains("screw"),
            "verify must reject a named screw that is a tap"
        );
        assert!(
            v.contains("helix") && v.contains("torus") && v.contains("revolve"),
            "verify must reject helix/torus/revolve in place of thread CUT"
        );
        assert!(
            (v.contains("named m8") || v.contains("body named m8")) && v.contains("af 13"),
            "verify must bind a named-M8 body to ISO AF 13"
        );
        assert!(
            v.contains("blank shank") || (v.contains("hex+shank") && v.contains("thread")),
            "verify must reject a named-M8 hex+shank with no thread CUT"
        );
        assert!(
            v.contains("documentid") || v.contains("document id"),
            "verify must bind documentId M8 to ISO AF 13"
        );
        assert!(
            v.contains("shell") && v.contains("offset") && v.contains("after thread"),
            "verify must reject shell/offset after thread"
        );
        assert!(
            v.contains("draft") && v.contains("thicken") && v.contains("after thread"),
            "verify must reject draft/thicken after thread"
        );
        assert!(
            v.contains("common") && v.contains("after thread"),
            "verify must reject common after thread"
        );
        assert!(
            v.contains("edges:\"top\"") && v.contains("bottom"),
            "verify must require chamfer edges:top after thread"
        );
        assert!(
            !v.contains("draft_extrude") && !v.contains("## feature ops"),
            "short fastener-order rules only — no catalog dump"
        );
    }
}
