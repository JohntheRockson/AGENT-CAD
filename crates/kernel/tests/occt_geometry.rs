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
    assert!(
        out.metrics.volume > 1000.0,
        "volume vanished: {}",
        out.metrics.volume
    );
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
    assert!(
        out.metrics.volume < 80.0 * 40.0 * 8.0 - 100.0,
        "holes not patterned"
    );
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
    assert!(
        out.metrics.volume > 10.0,
        "pipe volume too small: {}",
        out.metrics.volume
    );
    let [xmin, _ymin, zmin, xmax, _ymax, zmax] = out.metrics.bbox;
    assert!(
        xmax - xmin > 30.0 && zmax - zmin > 20.0,
        "pipe bbox {:?}",
        out.metrics.bbox
    );
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
    assert!(
        out.metrics.volume > 100.0,
        "thicken volume {}",
        out.metrics.volume
    );
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
#[test]
fn later_cylinder_joins_existing_box() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [40, 40, 8], "centered": true },
            { "op": "cylinder", "diameter": 20, "height": 24, "at": [0, 0, 8], "axis": "Z" }
          ]
        }"#,
    )
    .unwrap();

    let out = Engine::new()
        .execute(&prog)
        .expect("box + cylinder should join");

    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    assert!(
        dx > 35.0 && dy > 35.0,
        "box XY vanished — cylinder replaced the body? bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dz > 28.0,
        "expected ~32mm stacked height, got dz={dz:.1} bbox={:?}",
        out.metrics.bbox
    );
    let box_vol = 40.0 * 40.0 * 8.0;
    assert!(
        out.metrics.volume > box_vol + 500.0,
        "volume {} looks like box-only or cylinder-only",
        out.metrics.volume
    );
}

#[test]
fn fuse_as_first_feature_creates_a_solid() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "fuse", "depth": 10, "profile": { "rect": { "w": 40, "h": 20, "centered": true } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("fuse-first should build a solid");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!((xmax - xmin).abs() > 30.0, "bbox={:?}", out.metrics.bbox);
    assert!((ymax - ymin).abs() > 15.0, "bbox={:?}", out.metrics.bbox);
    assert!((zmax - zmin).abs() > 8.0, "bbox={:?}", out.metrics.bbox);
}

#[test]
fn cylinder_on_y_axis_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "cylinder", "diameter": 12, "height": 40, "axis": "Y", "at": [0, -20, 0] }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("Y-axis cylinder should build");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    assert!(
        dy > 30.0,
        "expected length along Y, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dx > 8.0 && dz > 8.0,
        "expected ~12mm diameter in XZ, bbox={:?}",
        out.metrics.bbox
    );
}

#[test]
fn body_rotate_y_on_box_builds() {
    use kernel::ir::CadDocument;
    let doc: CadDocument = serde_json::from_str(
        r#"{
          "documentId": "rot",
          "units": "mm",
          "bodies": [
            {
              "bodyId": "body_arm",
              "transform": { "position": [0, 0, 0], "rotation": [0, 90, 0] },
              "features": [{ "op": "box", "size": [40, 10, 6], "centered": true }]
            }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute_document(&doc)
        .expect("body rotate Y should build");
    assert_eq!(out.bodies.len(), 1);
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dz = (zmax - zmin).abs();
    // 40×10×6 box rotated 90° about Y → length along Z, thickness along X
    assert!(
        dz > 30.0,
        "expected ~40mm along Z after Y rot, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dx > 4.0 && dx < 15.0,
        "expected ~6mm along X after Y rot, bbox={:?}",
        out.metrics.bbox
    );
    let _ = (ymin, ymax);
}

#[test]
fn body_rotate_y_on_disconnected_solids_builds() {
    use kernel::ir::CadDocument;
    let doc: CadDocument = serde_json::from_str(
        r#"{
          "documentId": "compound",
          "units": "mm",
          "bodies": [
            {
              "bodyId": "body_pair",
              "transform": { "position": [0, 0, 0], "rotation": [0, 45, 0] },
              "features": [
                { "op": "box", "size": [12, 8, 6], "centered": true },
                { "op": "cylinder", "diameter": 8, "height": 20, "at": [40, 0, 0], "axis": "Y" }
              ]
            }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute_document(&doc)
        .expect("Y rotation of a multi-solid body should build");
    assert!(!out.bodies[0].mesh.positions.is_empty());
    assert!(out.metrics.volume > 100.0);
}

#[test]
fn cut_without_depth_field_still_executes() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [40, 40, 10], "centered": true },
            { "op": "cut", "through": true, "profile": { "circle": { "d": 8 } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("through-cut with omitted depth should build");
    assert!(out.metrics.volume < 40.0 * 40.0 * 10.0 - 10.0);
}

#[test]
fn m8_external_thread_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "thread", "kind": "external", "size": "M8", "length": 8 }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("M8 external thread should build");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    assert!(
        dx > 6.0 && dy > 6.0,
        "expected ~8mm diameter, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dz > 6.0,
        "expected ~8mm length, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        out.metrics.volume > 50.0,
        "volume vanished: {}",
        out.metrics.volume
    );
    let major_cyl = std::f64::consts::PI * 4.0 * 4.0 * 8.0;
    assert!(
        out.metrics.volume < 0.97 * major_cyl,
        "thread should cut below a smooth Ø8 cylinder, vol={} cyl={}",
        out.metrics.volume,
        major_cyl
    );
    assert!(out.metrics.is_solid);
    let variation = radius_variation_at_z(&out.mesh, (zmin + zmax) * 0.5, 0.2);
    assert!(
        variation > 0.08,
        "thread should be helical (radius varies around a slice); variation={variation} — stacked rings are axisymmetric"
    );
}

fn radius_variation_at_z(mesh: &kernel::engine::MeshData, z: f64, band: f64) -> f64 {
    let mut rs = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() <= band {
            let x = chunk[0] as f64;
            let y = chunk[1] as f64;
            rs.push((x * x + y * y).sqrt());
        }
    }
    if rs.len() < 8 {
        return 0.0;
    }
    let mean = rs.iter().sum::<f64>() / rs.len() as f64;
    (rs.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rs.len() as f64).sqrt()
}

#[test]
fn m8_tap_in_plate_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [30, 30, 10], "centered": true },
            { "op": "thread", "kind": "tap", "size": "M8", "center": [0, 0], "through": true }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("M8 tap in a plate should build");
    let plate = 30.0 * 30.0 * 10.0;
    assert!(
        out.metrics.volume < plate - 50.0,
        "tap should remove material, volume={}",
        out.metrics.volume
    );
}

