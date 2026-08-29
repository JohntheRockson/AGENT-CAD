//! Axum HTTP server for AgentCAD.
//!
//! # Endpoints
//! - `POST /api/run`    – execute a CadProgram, return mesh + metrics
//! - `POST /api/export` – execute a CadProgram, stream back a binary file
//! - `POST /api/chat`   – natural language → Gemini → CadProgram → mesh (with repair loop)
//! - `POST /api/topology` – list faces/edges with semantic tags for agent selection
//! - `POST /api/verify`  – deterministic structural checks (no LLM)
//! - `GET  /api/health` – liveness probe
//!
//! # Geometry backend
//! Compiled with `features = ["occt"]` by default (server/Cargo.toml).
//! Pass `--no-default-features` to use the mock backend for UI development
//! without a long compile.
//!
//! # Running
//! ```text
//! cargo run -p server --release
//! # Open http://localhost:5173 in the browser (after `npm run dev` in apps/web)
//! ```

mod preview;

use std::{convert::Infallible, sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{Json, State},
    http::{header, Method, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Response,
    },
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use kernel::{
    engine::{DocumentOutput, Engine, ExportFormat, MetricsData},
    ir::{CadBody, CadDocument, Units},
    verify::{self, VerificationReport},
};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

// ── Gemini configuration ───────────────────────────────────────────────────────

const GEMINI_MODEL: &str = "gemini-3.7-flash";

const SYSTEM_PROMPT: &str = r#"You are an expert CAD engineering AI integrated into AgentCAD.
Convert the user's description into a CadDocument: one or more independent bodies, each with its own feature tree.

## Output rules
- Output ONLY a single valid JSON object. No markdown, no prose outside JSON.
- New designs (default):
  { "say": "<2–4 sentence summary>", "document": {
      "documentId": "<slug>",
      "units": "mm",
      "parameters": { "plate_width": 80, "plate_depth": 40, "plate_thickness": 10 },
      "bodies": [
        { "bodyId": "<slug>", "name": "<label>", "visible": true,
          "transform": { "position": [0,0,0], "rotation": [0,0,0] },
          "features": [ { "op": "box", "size": ["plate_width", "plate_depth", "plate_thickness"] } ],
          "references": [] }
      ]
    } }
- Targeted edit of one body (when a targetBodyId is given):
  { "say": "...", "body": { "bodyId": "<same id>", "name": "...", "visible": true, "transform": {...}, "features": [...], "references": [] } }
  Copy transform/visibility from the target. Do NOT return other bodies.
- Legacy single-solid `{ "say", "program": { "units", "features" } }` is allowed and becomes one body.
- `say` is a 2–4 sentence summary of what you built. No JSON, no op names. Plain English.
- Feature ops still use `"op"` (not `"type"`). Sizes > 0. Coordinates MAY be negative.

## Parameters (CRITICAL for verification)
- Put every important **overall** dimension in `"parameters"` (`bolt_length`, `head_width`, `plate_thickness`).
- Reference parameters by name OR a simple expression anywhere a number is allowed:
  `"size": ["plate_width", "plate_depth", "plate_thickness"]`, `"depth": "head_height"`, `"length": "bolt_length - head_height"`.
- Hex heads: `{ "hex": { "across_flats": "head_width" } }` — do NOT hard-code hex polyline points.
- Internal dims (`head_height`, pitch) may be parameters if they are referenced in features. Do not expect them to match the overall bounding box.
- When editing one dimension, change the parameter value and keep feature fields as names/expressions.

## Threads (CRITICAL)
- External/internal threads MUST use `{ "op": "thread", "kind": "external"|"internal", "size": "M8", "length": <or expression> }`.
- `size` is an ISO/UN designation (`M8`, `M8x1`, `1/4-20`). The kernel cuts a **helix**, not stacked rings.
- M8 coarse is Ø8 × 1.25 mm (ISO 261). Do not fake threads with patterned tori or revolved grooves.

## Multi-body (CRITICAL)
- Assemblies and multi-part designs MUST be separate bodies (base plate, bracket, shaft, fasteners, lid, …) — never one fused blob.
- Each body is a complete CadProgram feature list. Holes belong on the body they pierce.
- Cross-body boolean (optional): on the TOOL body, `"references": [{ "op": "cut"|"fuse", "target": "<bodyId>", "consume": false }]`.
  `consume: true` hides the tool after the boolean.
- Place bodies with `"transform": { "position": [x,y,z], "rotation": [rx,ry,rz] }` (Euler degrees).
  Rotation on X, Y, or Z is valid. Do not avoid Y rotation.
- Start EVERY body with a solid: `box`, `cylinder`, `sphere`, `cone`, `torus`, `ellipsoid`, `helix`, `thread` (external), `sketch` then `extrude`/`revolve`/`sweep`, or `fuse`.
  Never start a body with `cut`, `hole`, `fillet`, `chamfer`, `transform`, `offset`, `thicken`, `draft`, `common`, or internal `thread` (tap).
- bodyId: stable slug like `body_base_plate`. name: human label for the outliner.
- When the user asks to change one part (holes, thickness, that bracket), edit ONLY that body.
- Example assembly (two bodies, not fused):
  plate `{ "op":"box", "size":[80,50,6] }` as body_base_plate, pin as body_pin with
  `"transform": { "position": [0,0,6], "rotation":[0,0,0] }` and a cylinder. Holes live on the plate body.

## Coordinate system (CRITICAL — read this)
- **Z is up. The ground is the XY plane.** Parts sit on XY and grow in +Z.
- For **extrude / fuse / box**: if the user says "on the ground/floor/grid", use `"plane": "XY"`. Do **not** set `"plane": "XZ"` unless they want a **vertical wall**.
- For **revolve / lathe / tube / venturi / bottle**: `"plane": "XZ"` is required. Sketch `[radius, height]` and revolve around **Z**.
- Default `"plane"` is `"XY"`. Omit it. Stacking = change **Z** in `at` (`[0, 0, 5]`, `[0, 0, 10]`, …). Never stack by changing Y.
- World origin (0,0,0) is the CENTER of the part in XY.
- Rectangles and boxes are CENTERED on their `at` point by default (`"centered": true`).
  A 50×50 rect at `[0, 0]` spans X=[-25, 25] and Y=[-25, 25].
  **WRONG:** `"at": [-25, -25]` on a 50×50 centered rect. That shifts the whole part off the origin.
  **RIGHT:** omit `at`, or use `"at": [0, 0]`.
- NEVER build only the positive octant. Symmetric parts must straddle the origin.
- A hole at the center of a centered plate is `"center": [0, 0]`.

## CadProgram schema
{ "units": "mm"|"in", "features": [ ...ops tagged by "op"... ] }

## Profiles (used by sketch, cut, fuse, loft, sweep, common)
{ "rect":     { "w": <w>, "h": <h>, "at": [x,y], "centered": true } }
{ "circle":   { "d": <diameter>, "at": [x,y] } }          — `at` is the CENTER
{ "polyline": { "points": [[x,y],...], "closed": true } } — ≥3 points, coords may be negative
{ "arc":      { "center": [x,y], "radius": <r>, "start_angle": <deg>, "end_angle": <deg> } }
{ "compound": { "outer": <Profile>, "holes": [ <Profile>, ... ] } }
          Multi-contour: outer profile with inner holes (flange with cutouts, pocket islands).
{ "ellipse":  { "major": <d1>, "minor": <d2>, "at": [x,y] } } — full widths, like circle `d`
{ "hex":      { "across_flats": <wrench size>, "at": [x,y] } } — regular hex (M8 head = 13)

Set `"centered": false` on a rect ONLY when `at` should be the min-corner, not the center.

## Control arms / wishbones / brackets with pockets (CRITICAL)
- Sketch ONE simple OUTER outline. Then CUT the inner window with hole/cut.
- NEVER close a polyline by tracing back around the inside of the part. That self-intersects and tessellates as jagged disconnected bars.
- Bosses and bushing eyes: cylinder JOINED onto the extruded plate. The cylinder MUST overlap the plate (height taller than thickness, `at.z` a few mm below the plate). A cylinder sitting above the face stays a separate lump.
- Fasteners (bolts/bushings) are separate bodies. Structural parts (arm, knuckle, strut housing) must each be ONE continuous solid.
- Assemblies must be ASSEMBLED, not an exploded view. Put the knuckle between the UCA and LCA ball-joint cups so bboxes overlap. Plant the strut on the LCA pad (strut `transform.position` = the pad, cylinders stacked in +Z from there). The top hat is at the TOP (high Z), not at z=0.
- Fillet last with a SMALL radius (1.5–2.5, always less than half the plate thickness). Skip fillet rather than using a large r that shatters the solid.

Example A-arm (copy this pattern):
  sketch outer triangle/wishbone polyline closed, extrude 8,
  cut inner window,
  cylinder bosses at the three eyes overlapping the plate,
  hole through each eye,
  fillet r=2.

## Feature ops

### 2D → 3D
sketch         { "op":"sketch", "plane":"XY"|"XZ"|"YZ", "profile": <Profile>, "origin":[x,y],
                 "face":"largest"|"top"|"bottom"|<index> }
               Optional `face` places the sketch on an existing solid face (then extrude/thicken).
extrude        { "op":"extrude", "depth": <positive>, "symmetric": false }
               symmetric:true extrudes depth/2 both ways. On a face-sketch, fuses into the solid.
draft_extrude  { "op":"draft_extrude", "depth": <positive>, "angle": <deg> }
               Tapered extrusion of the last sketch. Positive angle tapers inward (pyramid-like).
revolve        { "op":"revolve", "axis":"X"|"Y"|"Z", "angle":360, "origin":[x,y,z] }
               LATHE. Sketch a HALF CROSS-SECTION first. The revolve axis MUST lie in the
               sketch plane. Revolving around the plane normal makes a flat disk (WRONG).
               Tube/venturi/bottle standing on the ground:
                 plane XZ, points [radius, height], axis Z, origin [0,0,0].
                 u = radius (X), v = height (Z). Inner AND outer contours in one closed polyline
                 → hollow after revolve. Do not extrude a circle — that is a disk.
               WRONG: sketch on XY then revolve around Z (plane normal → disk).
               WRONG: sketch on XZ then revolve around Y (XZ normal → disk).
loft           { "op":"loft", "ruled": true, "sections": [ {"profile":<Profile>, "at":[x,y,z]}, ... ], "apex": [x,y,z] }
               ≥2 sections, OR 1 section plus apex. ruled:true = flat sides (square pyramid).
               Keep section XY at 0 and increase Z for a centered pyramid.
sweep          { "op":"sweep", "profile":<Profile>, "path": <Path>, "fuse": true }
               Profile may be omitted to sweep the last sketch.
pipe           { "op":"pipe", "diameter":<d>, "path": <Path>, "fuse": true }
               Path: { "polyline": { "points": [[x,y,z],...] } } (≥2 pts)
                  or { "helix": { "pitch":<p>, "height":<h>, "radius":<r>, "center":[x,y,z], "axis":"Z" } }
               (`at` is accepted as an alias for `center` on helix paths.)
helix          { "op":"helix", "pitch":<p>, "height":<h>, "radius":<r>, "diameter":<wire_d>,
                 "center":[x,y,z], "axis":"Z", "fuse": true }
               Spring / coil. `section_diameter` / `at` are aliases for diameter / center.
thicken        { "op":"thicken", "thickness":<t>, "face":"largest"|<index>, "fuse": true }
               Thickens the last sketch face, a selected solid face, or an existing shell/solid.

### Primitives (can be the FIRST feature — no sketch needed)
box       { "op":"box", "size":[dx,dy,dz], "at":[x,y,z], "centered": true }
          XY-centered by default; bottom sits on Z = at[2].
cylinder  { "op":"cylinder", "diameter":<d>, "height":<h>, "at":[x,y,z], "axis":"Z"|"X"|"Y" }
          Axis X and Y are valid (the kernel rotates a Z primitive). Do not avoid them.
sphere    { "op":"sphere", "diameter":<d>, "at":[x,y,z] }
cone      { "op":"cone", "d1":<base>, "d2":<top>, "height":<h>, "at":[x,y,z] }
          d2=0 is a pointed cone.
torus     { "op":"torus", "major":<R>, "minor":<r>, "at":[x,y,z] }
ellipsoid { "op":"ellipsoid", "radii":[rx,ry,rz], "at":[x,y,z] }
helix     { "op":"helix", "pitch":<p>, "height":<h>, "radius":<r>, "section_diameter":<d>, "at":[x,y,z], "axis":"Z" }
          Solid spring / coil (circular wire swept along a helix).

A coilover strut BODY is still stacked cylinders (tube, can, hat) along +Z that OVERLAP.
The spring itself MAY be a separate `helix` body. `at` on a cylinder is the BOTTOM.
Consecutive stacks: at.z = previous_bottom + previous_height - 2 (a few mm overlap).
Do not place the top hat at z=0. Plant the strut on the LCA mount with overlapping coordinates.

If the body already has a solid, a later box/cylinder/sphere/cone/torus/ellipsoid/helix/extrude is
JOINED (boolean union) into it. Use that for bosses, bushing eyes, bolt heads, ball-joint
studs. It does NOT replace the body. To subtract, use hole/cut/thread(internal). To make a separate part,
add a new body — do not start a second feature tree on the same body expecting a replace.
`fuse` as the FIRST feature of a body creates that solid (extruded profile). Prefer `box`
or `cylinder` when they fit; fuse-first is still valid.

### Threads (tap = internal, die = external)
ISO metric and unified inch. Size strings: "M8", "M8x1", "M10", "1/4-20", or #8-32.
Coarse pitch is filled in when omitted (M8 → 1.25 mm).

thread  { "op":"thread", "kind":"external"|"internal"|"die"|"tap", "size":"M8",
          "length":<mm>, "at":[x,y,z], "axis":"Z", "hand":"right"|"left" }
        EXTERNAL / DIE: first feature → threaded cylinder (bolt shank). On an existing
        solid → cuts a helical groove into a boss at `at` along `axis`.
        INTERNAL / TAP: needs an existing solid. Drills the tap hole and cuts the thread.
        Use hole-style placement: "center":[x,y], "plane":"XY", "through": true.
        Example M8 bolt shank 20 mm: { "op":"thread", "kind":"external", "size":"M8", "length":20 }
        Example M8 tapped hole: after a box, { "op":"thread", "kind":"tap", "size":"M8", "center":[0,0], "through":true }
        Do NOT fake threads with stacked toruses. Use this op.

### Booleans & holes
hole  { "op":"hole", "diameter":<d>, "depth":<h>, "center":[x,y], "plane":"XY",
        "face":"largest"|"top"|<index> }
      Through-hole by default. `depth`/`center` may be omitted (depth→1, center→[0,0]).
      Prefer `face` when drilling on a selected face. Set "through": false for a blind hole.
cut   { "op":"cut",  "profile":<Profile>, "depth":<h>, "at":[x,y,z], "plane":"XY",
        "face":"largest"|"top"|<index>, "through": true }
      Through-cut by default. `depth` may be omitted (defaults to 1). Use `face` to pocket on a solid face.
      Plane UV: XY=(X,Y), XZ=(X,Z), YZ=(Y,Z).
fuse  { "op":"fuse", "profile":<Profile>, "depth":<h>, "at":[x,y,z], "plane":"XY",
        "face":"largest"|"top"|<index> }
      Boss on a plane or on a selected face. If first feature on the body, it becomes the solid.
common { "op":"common", "profile":<Profile>, "depth":<h>, "at":[x,y,z], "plane":"XY" }
      Boolean intersection (keep only overlap with the extruded tool).

### Modify the current solid
fillet    { "op":"fillet", "radius":<r>, "edges":"all"|"top"|"longest"|[0,3,7] }
          `all`/`top` fillets the top perimeter of a plate (not thickness edges).
          r must be less than the local wall thickness.
chamfer   { "op":"chamfer", "distance":<d>, "angle":<deg>, "edges":"all"|"top"|"longest"|[0,3,7] }
          Optional angle (degrees) for a distance+angle chamfer.
transform { "op":"transform", "translate":[x,y,z], "rotate":{"axis":[x,y,z],"angle":<deg>,"origin":[x,y,z]}, "scale":<s> }
mirror    { "op":"mirror", "plane":"YZ"|"XZ"|"XY", "origin":[x,y,z], "fuse": true }
          YZ flips X, XZ flips Y, XY flips Z. fuse:true unions the copy with the original.
pattern   { "op":"pattern", "kind":"linear"|"circular", "count":<n≥2>, "spacing":<d>,
            "direction":[x,y,z], "axis":"Z", "angle":<deg>, "center":[x,y,z],
            "scope":"body"|"feature" }
          scope "body" (default) patterns the whole solid.
          scope "feature" re-applies the LAST cut/fuse/hole tool (bolt circles, hole grids).
          Example bolt circle: hole at [20,0], then
            { "op":"pattern", "scope":"feature", "kind":"circular", "count":6,
              "center":[0,0,0], "axis":"Z", "angle":60 }
shell     { "op":"shell", "thickness":<t>, "faces":"all"|[0]|"largest" }
offset    { "op":"offset", "distance":<d> }
          Grow (positive) or shrink (negative) the whole solid.
draft     { "op":"draft", "faces":"side"|[indices], "angle":<deg>, "direction":[0,0,1] }
          Draft existing side faces. Different from draft_extrude (which tapers a new prism).

## Topology for agents
Prefer semantic selectors over raw indices when possible:
  face: "largest" | "top" | "bottom" | <index>
  edges: "all" | "top" | "longest" | [indices]
Call /api/topology with the current program when fillets/faces fail; it returns tagged faces/edges.

## How to build common shapes
Stepped pyramid (square, stairs on every side, CENTERED):
  Prefer `box` + `fuse` on plane XY (omit plane). Stack with at [0,0,z].
  WRONG: `"plane": "XZ"` — that stands the layers up and boolean-unions them into a cross.
  WRONG: `"at": [-w/2, -h/2]` on a centered rect — that is the old corner convention.

Stepped pyramid with stairs on ONE side only:
  Centered stacked boxes, then fuse a staircase of boxes along -Y (or +Y) whose XY positions
  use negative coordinates as needed so the flight of stairs sits on one face.

Smooth square pyramid:
  loft a large centered square at z=0 to a tiny centered square (or apex) at z=height, ruled:true.
  Or: sketch a centered square then draft_extrude with a positive angle.

Smooth circular pyramid / cone:  { "op":"cone", "d1":100, "d2":0, "height":60 }

L-bracket / anything not a rectangle: use a closed polyline with negative AND positive points
  e.g. points [[0,0],[80,0],[80,10],[10,10],[10,50],[0,50]] then extrude. Center with transform if needed.

Hole grid on a plate: one hole, then pattern scope=feature linear/circular.
Pipe frame: pipe/sweep with a polyline path.
Flange with bolt holes in one sketch: compound outer rect + hole circles, then extrude.

## Example — venturi / lathe tube (half-section on XZ, revolve around Z)
{
  "units": "mm",
  "features": [
    { "op": "sketch", "plane": "XZ", "profile": { "polyline": { "closed": true, "points": [
      [20, 0], [20, 10], [12, 30], [12, 50], [20, 70], [20, 80],
      [14, 80], [14, 68], [8, 50], [8, 30], [14, 12], [14, 0]
    ] } } },
    { "op": "revolve", "axis": "Z", "angle": 360 },
    { "op": "cut", "plane": "YZ", "through": true, "depth": 10,
      "profile": { "circle": { "d": 3, "at": [0, 40] } } }
  ]
}

## Example — centered mounting plate with feature pattern
{
  "units": "mm",
  "features": [
    { "op": "box", "size": [80, 40, 10], "centered": true },
    { "op": "hole", "diameter": 8, "depth": 15, "center": [-25, 0] },
    { "op": "pattern", "scope": "feature", "kind": "linear", "count": 2,
      "spacing": 50, "direction": [1, 0, 0] },
    { "op": "fillet", "radius": 3, "edges": "top" }
  ]
}

## Example — centered stepped pyramid
{
  "units": "mm",
  "features": [
    { "op": "box", "size": [120, 120, 10], "at": [0, 0, 0], "centered": true },
    { "op": "fuse", "depth": 10, "at": [0, 0, 10], "profile": { "rect": { "w": 100, "h": 100, "centered": true } } },
    { "op": "fuse", "depth": 10, "at": [0, 0, 20], "profile": { "rect": { "w": 80, "h": 80, "centered": true } } },
    { "op": "fuse", "depth": 10, "at": [0, 0, 30], "profile": { "rect": { "w": 60, "h": 60, "centered": true } } },
    { "op": "fuse", "depth": 10, "at": [0, 0, 40], "profile": { "rect": { "w": 40, "h": 40, "centered": true } } },
    { "op": "fillet", "radius": 1, "edges": "all" }
  ]
}

## Example — M8 bolt shank (external thread / die)
{
  "units": "mm",
  "features": [
    { "op": "thread", "kind": "external", "size": "M8", "length": 24, "at": [0, 0, 0], "axis": "Z" },
    { "op": "cylinder", "diameter": 13, "height": 5.5, "at": [0, 0, 24] }
  ]
}

## Example — plate with M8 tapped hole
{
  "units": "mm",
  "features": [
    { "op": "box", "size": [40, 40, 12], "centered": true },
    { "op": "thread", "kind": "tap", "size": "M8", "center": [0, 0], "plane": "XY", "through": true }
  ]
}"#;

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    /// Zero-cost copy: the real kernel lives in a thread-local inside Engine.
    engine: Engine,
    /// Shared HTTP client (internally Arc-backed, cheap to clone).
    http: reqwest::Client,
    /// Gemini API key loaded from GEMINI_KEY env var.
    gemini_key: Arc<String>,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct RunRequest {
    #[serde(default)]
    program: Option<serde_json::Value>,
    #[serde(default)]
    document: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct RunResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh: Option<MeshPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<MetricsPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    verification: Option<VerificationPayload>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bodies: Vec<BodyPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BodyPayload {
    body_id: String,
    name: String,
    visible: bool,
    suppressed: bool,
    mesh: MeshPayload,
    metrics: MetricsPayload,
}

#[derive(Serialize)]
struct MeshPayload {
    /// Flat position array [x0,y0,z0, x1,y1,z1, …]
    positions: Vec<f32>,
    /// Per-vertex normals (same length as positions).
    normals: Vec<f32>,
    /// Triangle indices. Empty for non-indexed geometry.
    indices: Vec<u32>,
}

#[derive(Serialize)]
struct MetricsPayload {
    volume: f64,
    /// [xmin, ymin, zmin, xmax, ymax, zmax] in document units
    bbox: [f64; 6],
    surface_area: f64,
    is_solid: bool,
    /// Linear/volume values are expressed in these units (not cosmetic labels).
    units: String,
}

#[derive(Serialize)]
struct VerificationPayload {
    passed: bool,
    checks: Vec<VerificationCheckPayload>,
}

#[derive(Serialize)]
struct VerificationCheckPayload {
    name: String,
    passed: bool,
    message: String,
}

#[derive(Deserialize)]
struct ExportRequest {
    #[serde(default)]
    program: Option<serde_json::Value>,
    #[serde(default)]
    document: Option<serde_json::Value>,
    format: ExportFormat,
}

// ── Chat types ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    /// Prior conversation turns sent by the frontend for multi-turn context.
    #[serde(default)]
    history: Vec<HistoryMessage>,
  /// Current multi-body document so the agent can patch one body.
  #[serde(default)]
  document: Option<CadDocument>,
  #[serde(default, alias = "targetBodyId")]
  target_body_id: Option<String>,
  /// When the user scrubbed the design timeline, this is the active step index.
  #[serde(default, alias = "timelineStepIndex")]
  timeline_step_index: Option<u32>,
  #[serde(default, alias = "timelineStepLabel")]
  timeline_step_label: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
struct HistoryMessage {
    /// "user" | "assistant" (frontend convention; mapped to "model" for Gemini)
    role: String,
    content: String,
}

/// Events streamed to the designer over `text/event-stream`.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatSseEvent {
    ThinkingStart,
    ThinkingDelta { text: String },
    ThinkingDone { ms: u64 },
    WritingStart,
    WritingDone { ms: u64 },
    Repair { attempt: u32, error: String },
    CalculatingStart,
    CalculatingDone { ms: u64 },
    VerifyingStart,
    VerifyingDone {
        ms: u64,
        verification: VerificationPayload,
    },
    Result {
        success: bool,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        program: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mesh: Option<MeshPayload>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metrics: Option<MetricsPayload>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        bodies: Vec<BodyPayload>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        attempts: u32,
    },
}

type SseTx = tokio::sync::mpsc::Sender<Result<Event, Infallible>>;

// ── Gemini REST API types ─────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct GeminiRequest {
    system_instruction: GeminiSystemInstruction,
    contents: Vec<GeminiContent>,
    generation_config: GeminiGenerationConfig,
}

