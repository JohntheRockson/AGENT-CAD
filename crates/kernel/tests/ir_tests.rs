//! Integration tests for the kernel crate.
//!
//! These tests run with default features (no `occt`), so they always use the
//! mock backend. They verify:
//!  1. JSON round-trips correctly for all op types.
//!  2. A mounting bracket produces valid geometry (positions, normals, volume > 0).
//!  3. Invalid IR is rejected before touching geometry.

use kernel::{
    engine::{Engine, KernelError},
    ir::*,
    CadProgram,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse(json: &str) -> CadProgram {
    serde_json::from_str(json).expect("JSON parse failed")
}

fn bracket() -> CadProgram {
    parse(
        r#"{
            "units": "mm",
            "features": [
                { "op": "sketch", "id": "base", "plane": "XY",
                  "profile": { "rect": { "w": 60.0, "h": 40.0 } } },
                { "op": "extrude", "id": "body", "depth": 8.0 },
                { "op": "hole",    "diameter": 8.0, "depth": 10.0, "center": [12.0, 20.0] },
                { "op": "hole",    "diameter": 8.0, "depth": 10.0, "center": [48.0, 20.0] },
                { "op": "fillet",  "edges": "all", "radius": 2.0 }
            ]
        }"#,
    )
}

// ── Parsing and round-trips ───────────────────────────────────────────────────

#[test]
fn bracket_parses_correctly() {
    let prog = bracket();
    assert_eq!(prog.units, Units::Mm);
    assert_eq!(prog.features.len(), 5);

    // Verify sketch op
    match &prog.features[0] {
        Feature::Sketch(op) => {
            assert_eq!(op.plane, SketchPlane::XY);
            match &op.profile {
                Profile::Rect(r) => {
                    assert!((r.w - 60.0).abs() < f64::EPSILON);
                    assert!((r.h - 40.0).abs() < f64::EPSILON);
                }
                _ => panic!("expected Rect profile"),
            }
        }
        _ => panic!("expected Sketch feature"),
    }

    // Verify fillet edges default to "all"
    match &prog.features[4] {
        Feature::Fillet(op) => assert!(op.edges.is_all()),
        _ => panic!("expected Fillet feature"),
    }
}

#[test]
fn bracket_json_round_trips() {
    let original = bracket();
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: CadProgram = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, restored);
}

#[test]
fn edge_selection_variants_parse() {
    let fillet_all: FilletOp =
        serde_json::from_str(r#"{"radius":1.0,"edges":"all"}"#).unwrap();
    assert!(fillet_all.edges.is_all());

    let fillet_idx: FilletOp =
        serde_json::from_str(r#"{"radius":1.0,"edges":[0,3,5]}"#).unwrap();
    assert!(!fillet_idx.edges.is_all());

    let fillet_default: FilletOp =
        serde_json::from_str(r#"{"radius":1.5}"#).unwrap();
    assert!(fillet_default.edges.is_all());
}

#[test]
fn inches_unit_parses() {
    let prog: CadProgram = serde_json::from_str(
        r#"{"units":"in","features":[
               {"op":"sketch","plane":"XY","profile":{"rect":{"w":2.0,"h":1.0}}},
               {"op":"extrude","depth":0.25}
           ]}"#,
    )
    .unwrap();
    assert_eq!(prog.units, Units::Inch);
}

// ── Execution (mock backend) ──────────────────────────────────────────────────

#[test]
fn bracket_executes_and_returns_geometry() {
    let engine = Engine::default(); // always mock
    let result = engine.execute(&bracket());
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());

    let output = result.unwrap();

    // Mesh
    assert!(!output.mesh.positions.is_empty(), "positions must not be empty");
    assert_eq!(
        output.mesh.positions.len(),
        output.mesh.normals.len(),
        "positions and normals must have equal length"
    );

    // Non-indexed mock mesh: length must be divisible by 9 (3 verts × 3 floats)
    assert_eq!(
        output.mesh.positions.len() % 9,
        0,
        "non-indexed position array must be divisible by 9"
    );

    // Metrics
    assert!(output.metrics.volume > 0.0, "volume must be positive");
    assert!(output.metrics.is_solid, "result must be a solid");
    assert!(output.metrics.surface_area > 0.0, "surface area must be positive");

    // Bounding box sanity
    let [xmin, ymin, zmin, xmax, ymax, zmax] = output.metrics.bbox;
    assert!(xmax > xmin);
    assert!(ymax > ymin);
    assert!(zmax > zmin);
}

#[test]
fn volume_matches_expected_for_simple_box() {
    let engine = Engine::default();
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":10.0,"h":5.0}}},
                {"op":"extrude","depth":2.0}
            ]
        }"#,
    );
    let output = engine.execute(&prog).unwrap();
    // Volume of a 10×5×2 box = 100 mm³
    assert!(
        (output.metrics.volume - 100.0).abs() < 1e-6,
        "volume = {}",
        output.metrics.volume
    );
}

// ── Validation rejections ─────────────────────────────────────────────────────

#[test]
fn rejects_negative_extrude_depth() {
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":40.0,"h":20.0}}},
                {"op":"extrude","depth":-5.0}
            ]
        }"#,
    );
    let result = Engine::default().execute(&prog);
    assert!(
        matches!(result, Err(KernelError::Validation(_))),
        "expected Validation error, got {:?}",
        result
    );
}

#[test]
fn rejects_zero_extrude_depth() {
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":40.0,"h":20.0}}},
                {"op":"extrude","depth":0.0}
            ]
        }"#,
    );
    assert!(Engine::default().execute(&prog).is_err());
}

#[test]
fn rejects_zero_diameter_hole() {
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":40.0,"h":20.0}}},
                {"op":"extrude","depth":5.0},
                {"op":"hole","diameter":0.0,"depth":7.0,"center":[10.0,10.0]}
            ]
        }"#,
    );
    assert!(
        Engine::default().execute(&prog).is_err(),
        "zero-diameter hole must be rejected"
    );
}

#[test]
fn rejects_negative_hole_depth() {
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":40.0,"h":20.0}}},
                {"op":"extrude","depth":5.0},
                {"op":"hole","diameter":6.0,"depth":-1.0,"center":[10.0,10.0]}
            ]
        }"#,
    );
    assert!(Engine::default().execute(&prog).is_err());
}

#[test]
fn rejects_negative_fillet_radius() {
    let prog = parse(
        r#"{
            "units": "mm",
            "features": [
                {"op":"sketch","plane":"XY","profile":{"rect":{"w":40.0,"h":20.0}}},
                {"op":"extrude","depth":5.0},
                {"op":"fillet","radius":-0.5}
            ]
        }"#,
    );
    assert!(Engine::default().execute(&prog).is_err());
}

#[test]
fn rejects_empty_features() {
    let prog = CadProgram {
        units: Units::Mm,
        features: vec![],
    };
    assert!(Engine::default().execute(&prog).is_err());
}
