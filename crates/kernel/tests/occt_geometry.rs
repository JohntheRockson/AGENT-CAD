//! Geometry tests that need the real OCCT kernel.
//!
//! Run with: `cargo test -p kernel --features occt --test occt_geometry`

use kernel::engine::Engine;
use kernel::ir::{CadDocument, CadProgram};

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
    let spread = angular_radius_spread_at_z(&out.mesh, (zmin + zmax) * 0.5, 0.2);
    assert!(
        spread > 0.25,
        "groove should sit on one side of a z-slice (helix), not all around (rings); spread={spread}"
    );
    let n_yaws = distinct_groove_yaws(&out.mesh, zmin + 1.2, zmin + 6.5, 12);
    assert!(
        n_yaws >= 5,
        "groove must walk around the shank (helix); distinct yaws over one thread={n_yaws} (rings stay in 1-2 bins)"
    );
    assert_no_vertical_uncut_strip(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
    assert_iso_v_thread_profile(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
    if let Ok(path) = std::env::var("AGENTCAD_DUMP_MESH_SHORT") {
        std::fs::write(&path, kernel::export::to_obj(&out.mesh)).expect("dump mesh");
    }
}

fn groove_yaw_at_z(mesh: &kernel::engine::MeshData, z: f64, band: f64) -> Option<f64> {
    const N: usize = 32;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() > band {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < 2.5 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
    }
    let mut best = None;
    let mut best_r = f64::MAX;
    for i in 0..N {
        if counts[i] >= 2 && mins[i] < best_r {
            best_r = mins[i];
            let yaw = (i as f64 + 0.5) / N as f64 * 2.0 * std::f64::consts::PI
                - std::f64::consts::PI;
            best = Some(yaw);
        }
    }
    best.filter(|_| best_r < 3.85)
}

fn distinct_groove_yaws(mesh: &kernel::engine::MeshData, z0: f64, z1: f64, samples: usize) -> usize {
    let mut bins = std::collections::HashSet::new();
    for i in 0..samples {
        let z = z0 + (z1 - z0) * (i as f64) / (samples as f64);
        if let Some(y) = groove_yaw_at_z(mesh, z, 0.1) {
            let bin = (((y + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * 16.0).floor()
                as i32;
            bins.insert(bin.rem_euclid(16));
        }
    }
    bins.len()
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

/// Fail if a yaw sector stays at the major radius over the whole shank band —
/// a generator-line of uncut cylinder (the M8 sliver). Also flags the +X
/// meridian specifically: that is the hex-vertex seam the cutter used to miss.
fn assert_no_vertical_uncut_strip(
    mesh: &kernel::engine::MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) {
    assert!(
        !mesh.positions.is_empty(),
        "empty mesh — WASM trap / placeholder, not a threaded shank"
    );
    let depth = kernel::thread::external_depth(pitch);
    let cut_r = r_major - 0.28 * depth;
    // 15° bins: a conspicuous leftover strip occupies a whole sector.
    // A single 7.5° polyline-vertex knife-edge is not the screenshot sliver.
    const N: usize = 24;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    let mut plus_x_min = f64::MAX;
    let mut plus_x_n = 0u32;
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let z = chunk[2] as f64;
        if z < z0 || z > z1 {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < r_major * 0.55 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
        if theta.abs() < 0.14 {
            plus_x_min = plus_x_min.min(r);
            plus_x_n += 1;
        }
    }
    let populated = counts.iter().filter(|c| **c >= 4).count();
    assert!(
        populated >= N * 2 / 3,
        "too few yaw bins have vertices ({populated}/{N}) — placeholder or tessellation failed"
    );
    if let Ok(path) = std::env::var("AGENTCAD_DUMP_MESH_SHORT") {
        let _ = std::fs::write(&path, kernel::export::to_obj(mesh));
    }
    let mut uncut: Vec<(usize, f64, u32)> = Vec::new();
    for i in 0..N {
        if counts[i] >= 6 && mins[i] > cut_r {
            uncut.push((i, mins[i], counts[i]));
        }
    }
    assert!(
        uncut.is_empty(),
        "vertical uncut strip after helical thread CUT: yaw bins {:?} stay above r={cut_r:.3} \
         (major={r_major}). Groove must continue all the way around.",
        uncut
            .iter()
            .map(|(i, r, n)| format!("bin{i} r={r:.3} n={n}"))
            .collect::<Vec<_>>()
    );
    assert!(
        plus_x_n >= 4,
        "+X meridian (hex-vertex seam) has no shank vertices — cannot inspect the sliver"
    );
    assert!(
        plus_x_min <= cut_r,
        "+X meridian still at r={plus_x_min:.3} (cut below {cut_r:.3}) — the screenshot sliver"
    );
    let panel_z0 = ((z0 + z1) * 0.5 - 4.0).max(z0);
    let panel_z1 = (panel_z0 + 8.0).min(z1);
    let panel = max_full_height_uncut_yaw_span_deg(mesh, r_major, panel_z0, panel_z1);
    assert!(
        panel < 25.0,
        "leftover uncut cylinder panel spans {panel:.1}° of yaw — cutter did not roll around the shank"
    );
}

/// Largest contiguous yaw span of triangles that sit on the major cylinder
/// and run most of the shank height — the leftover generator strip.
fn max_full_height_uncut_yaw_span_deg(
    mesh: &kernel::engine::MeshData,
    r_major: f64,
    z0: f64,
    z1: f64,
) -> f64 {
    let min_zspan = ((z1 - z0) * 0.55).clamp(5.0, 7.5);
    let r_lo = r_major - 0.08;
    let r_hi = r_major + 0.15;
    let tris: Vec<[usize; 3]> = if mesh.indices.is_empty() {
        (0..mesh.positions.len() / 9)
            .map(|t| {
                let i = t * 3;
                [i, i + 1, i + 2]
            })
            .collect()
    } else {
        mesh.indices
            .chunks(3)
            .filter_map(|c| {
                if c.len() == 3 {
                    Some([c[0] as usize, c[1] as usize, c[2] as usize])
                } else {
                    None
                }
            })
            .collect()
    };
    let mut yaws: Vec<f64> = Vec::new();
    for [a, b, c] in tris {
        let p = |i: usize| {
            [
                mesh.positions[i * 3] as f64,
                mesh.positions[i * 3 + 1] as f64,
                mesh.positions[i * 3 + 2] as f64,
            ]
        };
        let pa = p(a);
        let pb = p(b);
        let pc = p(c);
        let rs = [
            (pa[0] * pa[0] + pa[1] * pa[1]).sqrt(),
            (pb[0] * pb[0] + pb[1] * pb[1]).sqrt(),
            (pc[0] * pc[0] + pc[1] * pc[1]).sqrt(),
        ];
        if rs.iter().copied().fold(f64::INFINITY, f64::min) < r_lo
            || rs.iter().copied().fold(0.0_f64, f64::max) > r_hi
        {
            continue;
        }
        let zs = [pa[2], pb[2], pc[2]];
        let zspan = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - zs.iter().copied().fold(f64::INFINITY, f64::min);
        if zspan < min_zspan {
            continue;
        }
        let yaw = (pa[1] + pb[1] + pc[1]).atan2(pa[0] + pb[0] + pc[0]);
        yaws.push(yaw);
    }
    if yaws.len() < 3 {
        return 0.0;
    }
    yaws.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut best: f64 = 0.0;
    let mut run_start = yaws[0];
    let mut prev = yaws[0];
    for &y in &yaws[1..] {
        if y - prev > 0.22 {
            best = best.max(prev - run_start);
            run_start = y;
        }
        prev = y;
    }
    best = best.max(prev - run_start);
    let wrap = yaws[0] + 2.0 * std::f64::consts::PI - yaws[yaws.len() - 1];
    if wrap <= 0.22 {
        let extra = (yaws[0] - (-std::f64::consts::PI)) + (std::f64::consts::PI - yaws[yaws.len() - 1]);
        best = best.max(extra);
    }
    best * 180.0 / std::f64::consts::PI
}

/// Min radius in each yaw sector. A helix has a groove in some sectors only;
/// stacked rings have the same min radius all the way around.
fn angular_radius_spread_at_z(mesh: &kernel::engine::MeshData, z: f64, band: f64) -> f64 {
    const N: usize = 16;
    let mut mins = [f64::MAX; N];
    let mut counts = [0u32; N];
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        if (chunk[2] as f64 - z).abs() > band {
            continue;
        }
        let x = chunk[0] as f64;
        let y = chunk[1] as f64;
        let r = (x * x + y * y).sqrt();
        if r < 2.0 {
            continue;
        }
        let theta = y.atan2(x);
        let mut bin = (((theta + std::f64::consts::PI) / (2.0 * std::f64::consts::PI)) * N as f64)
            .floor() as isize;
        if bin < 0 {
            bin = 0;
        }
        let bin = (bin as usize).min(N - 1);
        mins[bin] = mins[bin].min(r);
        counts[bin] += 1;
    }
    let vals: Vec<f64> = mins
        .iter()
        .zip(counts.iter())
        .filter(|(_, c)| **c >= 2)
        .map(|(m, _)| *m)
        .collect();
    if vals.len() < 8 {
        return 0.0;
    }
    vals.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - vals.iter().copied().fold(f64::INFINITY, f64::min)
}

/// Sample (x,y,z) on the shank: vertices plus triangle centroids so coarse
/// flank tessellation still shows up in helix-phase bins.
fn shank_samples(mesh: &kernel::engine::MeshData, z0: f64, z1: f64, r_min: f64) -> Vec<[f64; 3]> {
    let mut pts = Vec::new();
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let p = [chunk[0] as f64, chunk[1] as f64, chunk[2] as f64];
        if p[2] >= z0 && p[2] <= z1 && (p[0] * p[0] + p[1] * p[1]).sqrt() >= r_min {
            pts.push(p);
        }
    }
    let tris: Vec<[usize; 3]> = if mesh.indices.is_empty() {
        (0..mesh.positions.len() / 9)
            .map(|t| [t * 3, t * 3 + 1, t * 3 + 2])
            .collect()
    } else {
        mesh.indices
            .chunks(3)
            .filter_map(|c| {
                (c.len() == 3).then_some([c[0] as usize, c[1] as usize, c[2] as usize])
            })
            .collect()
    };
    for [a, b, c] in tris {
        let pa = [
            mesh.positions[a * 3] as f64,
            mesh.positions[a * 3 + 1] as f64,
            mesh.positions[a * 3 + 2] as f64,
        ];
        let pb = [
            mesh.positions[b * 3] as f64,
            mesh.positions[b * 3 + 1] as f64,
            mesh.positions[b * 3 + 2] as f64,
        ];
        let pc = [
            mesh.positions[c * 3] as f64,
            mesh.positions[c * 3 + 1] as f64,
            mesh.positions[c * 3 + 2] as f64,
        ];
        let g = [
            (pa[0] + pb[0] + pc[0]) / 3.0,
            (pa[1] + pb[1] + pc[1]) / 3.0,
            (pa[2] + pb[2] + pc[2]) / 3.0,
        ];
        if g[2] >= z0 && g[2] <= z1 && (g[0] * g[0] + g[1] * g[1]).sqrt() >= r_min {
            pts.push(g);
        }
    }
    pts
}

/// Fail if the thread form is a boxy/wide bead (screenshot) instead of an
/// ISO-width groove: ~P/8 crest, 5H/8-class depth, sloped flanks, yaw walks.
fn assert_iso_v_thread_profile(
    mesh: &kernel::engine::MeshData,
    r_major: f64,
    pitch: f64,
    z0: f64,
    z1: f64,
) {
    let samples = shank_samples(mesh, z0, z1, r_major * 0.55);
    assert!(
        samples.len() >= 80,
        "too few shank samples ({}) — placeholder or tessellation failed",
        samples.len()
    );

    const N: usize = 32;
    let mut min_r = [f64::MAX; N];
    let mut counts = [0u32; N];
    let mut n_flank = 0u32;
    let mut r_floor = f64::MAX;
    let depth = kernel::thread::external_depth(pitch);
    for p in &samples {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        r_floor = r_floor.min(r);
        if r > r_major - 0.85 * depth && r < r_major - 0.15 {
            n_flank += 1;
        }
        let yaw = p[1].atan2(p[0]);
        let phase = (p[2] / pitch - yaw / (2.0 * std::f64::consts::PI)).rem_euclid(1.0);
        let bin = ((phase * N as f64).floor() as usize).min(N - 1);
        min_r[bin] = min_r[bin].min(r);
        counts[bin] += 1;
    }
    let populated: Vec<usize> = (0..N).filter(|&i| counts[i] >= 1).collect();
    assert!(
        populated.len() >= 10,
        "too few thread-phase bins ({}/{})",
        populated.len(),
        N
    );

    // Pure-crest bins never drop below the major — that is the leftover flat.
    let crest_bins = populated
        .iter()
        .filter(|&&i| min_r[i] > r_major - 0.12)
        .count();
    let crest_frac = crest_bins as f64 / populated.len() as f64;
    assert!(
        crest_frac < 0.36,
        "thread crest is boxy/wide: {crest_frac:.2} of pitch stays at the major \
         (ISO 68-1 crest is ~0.125). Old square/round bead left ~0.40+."
    );
    assert!(
        r_floor < r_major - 0.40 * depth,
        "groove too shallow (r={r_floor:.3} vs major {r_major}); depth should approach 5H/8"
    );
    assert!(
        n_flank >= 20,
        "flanks missing: only {n_flank} intermediate-r samples \
         (rectangular bites jump major↔groove)"
    );

    let n_yaws = distinct_groove_yaws(mesh, z0, z1, 12);
    assert!(
        n_yaws >= 5,
        "groove must still walk around the shank; distinct yaws={n_yaws}"
    );
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
    let t0 = std::time::Instant::now();
    let out = Engine::new()
        .execute(&prog)
        .expect("M8×40 hex-head bolt should build");
    let elapsed = t0.elapsed();
    assert!(
        elapsed.as_secs() < 25,
        "40 mm bolt took {elapsed:?} — segmented helix must stay viewport-fast"
    );
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
    let variation = radius_variation_at_z(&out.mesh, zmin + 20.0, 0.25);
    assert!(
        variation > 0.08,
        "shank should be helical at mid-length; variation={variation}"
    );
    let spread = angular_radius_spread_at_z(&out.mesh, zmin + 20.0, 0.25);
    assert!(
        spread > 0.25,
        "40 mm M8 must look helical, not stacked ticks; angular spread={spread}"
    );
    let n_yaws = distinct_groove_yaws(&out.mesh, zmin + 12.0, zmin + 28.0, 16);
    assert!(
        n_yaws >= 5,
        "40 mm groove must walk around the shank; distinct yaws={n_yaws}"
    );
    assert_no_vertical_uncut_strip(&out.mesh, 4.0, 1.25, zmin + 12.0, zmin + 28.0);
    assert_iso_v_thread_profile(&out.mesh, 4.0, 1.25, zmin + 12.0, zmin + 28.0);
    if let Ok(path) = std::env::var("AGENTCAD_DUMP_MESH") {
        std::fs::write(&path, kernel::export::to_obj(&out.mesh)).expect("dump mesh");
    }
}

/// Job 1 regression: viewport M8 must be a 60° V, not the boxy wide-crest bead.
#[test]
fn m8_external_thread_is_iso_v_not_boxy() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "cylinder", "diameter": 8, "height": 8, "axis": "Z" },
            { "op": "thread", "kind": "external", "size": "M8", "length": 8 }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("M8 V-thread should build");
    assert!(
        out.metrics.volume > 50.0,
        "placeholder volume after V-thread: {}",
        out.metrics.volume
    );
    let major_cyl = std::f64::consts::PI * 4.0 * 4.0 * 8.0;
    assert!(
        out.metrics.volume < 0.97 * major_cyl,
        "V-thread should cut the Ø8 cylinder, vol={}",
        out.metrics.volume
    );
    let [_, _, zmin, _, _, zmax] = out.metrics.bbox;
    assert_iso_v_thread_profile(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
    assert_no_vertical_uncut_strip(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
    let _ = zmax;
}

/// Cylinder + helical thread CUT (short enough to boolean for real, not instance).
/// The golden failure was a leftover +X generator-line of uncut cylinder.
#[test]
fn cylinder_plus_helical_thread_cut_has_no_vertical_sliver() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "cylinder", "diameter": 8, "height": 8, "axis": "Z" },
            { "op": "thread", "kind": "external", "size": "M8", "length": 8 }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&prog)
        .expect("cylinder + helical thread CUT must not WASM-trap");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    let dx = (xmax - xmin).abs();
    let dy = (ymax - ymin).abs();
    let dz = (zmax - zmin).abs();
    assert!(
        dx > 6.0 && dy > 6.0 && dz > 6.0,
        "placeholder bbox after thread cut: {:?}",
        out.metrics.bbox
    );
    let major_cyl = std::f64::consts::PI * 4.0 * 4.0 * 8.0;
    assert!(
        out.metrics.volume > 50.0 && out.metrics.volume < 0.97 * major_cyl,
        "placeholder volume after thread cut: {} (smooth cyl={})",
        out.metrics.volume,
        major_cyl
    );
    let mid = (zmin + zmax) * 0.5;
    let variation = radius_variation_at_z(&out.mesh, mid, 0.2);
    assert!(
        variation > 0.08,
        "thread should be helical; variation={variation} — stacked rings are axisymmetric"
    );
    let n_yaws = distinct_groove_yaws(&out.mesh, zmin + 1.2, zmin + 6.5, 12);
    assert!(
        n_yaws >= 5,
        "groove must walk around the shank; distinct yaws={n_yaws} (rings stay in 1-2 bins)"
    );
    assert_no_vertical_uncut_strip(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
    assert_iso_v_thread_profile(&out.mesh, 4.0, 1.25, zmin + 1.2, zmin + 6.5);
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
        dz > 5.0,
        "expected a visible hex head (and shank if fused), bbox={:?}",
        out.metrics.bbox
    );
    assert!(!out.mesh.positions.is_empty());
}