#[derive(Serialize, Clone)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct GeminiContent {
    #[serde(default)]
    role: String,
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct GeminiPart {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    text: String,
    /// Gemini marks thought-summary parts with `"thought": true`.
    #[serde(default, skip_serializing_if = "is_false")]
    thought: bool,
    #[serde(rename = "inline_data", skip_serializing_if = "Option::is_none")]
    inline_data: Option<GeminiInlineData>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct GeminiInlineData {
    mime_type: String,
    data: String,
}

#[derive(Serialize, Clone)]
struct GeminiGenerationConfig {
    temperature: f64,
    /// Ask Gemini to emit raw JSON (no markdown wrapper).
    response_mime_type: String,
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

#[derive(Serialize, Clone)]
struct ThinkingConfig {
    #[serde(rename = "includeThoughts")]
    include_thoughts: bool,
}

#[derive(Deserialize, Default)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
}

#[derive(Deserialize, Default)]
struct GeminiCandidate {
    #[serde(default)]
    content: GeminiContent,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn health_handler() -> &'static str {
    "ok"
}

/// POST /api/verify — deterministic structural checks (no LLM).
async fn verify_handler(
    Json(body): Json<RunRequest>,
) -> Json<serde_json::Value> {
    let document = match scene_from_values(body.document, body.program) {
        Ok(d) => d,
        Err(e) => {
            return Json(serde_json::json!({ "success": false, "error": e }));
        }
    };
    let engine = Engine::new();
    let document_for_kernel = document.clone();
    let result = tokio::task::spawn_blocking(move || engine.execute_document(&document_for_kernel))
        .await
        .unwrap_or_else(|e| {
            Err(kernel::engine::KernelError::InvalidState(format!(
                "verify task panicked: {e}"
            )))
        });
    match result {
        Ok(output) => {
            let report = verify::verify_structure(&document, &output);
            Json(serde_json::json!({
                "success": true,
                "passed": report.passed,
                "verification": verification_payload(&report),
                "metrics": metrics_payload(&output.metrics, &document.units),
            }))
        }
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

/// POST /api/topology — face/edge listing with semantic tags for the agent.
async fn topology_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RunRequest>,
) -> Json<serde_json::Value> {
    let document = match scene_from_values(body.document, body.program) {
        Ok(d) => d,
        Err(e) => {
            return Json(serde_json::json!({ "success": false, "error": e }));
        }
    };
    let Some(body0) = document.bodies.first() else {
        return Json(serde_json::json!({ "success": false, "error": "empty document" }));
    };
    let program = kernel::ir::CadProgram {
        units: document.units.clone(),
        features: body0.features.clone(),
    };
    let engine = state.engine;
    let result = tokio::task::spawn_blocking(move || engine.list_topology(&program))
        .await
        .unwrap_or_else(|e| {
            Err(kernel::engine::KernelError::InvalidState(format!(
                "topology task panicked: {e}"
            )))
        });
    match result {
        Ok(report) => Json(serde_json::json!({ "success": true, "topology": report })),
        Err(e) => Json(serde_json::json!({ "success": false, "error": e.to_string() })),
    }
}

/// POST /api/run
///
/// Body:  `{ "document": <CadDocument> }` or `{ "program": <CadProgram|CadDocument> }`
async fn run_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RunRequest>,
) -> Json<RunResponse> {
    let document = match scene_from_values(body.document, body.program) {
        Ok(d) => d,
        Err(e) => {
            return Json(RunResponse {
                success: false,
                mesh: None,
                metrics: None,
                verification: None,
                bodies: vec![],
                error: Some(e),
            })
        }
    };

    let engine = state.engine;
    let units = document.units;
    let started = Instant::now();
    let document_for_kernel = document.clone();
    let result = tokio::task::spawn_blocking(move || engine.execute_document(&document_for_kernel))
        .await
        .unwrap_or_else(|e| {
            Err(kernel::engine::KernelError::InvalidState(format!(
                "blocking task panicked: {e}"
            )))
        });
    tracing::info!(
        "POST /api/run compute {:.2}s",
        started.elapsed().as_secs_f64()
    );

    match result {
        Ok(output) => {
            let report = verify::verify_structure(&document, &output);
            Json(document_run_response(output, &units, Some(&report)))
        }
        Err(e) => Json(RunResponse {
            success: false,
            mesh: None,
            metrics: None,
            verification: None,
            bodies: vec![],
            error: Some(e.to_string()),
        }),
    }
}

/// POST /api/export
async fn export_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ExportRequest>,
) -> Response {
    let document = match scene_from_values(body.document, body.program) {
        Ok(d) => d,
        Err(e) => {
            let body = serde_json::json!({ "error": e }).to_string();
            return Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
        }
    };
    let engine = state.engine;
    let format = body.format;
    let ext = format.extension();
    let mime = format.mime();

    let result = tokio::task::spawn_blocking(move || engine.export_document(&document, &format))
        .await
        .unwrap_or_else(|e| {
            Err(kernel::engine::KernelError::InvalidState(format!(
                "blocking task panicked: {e}"
            )))
        });

    match result {
        Ok(data) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"model.{ext}\""),
            )
            .body(Body::from(Bytes::from(data)))
            .unwrap(),
        Err(e) => {
            let body = serde_json::json!({ "error": e.to_string() }).to_string();
            Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
    }
}