#[test]
fn ellipsoid_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [{ "op": "ellipsoid", "radii": [10, 6, 4] }]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("ellipsoid should build");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!((xmax - xmin).abs() > 16.0, "bbox={:?}", out.metrics.bbox);
    assert!((ymax - ymin).abs() > 8.0, "bbox={:?}", out.metrics.bbox);
    assert!((zmax - zmin).abs() > 5.0, "bbox={:?}", out.metrics.bbox);
    let _ = (xmin, ymin, zmin);
}

#[test]
fn helix_spring_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [{
            "op": "helix", "pitch": 6, "height": 18, "radius": 8, "section_diameter": 2
          }]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("helix spring should build");
    let [_xmin, _ymin, zmin, _xmax, _ymax, zmax] = out.metrics.bbox;
    let dz = (zmax - zmin).abs();
    assert!(
        dz > 10.0,
        "expected coil height, bbox={:?}",
        out.metrics.bbox
    );
    assert!(out.metrics.volume > 10.0);
}

#[test]
fn offset_grows_a_box() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [20, 20, 20], "centered": true },
            { "op": "offset", "distance": 2 }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new().execute(&prog).expect("offset should build");
    assert!(
        out.metrics.volume > 20.0 * 20.0 * 20.0 + 100.0,
        "offset should grow volume, got {}",
        out.metrics.volume
    );
}

/// Canonical hex-head bolt: hex extrude → overlapping shank → thread cut.
/// This is the recipe that must work for "M8 × 40 mm, 10 mm hex head".
#[test]
fn m8_hex_head_bolt_40mm_builds() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 10 } } },
            { "op": "extrude", "depth": 5.5 },
            { "op": "cylinder", "diameter": 8, "height": 35.5, "at": [0, 0, 4.5] },
            { "op": "thread", "kind": "external", "size": "M8", "length": 34.5, "at": [0, 0, 5.5] }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("M8×40 hex-head bolt should build");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    assert!(
        dx > 9.0 && dy > 9.0,
        "expected ~10 mm hex head, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        dz > 38.0 && dz < 48.0,
        "expected ~40 mm overall length, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        out.metrics.volume > 200.0,
        "volume vanished: {}",
        out.metrics.volume
    );
    let variation = radius_variation_at_z(&out.mesh, zmin + 20.0, 0.35);
    assert!(
        variation > 0.08,
        "shank should be helical at mid-length; variation={variation}"
    );
}

/// Agent often threads first then fuses a hex head — that boolean used to crash.
/// Compound fallback must still produce a visible head + shank.
#[test]
fn hex_head_fused_onto_short_thread_does_not_crash() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "thread", "kind": "external", "size": "M8", "length": 8 },
            { "op": "fuse", "depth": 5.5, "at": [0, 0, 8],
              "profile": { "hex": { "across_flats": 10 } } }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("fusing a hex onto a thread must not crash");
    let [_xmin, _ymin, zmin, _xmax, _ymax, zmax] = out.metrics.bbox;
    let dz = (zmax - zmin).abs();
    assert!(
        dz > 10.0,
        "expected thread + head height, bbox={:?}",
        out.metrics.bbox
    );
    assert!(!out.mesh.positions.is_empty());
}
