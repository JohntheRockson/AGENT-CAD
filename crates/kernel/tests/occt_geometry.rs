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
