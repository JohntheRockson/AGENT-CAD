//! Geometry tests that need the real OCCT kernel.
//!
//! Run with: `cargo test -p kernel --features occt --test occt_geometry`

use kernel::engine::{Engine, ExportFormat, MeshData, MeshProvenance};
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
    // Band must be thinner than the ISO P/8 crest (~0.16 mm). A 0.2 mm
    // slab smears a real M8 crest into every yaw bin and looks like rings.
    let spread = angular_radius_spread_at_z(&out.mesh, (zmin + zmax) * 0.5, 0.08);
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

/// Groove helix phase at `z`: `z/P − yaw/2π` (turns, wrapped to `[0, 1)`).
/// A continuous helix holds this nearly constant; an instance-window jump
/// steps it.
fn helix_phase_at_z(mesh: &kernel::engine::MeshData, z: f64, pitch: f64, band: f64) -> Option<f64> {
    let yaw = groove_yaw_at_z(mesh, z, band)?;
    Some((z / pitch.max(1e-9) - yaw / (2.0 * std::f64::consts::PI)).rem_euclid(1.0))
}

/// Fail if instanced slabs meet with a visible helix step (Ian's mid-shank
/// horizontal jumps). Does not loosen the mid-shank helix / ISO checks.
fn assert_helix_continuous_across_instance_windows(
    mesh: &kernel::engine::MeshData,
    pitch: f64,
    z0: f64,
    z1: f64,
) {
    let step = (pitch * 0.35).clamp(0.30, 0.50);
    let mut zs = Vec::new();
    let mut z = z0;
    while z <= z1 + 1e-9 {
        zs.push(z);
        z += step;
    }
    let mut phases: Vec<(f64, f64)> = Vec::new();
    for &zi in &zs {
        if let Some(p) = helix_phase_at_z(mesh, zi, pitch, 0.12) {
            phases.push((zi, p));
        }
    }
    assert!(
        phases.len() >= 8,
        "too few helix-phase samples ({}) between {z0:.1} and {z1:.1} — \
         cannot inspect instance seams",
        phases.len()
    );
    // Unwrap, then 3-sample median so a single tip-cap misfire is not a seam.
    let mut unwrapped = vec![phases[0].1];
    for i in 1..phases.len() {
        let mut p = phases[i].1;
        let prev = unwrapped[i - 1];
        while p - prev > 0.5 {
            p -= 1.0;
        }
        while p - prev < -0.5 {
            p += 1.0;
        }
        unwrapped.push(p);
    }
    let median3 = |i: usize| -> f64 {
        let a = unwrapped[i.saturating_sub(1)];
        let b = unwrapped[i];
        let c = unwrapped[(i + 1).min(unwrapped.len() - 1)];
        let mut v = [a, b, c];
        v.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
        v[1]
    };
    let mut worst = 0.0_f64;
    let mut worst_at = phases[0].0;
    for i in 1..phases.len() {
        let d = (median3(i) - median3(i - 1)).abs();
        if d > worst {
            worst = d;
            worst_at = 0.5 * (phases[i - 1].0 + phases[i].0);
        }
    }
    assert!(
        worst < 0.10,
        "helix phase jumps {worst:.3} turn near z={worst_at:.2} \
         (instance window seam / yaw discontinuity)"
    );
}