/// The live golden M8×40 IR (13 mm hex, 34.7 mm thread). Long enough that the
/// kernel instances short rods on an uncut host — that path must not leave a
/// generator-line of the original cylinder.
#[test]
fn m8_bolt_40mm_document_has_no_vertical_sliver() {
    let doc: CadDocument = serde_json::from_str(
        r#"{
          "documentId": "m8_bolt_40mm",
          "units": "mm",
          "parameters": {
            "bolt_length": 40,
            "head_height": 5.3,
            "head_width": 13
          },
          "bodies": [
            {
              "bodyId": "body_m8_bolt",
              "name": "M8 Bolt",
              "visible": true,
              "suppressed": false,
              "transform": { "position": [0, 0, 0], "rotation": [0, 0, 0] },
              "features": [
                { "id": "sketch", "op": "sketch", "origin": [0, 0], "plane": "XY",
                  "profile": { "hex": { "across_flats": 13, "at": [0, 0] } } },
                { "depth": 5.3, "id": "body", "op": "extrude", "symmetric": false },
                { "at": [0, 0, 4.3], "axis": "Z", "diameter": 8, "height": 35.7, "op": "cylinder" },
                { "at": [0, 0, 5.3], "axis": "Z", "center": [0, 0], "hand": "right",
                  "kind": "external", "length": 34.7, "op": "thread", "plane": "XY",
                  "size": "M8", "through": false }
              ],
              "references": []
            }
          ]
        }"#,
    )
    .unwrap();
    let t0 = std::time::Instant::now();
    let out = Engine::new()
        .execute_document(&doc)
        .expect("M8×40 document should build");
    assert!(
        t0.elapsed().as_secs() < 25,
        "40 mm bolt took {:?} — segmented helix must stay viewport-fast",
        t0.elapsed()
    );
    let mesh = &out.bodies[0].mesh;
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!(
        (xmax - xmin).abs() > 12.0 && (ymax - ymin).abs() > 12.0,
        "expected ~13 mm hex head, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        (zmax - zmin).abs() > 38.0 && (zmax - zmin).abs() < 48.0,
        "expected ~40 mm overall, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        out.metrics.volume > 400.0,
        "placeholder volume: {}",
        out.metrics.volume
    );
    let n_yaws = distinct_groove_yaws(mesh, zmin + 12.0, zmin + 28.0, 16);
    assert!(
        n_yaws >= 5,
        "groove must walk around the shank; distinct yaws={n_yaws}"
    );
    if let Ok(path) = std::env::var("AGENTCAD_DUMP_MESH") {
        std::fs::write(&path, kernel::export::to_obj(mesh)).expect("dump mesh");
    }
    assert_no_vertical_uncut_strip(mesh, 4.0, 1.25, zmin + 8.0, zmin + 36.0);
    assert_iso_v_thread_profile(mesh, 4.0, 1.25, zmin + 12.0, zmin + 28.0);
}