/// POST /api/chat — Server-Sent Events stream of thinking / writing / kernel work.
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequest>,
) -> Sse<tokio_stream::wrappers::ReceiverStream<Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(async move {
        run_chat_session(state, body, tx).await;
    });
    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

async fn emit(tx: &SseTx, ev: ChatSseEvent) {
    let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".to_string());
    let _ = tx.send(Ok(Event::default().data(data))).await;
}

async fn emit_fail(tx: &SseTx, message: impl Into<String>, error: impl Into<String>, attempts: u32) {
    emit(
        tx,
        ChatSseEvent::Result {
            success: false,
            message: message.into(),
            program: None,
            mesh: None,
            metrics: None,
            bodies: vec![],
            error: Some(error.into()),
            attempts,
        },
    )
    .await;
}

fn gemini_text(text: impl Into<String>) -> GeminiPart {
    GeminiPart {
        text: text.into(),
        thought: false,
        inline_data: None,
    }
}

fn gemini_png(png: &[u8]) -> GeminiPart {
    GeminiPart {
        text: String::new(),
        thought: false,
        inline_data: Some(GeminiInlineData {
            mime_type: "image/png".into(),
            data: preview::to_base64(png),
        }),
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

async fn run_chat_session(state: Arc<AppState>, body: ChatRequest, tx: SseTx) {
    if state.gemini_key.is_empty() {
        emit_fail(
            &tx,
            "Server configuration error: GEMINI_KEY is not set.",
            "Set GEMINI_KEY in the .env file and restart the server.",
            0,
        )
        .await;
        return;
    }

    let mut contents: Vec<GeminiContent> = body
        .history
        .iter()
        .filter(|h| !h.content.trim().is_empty())
        .map(|h| GeminiContent {
            role: if h.role == "user" {
                "user".to_string()
            } else {
                "model".to_string()
            },
            parts: vec![gemini_text(h.content.clone())],
        })
        .collect();

    let user_text = compose_user_prompt(&body);
    contents.push(GeminiContent {
        role: "user".to_string(),
        parts: vec![gemini_text(user_text)],
    });

    const MAX_ATTEMPTS: u32 = 6;
    let mut last_error = String::from("Unknown error");

    for attempt in 1..=MAX_ATTEMPTS {
        let req_body = GeminiRequest {
            system_instruction: GeminiSystemInstruction {
                parts: vec![gemini_text(SYSTEM_PROMPT)],
            },
            contents: contents.clone(),
            generation_config: GeminiGenerationConfig {
                temperature: 0.1,
                response_mime_type: "application/json".to_string(),
                thinking_config: Some(ThinkingConfig {
                    include_thoughts: true,
                }),
            },
        };

        let model_text = match stream_gemini(&state, &req_body, &tx).await {
            Ok(text) => text,
            Err(e) => {
                last_error = e;
                tracing::error!(attempt, %last_error);
                break;
            }
        };

        tracing::info!(attempt, chars = model_text.len(), "Gemini stream finished");

        let json_text = extract_json(&model_text);

        let (document, say) = match parse_agent_payload(&json_text, body.document.as_ref()) {
            Ok(v) => v,
            Err(parse_err) => {
                last_error = format!("JSON parse error: {parse_err}");
                tracing::warn!(attempt, %last_error, "repair loop");
                emit(
                    &tx,
                    ChatSseEvent::Repair {
                        attempt,
                        error: last_error.clone(),
                    },
                )
                .await;
                let repair = format!(
                    "Your response could not be parsed. {parse_err}. \
                     Return {{ \"say\": \"...\", \"document\": <CadDocument> }} or \
                     {{ \"say\": \"...\", \"body\": <CadBody> }} — no markdown."
                );
                contents.push(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![gemini_text(model_text)],
                });
                contents.push(GeminiContent {
                    role: "user".to_string(),
                    parts: vec![gemini_text(repair)],
                });
                continue;
            }
        };

        if let Err(val_err) = document.validate() {
            last_error = format!("Validation error: {val_err}");
            tracing::warn!(attempt, %last_error, "repair loop");
            emit(
                &tx,
                ChatSseEvent::Repair {
                    attempt,
                    error: last_error.clone(),
                },
            )
            .await;
            let repair = format!(
                "The document failed AgentCAD validation: {val_err}. \
                 Fix it and return ONLY the corrected JSON object."
            );
            contents.push(GeminiContent {
                role: "model".to_string(),
                parts: vec![gemini_text(model_text)],
            });
            contents.push(GeminiContent {
                role: "user".to_string(),
                parts: vec![gemini_text(repair)],
            });
            continue;
        }

        emit(&tx, ChatSseEvent::CalculatingStart).await;
        let calc_start = Instant::now();
        let engine = state.engine;
        let document_for_kernel = document.clone();
        let kernel_result =
            tokio::task::spawn_blocking(move || engine.execute_document(&document_for_kernel))
                .await
                .unwrap_or_else(|e| {
                    Err(kernel::engine::KernelError::InvalidState(format!(
                        "kernel task panicked: {e}"
                    )))
                });
        emit(
            &tx,
            ChatSseEvent::CalculatingDone {
                ms: elapsed_ms(calc_start),
            },
        )
        .await;

        match kernel_result {
            Ok(output) => {
                tracing::info!(attempt, "Chat: geometry generated successfully");

                let quality = preview::quality_notes(&output);
                let preview_png = preview::render_png(&output);
                let quality_text = preview::quality_report(&quality);

                emit(&tx, ChatSseEvent::VerifyingStart).await;
                let verify_start = Instant::now();
                let report = verify::verify_document(&body.message, &document, &output);
                emit(
                    &tx,
                    ChatSseEvent::VerifyingDone {
                        ms: elapsed_ms(verify_start),
                        verification: verification_payload(&report),
                    },
                )
                .await;
                let mut verdict = verify_against_report(
                    &state,
                    &body.message,
                    &document,
                    &output,
                    &report,
                    &quality_text,
                    preview_png.as_deref(),
                )
                .await;
                if let Some(local) = preview::reject_reason(&document, &output) {
                    if matches!(
                        verdict,
                        VerifyVerdict::Ok { .. } | VerifyVerdict::Skipped { .. }
                    ) {
                        tracing::warn!("verify accepted an unfinished assembly; forcing repair");
                        verdict = VerifyVerdict::Mismatch {
                            reason: format!(
                                "The model is not finished:\n{local}\n\
                                 Keep going. Mate every joint, fillet structural parts, and put the \
                                 strut on its mount. Return a corrected document — do not describe \
                                 an assembly you did not build."
                            ),
                            document: None,
                        };
                    }
                }

                match verdict {
                    VerifyVerdict::Mismatch { reason, document: Some(fixed) } => {
                        last_error = format!("Result did not match the request: {reason}");
                        tracing::warn!(attempt, %last_error, "verify rejected geometry");
                        emit(
                            &tx,
                            ChatSseEvent::Repair {
                                attempt,
                                error: last_error.clone(),
                            },
                        )
                        .await;
                        if let Err(val_err) = fixed.validate() {
                            contents.push(GeminiContent {
                                role: "model".to_string(),
                                parts: vec![gemini_text(model_text)],
                            });
                            contents.push(GeminiContent {
                                role: "user".to_string(),
                                parts: vec![gemini_text(format!(
                                    "The solid does not match the user request: {reason}. \
                                     Your corrected document failed validation: {val_err}. \
                                     Return a valid {{ \"say\", \"document\" }} JSON object."
                                ))],
                            });
                            continue;
                        }
                        emit(&tx, ChatSseEvent::CalculatingStart).await;
                        let calc_start = Instant::now();
                        let engine = state.engine;
                        let document_for_kernel = fixed.clone();
                        let retry = tokio::task::spawn_blocking(move || {
                            engine.execute_document(&document_for_kernel)
                        })
                        .await
                        .unwrap_or_else(|e| {
                            Err(kernel::engine::KernelError::InvalidState(format!(
                                "kernel task panicked: {e}"
                            )))
                        });
                        emit(
                            &tx,
                            ChatSseEvent::CalculatingDone {
                                ms: elapsed_ms(calc_start),
                            },
                        )
                        .await;
                        match retry {
                            Ok(output) => {
                                if let Some(local) = preview::reject_reason(&fixed, &output) {
                                    last_error = format!(
                                        "Corrected document still unfinished: {reason}\n{local}"
                                    );
                                    tracing::warn!(attempt, %last_error, "verify fix still incomplete");
                                    emit(
                                        &tx,
                                        ChatSseEvent::Repair {
                                            attempt,
                                            error: last_error.clone(),
                                        },
                                    )
                                    .await;
                                    let retry_png = preview::render_png(&output);
                                    contents.push(GeminiContent {
                                        role: "model".to_string(),
                                        parts: vec![gemini_text(model_text)],
                                    });
                                    contents.push(GeminiContent {
                                        role: "user".to_string(),
                                        parts: {
                                            let mut parts = vec![gemini_text(format!(
                                                "You returned a 'fixed' document but it is still not done.\n\
                                                 {local}\nLook at the attached render. Mate the knuckle to both \
                                                 ball joints, plant the strut on the LCA, and fillet the arms. \
                                                 Return {{ \"say\", \"document\" }}."
                                            ))];
                                            if let Some(png) = retry_png.as_deref() {
                                                parts.push(gemini_png(png));
                                            }
                                            parts
                                        },
                                    });
                                    continue;
                                }
                                let program_val =
                                    serde_json::to_value(&fixed).unwrap_or_default();
                                let message = say
                                    .clone()
                                    .filter(|s| !s.trim().is_empty())
                                    .unwrap_or_else(|| {
                                        "Updated the model after checking it against your request."
                                            .to_string()
                                    });
                                emit_success(
                                    &tx,
                                    message,
                                    program_val,
                                    output,
                                    &fixed.units,
                                    attempt,
                                )
                                .await;
                                return;
                            }
                            Err(kern_err) => {
                                last_error = format!(
                                    "Kernel error on corrected document: {}",
                                    kernel_error_for_model(&kern_err)
                                );
                                contents.push(GeminiContent {
                                    role: "model".to_string(),
                                    parts: vec![gemini_text(model_text)],
                                });
                                contents.push(GeminiContent {
                                    role: "user".to_string(),
                                    parts: vec![gemini_text(format!(
                                        "The solid did not match the request ({reason}) and the \
                                         corrected document failed: {}. \
                                         Return a valid {{ \"say\", \"document\" }} JSON object. \
                                         Do not drop body rotation or X/Y cylinders — those ops are valid. \
                                         Start each body with box/cylinder/sketch+extrude/fuse.",
                                        kernel_error_for_model(&kern_err)
                                    ))],
                                });
                                continue;
                            }
                        }
                    }
                    VerifyVerdict::Mismatch { reason, document: None } => {
                        last_error = format!("Result did not match the request: {reason}");
                        tracing::warn!(attempt, %last_error, "verify rejected geometry");
                        emit(
                            &tx,
                            ChatSseEvent::Repair {
                                attempt,
                                error: last_error.clone(),
                            },
                        )
                        .await;
                        contents.push(GeminiContent {
                            role: "model".to_string(),
                            parts: vec![gemini_text(model_text)],
                        });
                        contents.push(GeminiContent {
                            role: "user".to_string(),
                            parts: {
                                let mut parts = vec![gemini_text(format!(
                                    "The kernel built solids, but they do NOT look like what the user asked for. {reason}\n\
                                     Mesh quality:\n{quality_text}\n\
                                     Bounding box {bbox:?}, volume {vol:.1}.\n\
                                     Look at the attached isometric render. Fix jagged/self-intersecting \
                                     control arms, disconnected primitive bags, and missing fillets. \
                                     Return {{ \"say\", \"document\" }}.",
                                    bbox = output.metrics.bbox,
                                    vol = output.metrics.volume,
                                ))];
                                if let Some(png) = preview_png.as_deref() {
                                    parts.push(gemini_png(png));
                                }
                                parts
                            },
                        });
                        continue;
                    }
                    VerifyVerdict::Ok { say: verified_say }
                    | VerifyVerdict::Skipped { say: verified_say } => {
                        let program_val = serde_json::to_value(&document).unwrap_or_default();
                        let message = verified_say
                            .or(say)
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| "Updated the model.".to_string());
                        emit_success(
                            &tx,
                            message,
                            program_val,
                            output,
                            &document.units,
                            attempt,
                        )
                        .await;
                        return;
                    }
                }
            }
            Err(kern_err) => {
                last_error = format!("Kernel error: {}", kernel_error_for_model(&kern_err));
                tracing::warn!(attempt, %last_error, "repair loop");
                emit(
                    &tx,
                    ChatSseEvent::Repair {
                        attempt,
                        error: last_error.clone(),
                    },
                )
                .await;
                let topo_hint = topology_hint_for_document(&state.engine, &document).await;
                let repair = format!(
                    "The geometry kernel rejected the program: {}.                      Prefer face:\"largest\"|\"top\"|\"bottom\" and edges:\"top\"|\"longest\".                      For hole grids use pattern with scope:\"feature\" after the first hole.                      Body rotation (including Y) and cylinders on X/Y are valid.                      Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.                      Use cut/hole only after a solid exists.                      {}Fix the CadDocument and return ONLY the corrected JSON object.",
                    kernel_error_for_model(&kern_err),
                    topo_hint
                );
                contents.push(GeminiContent {
                    role: "model".to_string(),
                    parts: vec![gemini_text(model_text)],
                });
                contents.push(GeminiContent {
                    role: "user".to_string(),
                    parts: vec![gemini_text(repair)],
                });
                continue;
            }
        }
    }

    tracing::error!(%last_error, "Chat: all repair attempts exhausted");
    emit_fail(
        &tx,
        format!("Could not generate a valid model after {MAX_ATTEMPTS} attempts."),
        last_error,
        MAX_ATTEMPTS,
    )
    .await;
}