/// Fail if the first turn is leftover uncut cylinder / a triangular pipe-entry
/// notch (dead-height → thread start).
fn assert_clean_thread_entry(
    mesh: &kernel::engine::MeshData,
    r_major: f64,
    pitch: f64,
    thread_z0: f64,
) {
    let z_probe = thread_z0 + pitch * 0.55;
    let variation = radius_variation_at_z(mesh, z_probe, 0.16);
    assert!(
        variation > 0.08,
        "dead→thread start is uncut or notched (variation={variation:.4} at z={z_probe:.2})"
    );
    let yaw = groove_yaw_at_z(mesh, z_probe, 0.14);
    assert!(
        yaw.is_some(),
        "no groove just after thread start (z={z_probe:.2}) — leftover triangular notch"
    );
    // Mid-shank helix phase must already hold this close to the first turn.
    let z_mid = thread_z0 + pitch * 10.0;
    if let (Some(y0), Some(ym)) = (yaw, groove_yaw_at_z(mesh, z_mid, 0.14)) {
        let expected = ym - 2.0 * std::f64::consts::PI * (z_mid - z_probe) / pitch.max(1e-9);
        let mut d = y0 - expected;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        assert!(
            d.abs() < 0.45,
            "thread-start groove yaw is off the helix by {d:.3} rad \
             (entry notch or wrong first-rod phase)"
        );
    }
    assert_no_vertical_uncut_strip(
        mesh,
        r_major,
        pitch,
        thread_z0 + pitch * 0.35,
        thread_z0 + pitch * 1.8,
    );
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
        crest_frac < 0.22,
        "thread crest is boxy/wide: {crest_frac:.2} of pitch stays at the major \
         (ISO 68-1 crest is ~0.125). Old mid-triangle bead left ~0.40+."
    );
    assert!(
        r_floor < r_major - 0.40 * depth,
        "groove too shallow (r={r_floor:.3} vs major {r_major}); depth should approach 5H/8"
    );
    let crest_r = populated.iter().map(|&i| min_r[i]).fold(0.0_f64, |a, r| a.max(r));
    assert!(
        crest_r > r_major - 0.20,
        "crests were cut away (max leftover r={crest_r:.3}); ISO leaves ~P/8 at the major"
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
    let mid = 0.5 * (z0 + z1);
    let spread = angular_radius_spread_at_z(mesh, mid, (pitch * 0.06).clamp(0.06, 0.10));
    assert!(
        spread > 0.25,
        "ISO crest must still be helical at a thin z-slice; spread={spread}"
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

/// Locked ISO M8×40 caliper golden (AF 13, Ø8, P 1.25, L 40, head ~5.3).
/// Shared with `tests/reports/m8_x40.json` — do not drift to AF 10.
fn iso_m8_x40_golden_document() -> CadDocument {
    const RAW: &str = include_str!("../../../tests/reports/m8_x40.json");
    CadDocument::from_json_value(serde_json::from_str(RAW).expect("golden JSON"))
        .expect("golden CadDocument")
}

/// Canonical hex-head bolt: hex extrude → overlapping shank → thread cut.
/// ISO-ish golden: AF 13 (M8 wrench, not M6's 10), head ~5.3, Ø8 × 1.25.
/// `bolt_length` 40 is still tip-to-top in this IR (under-head ISO 4017 follow-up).
/// Shared with Inspector `tests/reports/m8_x40.json`.
#[test]
fn m8_hex_head_bolt_40mm_builds() {
    let doc = iso_m8_x40_golden_document();
    assert!(
        (doc.parameters.get("head_width").copied().unwrap_or(0.0) - 13.0).abs() < 1e-9,
        "ISO golden must stay AF 13, not AF 10"
    );
    let t0 = std::time::Instant::now();
    let out = Engine::new()
        .execute_document(&doc)
        .expect("M8×40 hex-head bolt should build")
        .into_model_output()
        .expect("golden document mesh");
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
        dx > 12.0 && dy > 12.0,
        "expected ~13 mm hex head (AF 13, not M6's 10), bbox={:?}",
        out.metrics.bbox
    );
    assert_eq!(
        out.metrics.mesh_provenance,
        MeshProvenance::InstancedThread,
        "long 34.7 mm thread must instance; metrics must name the uncut B-Rep"
    );
    assert!(
        out.metrics
            .mesh_provenance
            .honesty_note()
            .is_some_and(|n| n.contains("uncut")),
        "one-body honesty note must be visible to the caller"
    );
    assert!(
        dz > 38.0 && dz < 48.0,
        "expected ~40 mm overall length, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        out.metrics.volume > 400.0,
        "volume vanished: {}",
        out.metrics.volume
    );
    let variation = radius_variation_at_z(&out.mesh, zmin + 20.0, 0.25);
    assert!(
        variation > 0.08,
        "shank should be helical at mid-length; variation={variation}"
    );
    let spread = angular_radius_spread_at_z(&out.mesh, zmin + 20.0, 0.08);
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
    // Ian retest after #19: instance-window seams + dead→thread entry notch.
    assert_helix_continuous_across_instance_windows(&out.mesh, 1.25, zmin + 8.0, zmin + 36.0);
    assert_clean_thread_entry(&out.mesh, 4.0, 1.25, zmin + 5.3);
    if let Ok(path) = std::env::var("AGENTCAD_DUMP_MESH") {
        std::fs::write(&path, kernel::export::to_obj(&out.mesh)).expect("dump mesh");
    }
}

/// Locked ISO-ish golden: AF 13, head 5.3, Ø8 × 1.25. Overall Z is still
/// tip-to-top ~40 mm (`bolt_length` under-head ISO 4017 is a follow-up).
fn golden_m8_x40_program() -> CadProgram {
    serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 13 } } },
            { "op": "extrude", "depth": 5.3 },
            { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
            { "op": "thread", "kind": "external", "size": "M8", "length": 34.7, "at": [0, 0, 5.3] }
          ]
        }"#,
    )
    .unwrap()
}

