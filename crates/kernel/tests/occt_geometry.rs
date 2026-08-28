//! Geometry tests that need the real OCCT kernel.
//!
//! Run with: `cargo test -p kernel --features occt --test occt_geometry`

use kernel::engine::Engine;
use kernel::ir::CadProgram;

fn venturi_program() -> CadProgram {
    serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            {
              "op": "sketch",
              "plane": "XZ",
              "profile": {
                "polyline": {
                  "closed": true,
                  "points": [
                    [24, 0], [24, 8], [18, 14], [18, 66], [24, 72], [24, 80],
                    [15, 80], [15, 68], [11, 52], [7, 40], [11, 28], [15, 12], [15, 0]
                  ]
                }
              }
            },
            { "op": "revolve", "axis": "Z", "angle": 360, "origin": [0, 0, 0] },
            {
              "op": "cut",
              "plane": "YZ",
              "through": true,
              "depth": 25,
              "at": [0, 0, 0],
              "profile": { "circle": { "at": [0, 40], "d": 3 } }
            }
          ]
        }"#,
    )
    .expect("venturi JSON")
}

#[test]
fn venturi_revolve_is_a_tube_not_a_disk() {
    let out = Engine::new()
        .execute(&venturi_program())
        .expect("venturi should build");

    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();

    assert!(
        dz > 60.0,
        "expected ~80mm height along Z, got dz={dz:.1} bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dx > 30.0 && dy > 30.0,
        "expected ~48mm diameter in XY, got dx={dx:.1} dy={dy:.1}"
    );
    let min = dx.min(dy).min(dz);
    let max = dx.max(dy).max(dz);
    assert!(
        min / max > 0.2,
        "still looks planar ({dx:.1}×{dy:.1}×{dz:.1})"
    );
    assert!(
        out.metrics.volume > 1_000.0,
        "volume too small for a hollow tube: {}",
        out.metrics.volume
    );
}

#[test]
fn revolve_around_plane_normal_is_rejected() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "polyline": { "closed": true,
                "points": [[10,0],[20,0],[20,20],[10,20]] } } },
            { "op": "revolve", "axis": "Z", "angle": 360 }
          ]
        }"#,
    )
    .unwrap();

    let err = Engine::new().execute(&prog).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("disk") || msg.to_lowercase().contains("perpendicular"),
        "unexpected error: {msg}"
    );
}

/// 5 mm fillet on a 6 mm plate with a window used to fillet top+bottom+verticals
/// and explode into tessellation spikes. The kernel must keep the AABB in-family.
#[test]
fn thin_plate_fillet_does_not_spike() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [100, 70, 6], "centered": true },
            { "op": "cut", "through": true, "depth": 10,
              "profile": { "rect": { "w": 50, "h": 30, "centered": true } } },
            { "op": "fillet", "radius": 5, "edges": "all" }
          ]
        }"#,
    )
    .unwrap();

    let out = Engine::new()
        .execute(&prog)
        .expect("6 mm plate with 5 mm fillet should build");

    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!(
        xmin > -55.0 && xmax < 55.0 && ymin > -40.0 && ymax < 40.0,
        "fillet exploded XY bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        zmin > -4.0 && zmax < 12.0,
        "fillet exploded Z bbox={:?}",
        out.metrics.bbox
    );
    for chunk in out.mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        assert!(
            chunk[0] > -60.0
                && chunk[0] < 60.0
                && chunk[1] > -45.0
                && chunk[1] < 45.0
                && chunk[2] > -8.0
                && chunk[2] < 16.0,
            "spike vertex {:?}",
            chunk
        );
    }
    assert!(out.metrics.volume > 1000.0, "volume vanished: {}", out.metrics.volume);
}

#[test]
fn feature_pattern_holes_on_plate() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [80, 40, 8], "centered": true },
            { "op": "hole", "diameter": 6, "depth": 20, "center": [-25, 0] },
            { "op": "pattern", "scope": "feature", "kind": "linear", "count": 2,
              "spacing": 50, "direction": [1, 0, 0] }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("feature pattern holes");
    assert!(out.metrics.is_solid);
    // Two through-holes remove more volume than one.
    assert!(out.metrics.volume < 80.0 * 40.0 * 8.0 - 100.0, "holes not patterned");
}

#[test]
fn pipe_polyline_builds_solid() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "pipe", "diameter": 6, "fuse": false,
              "path": { "polyline": { "points": [[0,0,0],[40,0,0],[40,0,30]] } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("pipe");
    assert!(out.metrics.volume > 10.0, "pipe volume too small: {}", out.metrics.volume);
    let [xmin, _ymin, zmin, xmax, _ymax, zmax] = out.metrics.bbox;
    assert!(xmax - xmin > 30.0 && zmax - zmin > 20.0, "pipe bbox {:?}", out.metrics.bbox);
}

#[test]
fn compound_profile_extrude() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": {
                "compound": {
                  "outer": { "rect": { "w": 40, "h": 40, "centered": true } },
                  "holes": [ { "circle": { "d": 12, "at": [0, 0] } } ]
                }
              }
            },
            { "op": "extrude", "depth": 5 }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("compound extrude");
    let solid_box = 40.0 * 40.0 * 5.0;
    assert!(
        out.metrics.volume < solid_box - 50.0,
        "expected hole in compound profile, volume={}",
        out.metrics.volume
    );
}

#[test]
fn common_boolean_shrinks_volume() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [40, 40, 40], "centered": true },
            { "op": "common", "depth": 40, "at": [0,0,-20],
              "profile": { "circle": { "d": 30, "at": [0, 0] } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("common");
    assert!(out.metrics.volume < 40.0 * 40.0 * 40.0 * 0.7);
}

#[test]
fn topology_lists_faces_and_edges() {
    let prog: CadProgram = serde_json::from_str(
        r#"{ "units":"mm", "features":[ { "op":"box", "size":[20,10,5], "centered": true } ] }"#,
    )
    .unwrap();
    let report = Engine::new().list_topology(&prog).expect("topology");
    assert_eq!(report.summary.face_count, 6);
    assert!(report.summary.edge_count >= 12);
    assert!(report.summary.largest_face.is_some());
    assert!(report.summary.top_face.is_some());
}

#[test]
fn thicken_sketch_makes_solid() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "rect": { "w": 30, "h": 20, "centered": true } } },
            { "op": "thicken", "thickness": 4, "fuse": false }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("thicken");
    assert!(out.metrics.volume > 100.0, "thicken volume {}", out.metrics.volume);
}

#[test]
fn cut_on_largest_face() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [50, 50, 10], "centered": true },
            { "op": "cut", "face": "largest", "through": false, "depth": 3,
              "profile": { "circle": { "d": 10, "at": [0, 0] } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("cut on face");
    assert!(out.metrics.volume < 50.0 * 50.0 * 10.0);
}