async fn emit_success(
    tx: &SseTx,
    message: String,
    program_val: serde_json::Value,
    output: DocumentOutput,
    units: &Units,
    attempts: u32,
) {
    let combined = output.clone().into_model_output().ok();
    emit(
        tx,
        ChatSseEvent::Result {
            success: true,
            message,
            program: Some(program_val),
            mesh: combined.as_ref().map(|o| mesh_payload(&o.mesh)),
            metrics: Some(metrics_payload(&output.metrics, units)),
            bodies: body_payloads(&output, units),
            error: None,
            attempts,
        },
    )
    .await;
}

enum VerifyVerdict {
    Ok { say: Option<String> },
    Skipped { say: Option<String> },
    Mismatch {
        reason: String,
        document: Option<CadDocument>,
    },
}

#[derive(Deserialize)]
struct VerifyJson {
    ok: bool,
    #[serde(default)]
    say: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    program: Option<serde_json::Value>,
    #[serde(default)]
    document: Option<serde_json::Value>,
    #[serde(default)]
    body: Option<serde_json::Value>,
}

/// Deterministic checks first; optional Gemini (+ isometric preview) afterwards.
async fn verify_against_report(
    state: &AppState,
    user_message: &str,
    document: &CadDocument,
    output: &DocumentOutput,
    report: &VerificationReport,
    quality_text: &str,
    preview_png: Option<&[u8]>,
) -> VerifyVerdict {
    if !report.passed {
        tracing::warn!("deterministic verify failed: {}", report.summary());
        return VerifyVerdict::Mismatch {
            reason: format!("Verification failed: {}", report.summary()),
            document: None,
        };
    }

    let metrics = &output.metrics;
    let [xmin, ymin, zmin, xmax, ymax, zmax] = metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    let units = document.units.as_str();
    let program_json = serde_json::to_string_pretty(document).unwrap_or_default();
    let n_bodies = document.bodies.len();

    let prompt = format!(
        "The user asked:\n{user_message}\n\n\
         Deterministic verification PASSED for this solid ({units}):\n\
         - bbox [{xmin:.2}, {ymin:.2}, {zmin:.2}, {xmax:.2}, {ymax:.2}, {zmax:.2}] {units}\n\
         - extents {dx:.2}×{dy:.2}×{dz:.2} {units}\n\
         - volume = {vol:.2} {units}³\n\
         - surface_area = {area:.2}\n\n\
         You produced this CadDocument ({n_bodies} bodies):\n{program_json}\n\n\
         Per-body mesh quality:\n{quality_text}\n\n\
         An isometric render of the ACTUAL tessellated solids is attached. Look at it.\n\
         Does this match what the user asked for AS REAL CAD PARTS?\n\
         Rules:\n\
         - Assemblies should be multiple bodies, not one fused blob.\n\
         - A tube/venturi along Z must have similar dx and dy AND a real dz (not a disk).\n\
         - Volume must be clearly > 0.\n\
         - Per-body mesh shells: OCCT counts faces, not parts. shells=20–80 on one body is normal \
           tessellation. Only reject a body if the RENDER shows a bag of floating boxes/cylinders \
           that do not touch. Do not set ok:false just because the shell number is > 1.\n\
         - Structural parts should look assembled: knuckle between the arms, ball joints in their \
           cups, strut standing on the LCA pad with the top hat at the TOP.\n\
         - ACCEPT a complete first-pass multi-body suspension that is mated, even if proportions \
           are approximate, a coilover spring is a helix or stacked rings, or parts mildly clip. \
           Do not reject for 'awkward clipping', 'incorrectly proportioned', or missing fillets.\n\
         - REJECT only if major parts are missing, the layout is exploded (parts floating apart), \
           or the render is clearly disconnected primitives / a self-intersecting scribble.\n\
         Reply with JSON only:\n\
         {{ \"ok\": true, \"say\": \"<2-4 sentence description>\" }}\n\
         or\n\
         {{ \"ok\": false, \"reason\": \"<what's wrong>\", \"say\": \"...\", \"document\": {{ ...fixed CadDocument }} }}",
        vol = metrics.volume,
        area = metrics.surface_area,
    );

    let mut parts = vec![gemini_text(prompt)];
    if let Some(png) = preview_png {
        parts.push(gemini_png(png));
    }

    let req_body = GeminiRequest {
        system_instruction: GeminiSystemInstruction {
            parts: vec![gemini_text(SYSTEM_PROMPT)],
        },
        contents: vec![GeminiContent {
            role: "user".to_string(),
            parts,
        }],
        generation_config: GeminiGenerationConfig {
            temperature: 0.0,
            response_mime_type: "application/json".to_string(),
            thinking_config: None,
        },
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        GEMINI_MODEL, state.gemini_key
    );

    let http_resp = match state.http.post(&url).json(&req_body).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("verify say call failed: {e}");
            return VerifyVerdict::Skipped { say: None };
        }
    };
    if !http_resp.status().is_success() {
        tracing::warn!("verify Gemini status {}", http_resp.status());
        return VerifyVerdict::Skipped { say: None };
    }
    let parsed: GeminiResponse = match http_resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("verify deserialize failed: {e}");
            return VerifyVerdict::Skipped { say: None };
        }
    };
    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().find(|p| !p.thought && !p.text.is_empty()))
        .map(|p| p.text)
        .unwrap_or_default();
    let json_text = extract_json(&text);
    let v: VerifyJson = match serde_json::from_str(&json_text) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("verify JSON parse failed: {e}");
            return VerifyVerdict::Skipped { say: None };
        }
    };

    if v.ok {
        VerifyVerdict::Ok {
            say: v.say.filter(|s| !s.trim().is_empty()),
        }
    } else {
        let reason = v
            .reason
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "geometry does not match the request".to_string());
        let fixed = v
            .document
            .or(v.program)
            .and_then(|val| CadDocument::from_json_value(val).ok())
            .or_else(|| {
                v.body.and_then(|b| {
                    serde_json::from_value::<CadBody>(b).ok().map(|body| {
                        let mut d = document.clone();
                        d.replace_body(body);
                        d
                    })
                })
            });
        VerifyVerdict::Mismatch {
            reason,
            document: fixed,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StreamPhase {
    Idle,
    Writing,
}

async fn stream_gemini(
    state: &AppState,
    req_body: &GeminiRequest,
    tx: &SseTx,
) -> Result<String, String> {
    match stream_gemini_once(state, req_body, tx).await {
        Ok(text) => Ok(text),
        Err(e) if e.contains("400") || e.to_lowercase().contains("thinking") => {
            tracing::warn!("Gemini stream failed with thinkingConfig, retrying without it: {e}");
            let mut fallback = req_body.clone();
            fallback.generation_config.thinking_config = None;
            stream_gemini_once(state, &fallback, tx).await
        }
        Err(e) => Err(e),
    }
}

async fn stream_gemini_once(
    state: &AppState,
    req_body: &GeminiRequest,
    tx: &SseTx,
) -> Result<String, String> {
    use futures_util::StreamExt;

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
        GEMINI_MODEL, state.gemini_key
    );

    let http_resp = state
        .http
        .post(&url)
        .header(header::ACCEPT, "text/event-stream")
        .json(req_body)
        .send()
        .await
        .map_err(|e| format!("HTTP request to Gemini failed: {e}"))?;

    if !http_resp.status().is_success() {
        let status = http_resp.status();
        let body_text = http_resp.text().await.unwrap_or_default();
        return Err(format!("Gemini returned {status}: {body_text}"));
    }

    emit(tx, ChatSseEvent::ThinkingStart).await;
    let think_start = Instant::now();
    let mut write_start: Option<Instant> = None;
    let mut phase = StreamPhase::Idle;
    let mut output = String::new();
    let mut buf = String::new();
    let mut stream = http_resp.bytes_stream();

    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("Gemini stream error: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&bytes));

        loop {
            let sep = buf
                .find("\r\n\r\n")
                .map(|p| (p, 4))
                .or_else(|| buf.find("\n\n").map(|p| (p, 2)));
            let Some((pos, seplen)) = sep else { break };
            let event = buf[..pos].to_string();
            buf.drain(..pos + seplen);

            for line in event.lines() {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                let parsed: GeminiResponse = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for cand in parsed.candidates {
                    for part in cand.content.parts {
                        if part.text.is_empty() {
                            continue;
                        }
                        if part.thought {
                            if phase == StreamPhase::Idle {
                                emit(
                                    tx,
                                    ChatSseEvent::ThinkingDelta {
                                        text: part.text.clone(),
                                    },
                                )
                                .await;
                            }
                        } else {
                            if phase != StreamPhase::Writing {
                                emit(
                                    tx,
                                    ChatSseEvent::ThinkingDone {
                                        ms: elapsed_ms(think_start),
                                    },
                                )
                                .await;
                                emit(tx, ChatSseEvent::WritingStart).await;
                                write_start = Some(Instant::now());
                                phase = StreamPhase::Writing;
                            }
                            output.push_str(&part.text);
                        }
                    }
                }
            }
        }
    }

    if phase != StreamPhase::Writing {
        emit(
            tx,
            ChatSseEvent::ThinkingDone {
                ms: elapsed_ms(think_start),
            },
        )
        .await;
        emit(tx, ChatSseEvent::WritingStart).await;
        write_start = Some(Instant::now());
    }

    emit(
        tx,
        ChatSseEvent::WritingDone {
            ms: write_start.map(elapsed_ms).unwrap_or(0),
        },
    )
    .await;

    if output.is_empty() {
        return Err("Gemini returned an empty stream".to_string());
    }
    Ok(output)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Strip markdown code fences and isolate the outermost JSON object.
/// Handles both ` ```json … ``` ` and ` ``` … ``` ` wrapping.
fn extract_json(text: &str) -> String {
    let text = text.trim();
    let inner = if let Some(s) = text.strip_prefix("```json") {
        s.trim_end_matches("```").trim()
    } else if let Some(s) = text.strip_prefix("```") {
        s.trim_end_matches("```").trim()
    } else {
        text
    };
    // Find the outermost { … } in case any prose precedes/follows the object.
    if let (Some(start), Some(end)) = (inner.find('{'), inner.rfind('}')) {
        return inner[start..=end].to_string();
    }
    inner.to_string()
}

/// Strip wasm backtraces so the model does not "learn" that Y-rotation is illegal.
fn kernel_error_for_model(err: &kernel::engine::KernelError) -> String {
    let raw = err.to_string();
    let lower = raw.to_lowercase();
    if lower.contains("internal cad kernel crash")
        || lower.contains("out of bounds")
        || lower.contains("wasm trap")
        || lower.contains("wasm runtime")
        || lower.contains("memory fault")
    {
        format!(
            "{raw} The kernel recovers after a crash; keep rotation and X/Y cylinders."
        )
    } else {
        raw
    }
}

/// Accept `{ document }`, `{ body }` (patch), `{ program }`, or a raw document/program.
fn parse_agent_payload(
    json_text: &str,
    current: Option<&CadDocument>,
) -> Result<(CadDocument, Option<String>), String> {
    let value: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| e.to_string())?;
    let say = value
        .get("say")
        .and_then(|s| s.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(doc_val) = value.get("document").cloned() {
        return Ok((CadDocument::from_json_value(doc_val)?, say));
    }
    if let Some(body_val) = value.get("body").cloned() {
        let patch: CadBody = serde_json::from_value(body_val).map_err(|e| e.to_string())?;
        let mut doc = current.cloned().unwrap_or_else(|| CadDocument {
            document_id: "document".into(),
            units: kernel::ir::Units::Mm,
            parameters: Default::default(),
            bodies: vec![],
        });
        doc.replace_body(patch);
        return Ok((doc, say));
    }
    if let Some(prog_val) = value.get("program").cloned() {
        return Ok((CadDocument::from_json_value(prog_val)?, say));
    }
    Ok((CadDocument::from_json_value(value)?, say))
}

fn compose_user_prompt(body: &ChatRequest) -> String {
    let mut text = body.message.clone();
    if let (Some(idx), Some(label)) = (body.timeline_step_index, body.timeline_step_label.as_deref()) {
        text.push_str(&format!(
            "\n\n[AgentCAD] The user is viewing design history step {idx} (\"{label}\"). \
             Edit THIS document state; later timeline steps will be discarded after your change.\n"
        ));
    }
    let Some(doc) = body.document.as_ref() else {
        return text;
    };
    if let Some(tid) = body.target_body_id.as_deref() {
        if let Some(target) = doc.bodies.iter().find(|b| b.body_id == tid) {
            let others: Vec<String> = doc
                .bodies
                .iter()
                .filter(|b| b.body_id != tid)
                .map(|b| format!("{} ({})", b.body_id, b.display_name()))
                .collect();
            text.push_str("\n\n[AgentCAD] Edit ONLY body `");
            text.push_str(tid);
            text.push_str("` (");
            text.push_str(target.display_name());
            text.push_str("). Return { \"say\", \"body\": { ... } } with this bodyId.\n");
            if let Ok(json) = serde_json::to_string_pretty(target) {
                text.push_str(&json);
            }
            if !others.is_empty() {
                text.push_str("\nOther bodies (do not emit them): ");
                text.push_str(&others.join(", "));
            }
            return text;
        }
    }
    text.push_str(
        "\n\n[AgentCAD] Current document. Keep unrelated bodies intact; add/split bodies as needed.\n",
    );
    if let Ok(json) = serde_json::to_string_pretty(doc) {
        text.push_str(&json);
    }
    text
}

/// Best-effort topology summary for repair prompts (prefix of features that still build).
async fn topology_hint_for_document(engine: &Engine, document: &CadDocument) -> String {
    let Some(body) = document.bodies.first() else {
        return String::new();
    };
    if body.features.len() < 2 {
        return String::new();
    }
    // Drop the last feature — often the failing fillet/pattern — and query topology.
    let mut features = body.features.clone();
    features.pop();
    let program = kernel::ir::CadProgram {
        units: document.units.clone(),
        features,
    };
    let engine = *engine;
    let report = tokio::task::spawn_blocking(move || engine.list_topology(&program))
        .await
        .ok()
        .and_then(|r| r.ok());
    match report {
        Some(t) => format!(
            "Topology hint (before last feature): faces={} edges={} largest_face={:?} top_face={:?} longest_edge={:?}. ",
            t.summary.face_count,
            t.summary.edge_count,
            t.summary.largest_face,
            t.summary.top_face,
            t.summary.longest_edge
        ),
        None => String::new(),
    }
}

fn scene_from_values(
    document: Option<serde_json::Value>,
    program: Option<serde_json::Value>,
) -> Result<CadDocument, String> {
    let v = document
        .or(program)
        .ok_or_else(|| "missing document or program".to_string())?;
    CadDocument::from_json_value(v)
}

fn mesh_payload(mesh: &kernel::engine::MeshData) -> MeshPayload {
    MeshPayload {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        indices: mesh.indices.clone(),
    }
}

fn metrics_payload(m: &MetricsData, units: &Units) -> MetricsPayload {
    MetricsPayload {
        volume: m.volume,
        bbox: m.bbox,
        surface_area: m.surface_area,
        is_solid: m.is_solid,
        units: units.as_str().to_string(),
    }
}

fn verification_payload(report: &VerificationReport) -> VerificationPayload {
    VerificationPayload {
        passed: report.passed,
        checks: report
            .checks
            .iter()
            .map(|c| VerificationCheckPayload {
                name: c.name.clone(),
                passed: c.passed,
                message: c.message.clone(),
            })
            .collect(),
    }
}

fn body_payloads(out: &DocumentOutput, units: &Units) -> Vec<BodyPayload> {
    out.bodies
        .iter()
        .map(|b| BodyPayload {
            body_id: b.body_id.clone(),
            name: b.name.clone(),
            visible: b.visible,
            suppressed: b.suppressed,
            mesh: mesh_payload(&b.mesh),
            metrics: metrics_payload(&b.metrics, units),
        })
        .collect()
}

fn document_run_response(
    output: DocumentOutput,
    units: &Units,
    verification: Option<&VerificationReport>,
) -> RunResponse {
    let combined = output.clone().into_model_output().ok();
    RunResponse {
        success: true,
        mesh: combined.as_ref().map(|o| mesh_payload(&o.mesh)),
        metrics: Some(metrics_payload(&output.metrics, units)),
        verification: verification.map(verification_payload),
        bodies: body_payloads(&output, units),
        error: None,
    }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Load .env from the workspace root (silently ignored if the file is absent).
    dotenvy::dotenv().ok();

    let gemini_key = std::env::var("GEMINI_KEY").unwrap_or_default();
    if gemini_key.is_empty() {
        tracing::warn!("GEMINI_KEY not set — /api/chat will return a configuration error");
    } else {
        let preview = &gemini_key[..8.min(gemini_key.len())];
        tracing::info!("GEMINI_KEY loaded ({preview}…)");
    }

    let engine = Engine::new();
    tracing::info!("warming OCCT WASM kernel (first process start compiles ~21 MB; later starts use disk cache)...");
    let warmup_started = Instant::now();
    match engine.warmup() {
        Ok(()) => tracing::info!(
            "OCCT kernel ready in {:.2}s",
            warmup_started.elapsed().as_secs_f64()
        ),
        Err(e) => tracing::error!("OCCT warmup failed: {e}"),
    }

    let state = Arc::new(AppState {
        engine,
        http: reqwest::Client::new(),
        gemini_key: Arc::new(gemini_key),
    });

    let cors = CorsLayer::new()
        .allow_origin(
            "http://localhost:5173"
                .parse::<axum::http::HeaderValue>()
                .unwrap(),
        )
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/run", post(run_handler))
        .route("/api/topology", post(topology_handler))
        .route("/api/verify", post(verify_handler))
        .route("/api/export", post(export_handler))
        .route("/api/chat", post(chat_handler))
        .layer(cors)
        .with_state(state);

    let addr = "127.0.0.1:3001";
    tracing::info!("AgentCAD server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