fn hex_shank_program() -> CadProgram {
    serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 13 } } },
            { "op": "extrude", "depth": 5.3 },
            { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] }
          ]
        }"#,
    )
    .unwrap()
}

fn assert_step_is_solid_in_mesh_bbox_family(step: &[u8], mesh_bbox: [f64; 6], label: &str) {
    assert!(
        step.len() > 512,
        "{label}: STEP must be non-empty, got {} bytes",
        step.len()
    );
    let text = std::str::from_utf8(step).unwrap_or("");
    assert!(
        text.contains("ISO-10303-21") || text.contains("STEP"),
        "{label}: missing ISO-10303/STEP header"
    );
    assert!(
        text.contains("MANIFOLD_SOLID_BREP")
            || text.contains("FACETED_BREP")
            || text.contains("CLOSED_SHELL")
            || text.contains("BREP_WITH_VOIDS"),
        "{label}: STEP does not parse as a solid (no MANIFOLD_SOLID_BREP/CLOSED_SHELL)"
    );
    let bb = kernel::export::cartesian_bbox_from_step(step)
        .unwrap_or_else(|| panic!("{label}: no CARTESIAN_POINT bbox in STEP"));
    assert!(
        bbox_same_family(mesh_bbox, bb),
        "{label}: STEP bbox {bb:?} not in the same family as mesh {mesh_bbox:?}"
    );
}

fn bbox_same_family(a: [f64; 6], b: [f64; 6]) -> bool {
    for i in 0..3 {
        let ea = (a[i + 3] - a[i]).abs();
        let eb = (b[i + 3] - b[i]).abs();
        let tol = 2.0_f64.max(0.15 * ea.max(eb).max(1.0));
        if (ea - eb).abs() > tol {
            return false;
        }
        let ca = 0.5 * (a[i] + a[i + 3]);
        let cb = 0.5 * (b[i] + b[i + 3]);
        if (ca - cb).abs() > 2.0 {
            return false;
        }
    }
    true
}

/// Golden M8×40 STEP is a faceted solid of the **viewport** instanced mesh
/// (threaded-looking), not OCCT `export_step`. Must not WASM-trap.
#[test]
fn golden_m8_x40_step_export_is_nonempty_solid() {
    let prog = golden_m8_x40_program();
    let engine = Engine::new();
    let mesh_out = engine
        .execute(&prog)
        .expect("golden M8 execute (needed for bbox family)");
    let mesh_bbox = kernel::engine::bbox_from_positions(&mesh_out.mesh.positions);
    let t0 = std::time::Instant::now();
    let step = engine
        .export(&prog, &ExportFormat::Step)
        .expect("golden M8×40 STEP must not WASM-trap");
    let elapsed = t0.elapsed();
    assert!(
        elapsed.as_secs() < 40,
        "40 mm bolt STEP took {elapsed:?} — must stay seconds, not a minute"
    );
    assert_step_is_solid_in_mesh_bbox_family(&step, mesh_bbox, "golden M8×40");
    assert_eq!(
        mesh_out.metrics.mesh_provenance,
        MeshProvenance::InstancedThread
    );
}

/// Inspector PR #13: hex-only and hex+shank probes crashed the same way as
/// golden M8. STEP must succeed for those prefixes too.
#[test]
fn hex_only_and_hex_shank_step_export_no_wasm_crash() {
    let engine = Engine::new();
    let golden = golden_m8_x40_program();

    let hex_only = CadProgram {
        units: golden.units.clone(),
        features: golden.features[..2].to_vec(),
    };
    let hex_mesh = engine.execute(&hex_only).expect("hex-only execute");
    let hex_step = engine
        .export(&hex_only, &ExportFormat::Step)
        .expect("hex-only STEP must not WASM-trap");
    assert_step_is_solid_in_mesh_bbox_family(
        &hex_step,
        kernel::engine::bbox_from_positions(&hex_mesh.mesh.positions),
        "hex-only",
    );

    let hex_shank = hex_shank_program();
    let shank_mesh = engine.execute(&hex_shank).expect("hex+shank execute");
    let shank_step = engine
        .export(&hex_shank, &ExportFormat::Step)
        .expect("hex+shank STEP must not WASM-trap");
    assert_step_is_solid_in_mesh_bbox_family(
        &shank_step,
        kernel::engine::bbox_from_positions(&shank_mesh.mesh.positions),
        "hex+shank",
    );
}

/// Empty tessellation must fail the export. A bbox-sized box would pass
/// `bbox_same_family` and is a placeholder solid — not allowed.
#[test]
fn empty_tessellation_step_export_fails_not_bbox_box() {
    let empty = MeshData {
        positions: vec![],
        normals: vec![],
        indices: vec![],
    };
    let err = kernel::export::step_export_bytes(&empty)
        .expect_err("empty tessellation must not write STEP");
    assert!(
        err.contains("empty") || err.contains("no solid"),
        "unexpected: {err}"
    );
    let text = kernel::export::to_step(&empty);
    assert!(text.is_empty());
    assert!(!text.contains("MANIFOLD_SOLID_BREP"));
    assert!(kernel::export::cartesian_bbox_from_step(text.as_bytes()).is_none());
}

/// Under-head fillet keeps the Ø8 circular junction (not `filter_to_line_edges`)
/// and uses bolt Z (not argmin3 X/Y). Failure is Err, not eprintln-only Ok.
#[test]
fn m8_underhead_fillet_keeps_circular_junction() {
    let unfilleted = Engine::new()
        .execute(&hex_shank_program())
        .expect("hex+shank");
    let filleted: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "sketch", "plane": "XY",
              "profile": { "hex": { "across_flats": 13 } } },
            { "op": "extrude", "depth": 5.3 },
            { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
            { "op": "fillet", "radius": 0.4, "edges": "all" }
          ]
        }"#,
    )
    .unwrap();
    let out = Engine::new()
        .execute(&filleted)
        .expect("under-head fillet must apply, not silent Ok");
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!(
        (xmax - xmin).abs() > 12.0 && (ymax - ymin).abs() > 12.0,
        "expected AF 13 hex after fillet, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        (zmax - zmin).abs() > 38.0 && (zmax - zmin).abs() < 48.0,
        "fillet must not explode Z, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        out.metrics.volume > 200.0,
        "volume vanished: {}",
        out.metrics.volume
    );
    // Concave under-head junction: blend *adds* a torus-like wedge (~0.8 mm³
    // at r=0.4 on Ø8). A dropped circle / silent no-op would stay within noise.
    let dv = out.metrics.volume - unfilleted.metrics.volume;
    assert!(
        dv > 0.3,
        "fillet did not add under-head blend (circle dropped?): ΔV={dv} ({} vs {})",
        out.metrics.volume,
        unfilleted.metrics.volume
    );
}

/// Impossible fillet must Err — no eprintln-only success.
#[test]
fn fillet_failure_is_err_not_silent_ok() {
    let prog: CadProgram = serde_json::from_str(
        r#"{
          "units": "mm",
          "features": [
            { "op": "box", "size": [4, 4, 4], "centered": true },
            { "op": "fillet", "radius": 50, "edges": "all" }
          ]
        }"#,
    )
    .unwrap();
    let err = Engine::new()
        .execute(&prog)
        .expect_err("oversized fillet must not silently succeed");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("fillet") && (msg.contains("silent") || msg.contains("could not")),
        "unexpected: {err}"
    );
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
            { "op": "fuse", "depth": 5.3, "at": [0, 0, 8],
              "profile": { "hex": { "across_flats": 13 } } }
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
/// generator-line of the original cylinder. Same fixture as Inspector
/// `tests/reports/m8_x40.json`.
#[test]
fn m8_bolt_40mm_document_has_no_vertical_sliver() {
    let doc = iso_m8_x40_golden_document();
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
    assert_eq!(
        out.metrics.mesh_provenance,
        MeshProvenance::InstancedThread,
        "document golden must name the instanced-vs-uncut split"
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
    assert_helix_continuous_across_instance_windows(mesh, 1.25, zmin + 8.0, zmin + 36.0);
    assert_clean_thread_entry(mesh, 4.0, 1.25, zmin + 5.3);
}

/// Head height and dead height must stay put when only `bolt_length` changes.
/// A uniform Z scale (or bolt_length driving hex extrude depth) is the bug.
#[test]
fn m8_bolt_length_keeps_head_and_dead_height() {
    fn bolt(length: f64) -> CadDocument {
        CadDocument::from_json_value(serde_json::json!({
            "documentId": "m8_len",
            "units": "mm",
            "parameters": {
                "bolt_length": length,
                "head_height": 5.3,
                "dead_height": 2.0,
                "head_width": 13.0
            },
            "bodies": [{
                "bodyId": "body_m8_bolt",
                "name": "M8 Bolt",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": "head_width" } } },
                    { "op": "extrude", "depth": "head_height" },
                    { "op": "cylinder", "diameter": 8, "axis": "Z",
                      "height": "bolt_length - head_height + 1",
                      "at": [0, 0, "head_height - 1"] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": "bolt_length - head_height - dead_height",
                      "at": [0, 0, "head_height + dead_height"] }
                ]
            }]
        }))
        .expect("bolt document")
    }

    let short = Engine::new()
        .execute_document(&bolt(40.0))
        .expect("40 mm bolt");
    let long = Engine::new()
        .execute_document(&bolt(64.0))
        .expect("64 mm bolt");

    let head_40 = hex_head_z_extent(&short.bodies[0].mesh, 4.0);
    let head_64 = hex_head_z_extent(&long.bodies[0].mesh, 4.0);
    assert!(
        head_40.2 > 0.5 && head_64.2 > 0.5,
        "missing hex-head vertices: 40={head_40:?} 64={head_64:?}"
    );
    assert!(
        (head_40.2 - head_64.2).abs() < 0.35,
        "head Z extent changed with length: 40mm={:.3} 64mm={:.3}",
        head_40.2,
        head_64.2
    );
    assert!(
        (head_40.2 - 5.3).abs() < 0.8,
        "head Z should stay ~head_height 5.3, got {:.3}",
        head_40.2
    );

    let z40 = (short.metrics.bbox[5] - short.metrics.bbox[2]).abs();
    let z64 = (long.metrics.bbox[5] - long.metrics.bbox[2]).abs();
    assert!(
        (z64 - z40 - 24.0).abs() < 3.0,
        "shank/thread Z must grow with length: dz40={z40:.2} dz64={z64:.2}"
    );
    assert!(
        z64 > z40 + 16.0,
        "overall length did not grow: {z40:.2} → {z64:.2}"
    );
}

/// Golden recipe from the leftover-tessellate-crash job: hex → overlapping
/// cylinder → long thread CUT. Must instance short rods and never mesh the
/// >8-turn uncut host.
fn golden_hex_cylinder_thread_cut() -> CadDocument {
    CadDocument::from_json_value(serde_json::json!({
        "documentId": "m8_bolt",
        "units": "mm",
        "parameters": { "head_width": 13.0, "head_height": 5.3, "bolt_length": 24.0 },
        "bodies": [{
            "bodyId": "body_m8_bolt",
            "features": [
                { "op": "sketch", "plane": "XY",
                  "profile": { "hex": { "across_flats": "head_width" } } },
                { "op": "extrude", "depth": "head_height" },
                { "op": "cylinder", "diameter": 8, "height": "bolt_length", "at": [0, 0, 3] },
                { "op": "thread", "kind": "external", "size": "M8", "length": 20, "at": [0, 0, 5.3] }
            ]
        }]
    }))
    .expect("golden bolt document")
}

fn assert_not_wasm_trap(err: &kernel::engine::KernelError) {
    let msg = err.to_string().to_lowercase();
    assert!(
        !msg.contains("runtime")
            && !msg.contains("wasm trap")
            && !msg.contains("wasm runtime")
            && !msg.contains("out of bounds")
            && !msg.contains("memory fault")
            && !msg.contains("unreachable")
            && !msg.contains("internal cad kernel crash"),
        "instance-path failure trapped WASM: {err}"
    );
}

/// Golden hex → overlapping cylinder → thread CUT never tessellates a
/// >8-turn uncut host (instance short rods instead).
#[test]
fn golden_hex_cylinder_thread_cut_never_tessellates_long_uncut_host() {
    let _ = kernel::engine::take_long_host_tessellate_attempts();
    let t0 = std::time::Instant::now();
    let out = Engine::new()
        .execute_document(&golden_hex_cylinder_thread_cut())
        .expect("golden hex→cylinder→thread CUT should instance, not trap");
    assert!(
        t0.elapsed().as_secs() < 25,
        "golden bolt took {:?} — instance path must stay viewport-fast",
        t0.elapsed()
    );
    assert_eq!(
        kernel::engine::take_long_host_tessellate_attempts(),
        0,
        "tessellated a >8-turn uncut host (old fallthrough)"
    );
    let mesh = &out.bodies[0].mesh;
    assert!(
        !mesh.positions.is_empty(),
        "empty mesh — WASM trap / placeholder"
    );
    let [xmin, ymin, zmin, xmax, ymax, zmax] = out.metrics.bbox;
    assert!(
        (xmax - xmin).abs() > 12.0 && (ymax - ymin).abs() > 12.0,
        "expected ~13 mm hex head, bbox={:?}",
        out.metrics.bbox
    );
    assert!(
        (zmax - zmin).abs() > 20.0,
        "expected shank+head length, bbox={:?}",
        out.metrics.bbox
    );
    let n_yaws = distinct_groove_yaws(mesh, zmin + 8.0, zmin + 18.0, 12);
    assert!(
        n_yaws >= 5,
        "groove must walk around the shank; distinct yaws={n_yaws}"
    );
}

/// Force the short-rod instance path to fail. Must not trap WASM and must
/// not take the old tessellate_solid fallthrough onto the uncut long host.
#[test]
fn long_thread_instance_failure_does_not_fallthrough_to_uncut_host() {
    let _ = kernel::engine::take_long_host_tessellate_attempts();
    let _guard = kernel::engine::with_forced_thread_instance_failure();
    let result = Engine::new().execute_document(&golden_hex_cylinder_thread_cut());
    let attempts = kernel::engine::take_long_host_tessellate_attempts();
    match result {
        Err(e) => {
            assert_not_wasm_trap(&e);
            let msg = e.to_string();
            assert!(
                msg.contains("refusing uncut-host tessellate fallthrough")
                    || msg.contains("instance path"),
                "expected fail-closed instance error, got: {msg}"
            );
        }
        Ok(out) => {
            panic!(
                "instance-path failure must fail closed, not return an uncut-host mesh \
                 (verts={}, vol={}, long-host tessellate attempts={attempts})",
                out.bodies.first().map(|b| b.mesh.positions.len()).unwrap_or(0),
                out.metrics.volume,
            );
        }
    }
    assert_eq!(
        attempts, 0,
        "old fallthrough tessellated the >8-turn uncut host"
    );
}

/// Z span of vertices outside the shank radius (the hex head).
fn hex_head_z_extent(mesh: &kernel::engine::MeshData, shank_r: f64) -> (f64, f64, f64) {
    let cut = shank_r + 0.45;
    let mut z0 = f64::MAX;
    let mut z1 = f64::MIN;
    for chunk in mesh.positions.chunks(3) {
        if chunk.len() < 3 {
            continue;
        }
        let r = (chunk[0] as f64).hypot(chunk[1] as f64);
        if r > cut {
            let z = chunk[2] as f64;
            z0 = z0.min(z);
            z1 = z1.max(z);
        }
    }
    if z0 > z1 {
        (0.0, 0.0, 0.0)
    } else {
        (z0, z1, z1 - z0)
    }
}
