//! Golden M8×40 inspector: execute IR, dump mesh, write STEP/STL, try a hex-head
//! fillet, and emit an honest pass/fail report.
//!
//! Public kernel APIs only. A silent no-op fillet is FAIL. Do not patch the kernel.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use kernel::engine::{Engine, ExportFormat, MeshData, MetricsData};
use kernel::export::{to_obj, to_stl};
use kernel::ir::CadProgram;
use serde_json::json;

const FILLET_RADIUS_MM: f64 = 0.8;
/// Tight bbox match: STL vertices vs viewport mesh AABB (mm).
const STL_BBOX_TOL_MM: f64 = 0.05;
/// Hex-head metric change below this is treated as a silent no-op.
const FILLET_VOLUME_EPS_MM3: f64 = 0.25;
const FILLET_LEN_EPS_MM: f64 = 0.02;
const SHANK_R_MM: f64 = 4.0;

fn main() {
    let code = match run() {
        Ok(all_pass) => {
            if all_pass {
                0
            } else {
                1
            }
        }
        Err(e) => {
            eprintln!("inspector fatal: {e}");
            let _ = write_fatal_report(&e);
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<bool, String> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = crate_dir.join("out");
    fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir out: {e}"))?;

    let mut log: Vec<String> = Vec::new();
    let mut failed_cmds: Vec<String> = Vec::new();

    log.push(format!(
        "cwd={} crate_dir={}",
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "?".into()),
        crate_dir.display()
    ));
    log.push(format!(
        "rustc={}",
        rustc_version().unwrap_or_else(|| "unknown".into())
    ));

    #[cfg(feature = "occt")]
    log.push("built with feature `occt` (kernel/occt)".into());
    #[cfg(not(feature = "occt"))]
    {
        log.push("built WITHOUT feature `occt` — Engine is mock; STEP cannot be a real solid".into());
        failed_cmds.push(
            "cargo run --manifest-path tests/reports/Cargo.toml --features occt  (this binary has no `occt` feature)".into(),
        );
    }

    let ir_path = crate_dir.join("m8_x40.json");
    let ir_text = fs::read_to_string(&ir_path).map_err(|e| format!("read m8_x40.json: {e}"))?;
    let ir_value: serde_json::Value =
        serde_json::from_str(&ir_text).map_err(|e| format!("parse m8_x40.json: {e}"))?;
    let program: CadProgram =
        serde_json::from_value(ir_value.clone()).map_err(|e| format!("CadProgram: {e}"))?;

    let engine = Engine::new();
    log.push(format!("Engine::uses_occt = {}", engine.uses_occt()));

    if engine.uses_occt() {
        let t0 = Instant::now();
        match engine.warmup() {
            Ok(()) => log.push(format!("Engine::warmup ok in {:.2}s", t0.elapsed().as_secs_f64())),
            Err(e) => {
                let msg = format!("Engine::warmup failed: {e}");
                log.push(msg.clone());
                failed_cmds.push(msg);
            }
        }
    }

    // ── baseline execute ──────────────────────────────────────────────────
    let t0 = Instant::now();
    let baseline = match engine.execute(&program) {
        Ok(out) => {
            log.push(format!(
                "Engine::execute (golden M8) ok in {:.2}s  volume={:.3} bbox={:?} is_solid={} verts={}",
                t0.elapsed().as_secs_f64(),
                out.metrics.volume,
                out.metrics.bbox,
                out.metrics.is_solid,
                out.mesh.positions.len() / 3
            ));
            Some(out)
        }
        Err(e) => {
            let msg = format!("Engine::execute (golden M8) FAILED in {:.2}s: {e}", t0.elapsed().as_secs_f64());
            log.push(msg.clone());
            failed_cmds.push(msg);
            None
        }
    };

    let mesh_bbox = baseline
        .as_ref()
        .map(|o| bbox_from_mesh(&o.mesh))
        .unwrap_or([0.0; 6]);
    let head_base = baseline
        .as_ref()
        .map(|o| hex_head_metrics(&o.mesh, SHANK_R_MM));

    // Viewport mesh dump (OBJ)
    let obj_path = out_dir.join("m8_x40.obj");
    let obj_bytes = if let Some(ref out) = baseline {
        let s = to_obj(&out.mesh);
        fs::write(&obj_path, s.as_bytes()).map_err(|e| format!("write obj: {e}"))?;
        log.push(format!("wrote {} ({} bytes)", rel(&obj_path), s.len()));
        s.into_bytes()
    } else {
        Vec::new()
    };

    // STL from viewport mesh
    let stl_path = out_dir.join("m8_x40.stl");
    let stl_bytes = if let Some(ref out) = baseline {
        let b = to_stl(&out.mesh);
        fs::write(&stl_path, &b).map_err(|e| format!("write stl: {e}"))?;
        log.push(format!("wrote {} ({} bytes) via kernel::export::to_stl", rel(&stl_path), b.len()));
        b
    } else {
        Vec::new()
    };

    // Diagnostic STEP probes (not check a). Isolates "STEP is dead" vs "this bolt's STEP crashes".
    probe_step(&engine, &ir_value, 2, "hex-only (sketch+extrude)", &mut log);
    probe_step(&engine, &ir_value, 3, "hex+shank (no thread)", &mut log);

    // STEP from Engine::export (B-Rep). Long threads may instance rods on an
    // uncut host — STEP without helical grooves is OK (out of scope).
    let step_path = out_dir.join("m8_x40.step");
    let (step_bytes, step_export_err) = match engine.export(&program, &ExportFormat::Step) {
        Ok(b) => {
            fs::write(&step_path, &b).map_err(|e| format!("write step: {e}"))?;
            log.push(format!("wrote {} ({} bytes) via Engine::export Step", rel(&step_path), b.len()));
            (b, None)
        }
        Err(e) => {
            let msg = format!("Engine::export Step FAILED: {e}");
            log.push(msg.clone());
            failed_cmds.push(msg.clone());
            (Vec::new(), Some(msg))
        }
    };

    // Optional: B-Rep tessellation STL (not the viewport-mesh STL used for check b)
    match engine.export(&program, &ExportFormat::Stl) {
        Ok(b) => {
            let p = out_dir.join("m8_x40.export.stl");
            let _ = fs::write(&p, &b);
            log.push(format!(
                "also wrote {} ({} bytes) via Engine::export Stl (B-Rep tessellation; check b uses to_stl mesh)",
                rel(&p),
                b.len()
            ));
        }
        Err(e) => log.push(format!("Engine::export Stl (optional) failed: {e}")),
    }

    // Topology of hex head only (sketch+extrude) — fillet edge listing.
    let hex_only = hex_head_only(&ir_value);
    match serde_json::from_value::<CadProgram>(hex_only) {
        Ok(hex_prog) => match engine.list_topology(&hex_prog) {
            Ok(topo) => {
                log.push(format!(
                    "list_topology(hex head): faces={} edges={} tip={:?}",
                    topo.summary.face_count, topo.summary.edge_count, topo.summary.tip
                ));
                let p = out_dir.join("hex_head_topology.json");
                let _ = fs::write(
                    &p,
                    serde_json::to_string_pretty(&json!({
                        "summary": {
                            "face_count": topo.summary.face_count,
                            "edge_count": topo.summary.edge_count,
                            "largest_face": topo.summary.largest_face,
                            "top_face": topo.summary.top_face,
                            "bottom_face": topo.summary.bottom_face,
                            "longest_edge": topo.summary.longest_edge,
                            "tip": topo.summary.tip,
                        },
                        "edges": topo.edges.iter().take(64).map(|e| json!({
                            "index": e.index,
                            "length": e.length,
                            "mid": e.mid,
                            "curve_type": e.curve_type,
                            "tags": e.tags,
                        })).collect::<Vec<_>>(),
                    }))
                    .unwrap_or_else(|_| "{}".into()),
                );
            }
            Err(e) => log.push(format!("list_topology(hex head) failed: {e}")),
        },
        Err(e) => log.push(format!("hex-head-only CadProgram: {e}")),
    }

    // ── fillet: SAME IR + hex-head fillet after the hex extrude ───────────
    // Insert { op: fillet, radius, edges: all } after extrude (before shank),
    // matching the existing IR fillet used by thin_plate_fillet_does_not_spike.
    let fillet_value = with_hex_head_fillet(&ir_value, FILLET_RADIUS_MM);
    let fillet_ir_path = out_dir.join("m8_x40_fillet.json");
    let _ = fs::write(
        &fillet_ir_path,
        serde_json::to_string_pretty(&fillet_value).unwrap_or_else(|_| "{}".into()),
    );

    let fillet_prog: Result<CadProgram, _> = serde_json::from_value(fillet_value);
    let t1 = Instant::now();
    let fillet_result = match fillet_prog {
        Ok(fp) => match engine.execute(&fp) {
            Ok(out) => {
                log.push(format!(
                    "Engine::execute (M8 + hex-head fillet r={FILLET_RADIUS_MM} edges=all after extrude) ok in {:.2}s  volume={:.3} bbox={:?} is_solid={}",
                    t1.elapsed().as_secs_f64(),
                    out.metrics.volume,
                    out.metrics.bbox,
                    out.metrics.is_solid
                ));
                let obj = to_obj(&out.mesh);
                let _ = fs::write(out_dir.join("m8_x40_fillet.obj"), obj.as_bytes());
                Ok(out)
            }
            Err(e) => {
                let msg = format!(
                    "Engine::execute (M8 + hex-head fillet) FAILED in {:.2}s: {e}",
                    t1.elapsed().as_secs_f64()
                );
                log.push(msg.clone());
                Err(msg)
            }
        },
        Err(e) => Err(format!("fillet CadProgram: {e}")),
    };

    // ── checks ────────────────────────────────────────────────────────────
    let (step_pass, step_detail) = check_step_solid(&step_bytes, step_export_err.as_deref());
    let (stl_pass, stl_detail, stl_bbox) = check_stl_vs_mesh(&stl_bytes, mesh_bbox, baseline.as_ref());
    let (fillet_pass, fillet_detail, fillet_metrics, head_fillet) =
        check_fillet(&fillet_result, baseline.as_ref().map(|o| &o.metrics), head_base.as_ref());

    if !step_pass {
        failed_cmds.push(format!("STEP check: {step_detail}"));
    }
    if !stl_pass {
        failed_cmds.push(format!("STL check: {stl_detail}"));
    }
    if !fillet_pass {
        failed_cmds.push(format!("fillet check: {fillet_detail}"));
    }

    let all_pass = step_pass && stl_pass && fillet_pass;

    let report = ReportData {
        all_pass,
        step_pass,
        stl_pass,
        fillet_pass,
        step_detail,
        stl_detail,
        fillet_detail,
        step_bytes: step_bytes.len() as u64,
        stl_bytes: stl_bytes.len() as u64,
        obj_bytes: obj_bytes.len() as u64,
        baseline_metrics: baseline.as_ref().map(|o| o.metrics.clone()),
        mesh_bbox,
        stl_bbox,
        fillet_metrics,
        head_base,
        head_fillet,
        uses_occt: engine.uses_occt(),
        log,
        failed_cmds,
    };

    let md = render_markdown(&report);
    fs::write(crate_dir.join("REPORT.md"), &md).map_err(|e| format!("write REPORT.md: {e}"))?;
    fs::write(
        crate_dir.join("report.json"),
        serde_json::to_string_pretty(&report_json(&report)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write report.json: {e}"))?;

    println!("{md}");
    Ok(all_pass)
}

fn write_fatal_report(err: &str) -> Result<(), String> {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let md = format!(
        "# Inspector report: golden M8×40 fillet + STEP/STL\n\n\
         **Overall: FAIL** (inspector aborted)\n\n\
         Fatal error (not faked as pass):\n\n```\n{err}\n```\n"
    );
    fs::write(crate_dir.join("REPORT.md"), md).map_err(|e| e.to_string())?;
    Ok(())
}

fn rustc_version() -> Option<String> {
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()?;
    String::from_utf8(out.stdout).ok().map(|s| s.trim().to_string())
}

fn rel(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("?")
        .to_string()
}

fn probe_step(
    engine: &Engine,
    ir: &serde_json::Value,
    n_features: usize,
    label: &str,
    log: &mut Vec<String>,
) {
    let mut v = ir.clone();
    if let Some(arr) = v["features"].as_array_mut() {
        arr.truncate(n_features);
    }
    match serde_json::from_value::<CadProgram>(v) {
        Ok(p) => match engine.export(&p, &ExportFormat::Step) {
            Ok(b) => log.push(format!(
                "STEP probe {label}: ok, {} bytes (ISO-10303={})",
                b.len(),
                String::from_utf8_lossy(&b).contains("ISO-10303")
            )),
            Err(e) => log.push(format!("STEP probe {label} FAILED: {e}")),
        },
        Err(e) => log.push(format!("STEP probe {label} IR: {e}")),
    }
}

fn hex_head_only(ir: &serde_json::Value) -> serde_json::Value {
    let feats: Vec<serde_json::Value> = ir["features"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(2)
        .collect();
    json!({ "units": ir["units"], "features": feats })
}

fn with_hex_head_fillet(ir: &serde_json::Value, radius: f64) -> serde_json::Value {
    let mut ir = ir.clone();
    let features = ir["features"].as_array_mut().expect("features array");
    let insert_at = features
        .iter()
        .position(|f| f["op"] == "extrude")
        .map(|i| i + 1)
        .unwrap_or(features.len());
    features.insert(
        insert_at,
        json!({ "op": "fillet", "radius": radius, "edges": "all" }),
    );
    ir
}

fn bbox_from_mesh(mesh: &MeshData) -> [f64; 6] {
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut n = 0usize;
    for c in mesh.positions.chunks(3) {
        if c.len() < 3 {
            continue;
        }
        n += 1;
        let x = c[0] as f64;
        let y = c[1] as f64;
        let z = c[2] as f64;
        bb[0] = bb[0].min(x);
        bb[1] = bb[1].min(y);
        bb[2] = bb[2].min(z);
        bb[3] = bb[3].max(x);
        bb[4] = bb[4].max(y);
        bb[5] = bb[5].max(z);
    }
    if n == 0 {
        [0.0; 6]
    } else {
        bb
    }
}

#[derive(Clone, Debug)]
struct HexHead {
    count: usize,
    z0: f64,
    z1: f64,
    dz: f64,
    max_r: f64,
    min_r: f64,
}

fn hex_head_metrics(mesh: &MeshData, shank_r: f64) -> HexHead {
    let cut = shank_r + 0.45;
    let mut z0 = f64::MAX;
    let mut z1 = f64::MIN;
    let mut max_r: f64 = 0.0;
    let mut min_r: f64 = f64::MAX;
    let mut count = 0usize;
    for c in mesh.positions.chunks(3) {
        if c.len() < 3 {
            continue;
        }
        let r = (c[0] as f64).hypot(c[1] as f64);
        if r > cut {
            let z = c[2] as f64;
            z0 = z0.min(z);
            z1 = z1.max(z);
            max_r = max_r.max(r);
            min_r = min_r.min(r);
            count += 1;
        }
    }
    if count == 0 {
        HexHead {
            count: 0,
            z0: 0.0,
            z1: 0.0,
            dz: 0.0,
            max_r: 0.0,
            min_r: 0.0,
        }
    } else {
        HexHead {
            count,
            z0,
            z1,
            dz: z1 - z0,
            max_r,
            min_r,
        }
    }
}

fn check_step_solid(bytes: &[u8], export_err: Option<&str>) -> (bool, String) {
    if let Some(e) = export_err {
        return (false, format!("export failed: {e}"));
    }
    if bytes.is_empty() {
        return (false, "STEP file is empty".into());
    }
    let text = String::from_utf8_lossy(bytes);
    let has_iso = text.contains("ISO-10303") || text.contains("STEP");
    let has_solid = text.contains("MANIFOLD_SOLID_BREP")
        || text.contains("BREP_WITH_VOIDS")
        || text.contains("FACETED_BREP")
        || text.contains("CLOSED_SHELL");
    let nonempty = bytes.len() > 512;
    if has_iso && has_solid && nonempty {
        (
            true,
            format!(
                "non-empty STEP solid ({} bytes, MANIFOLD_SOLID_BREP/CLOSED_SHELL present). Helical grooves in STEP are out of scope.",
                bytes.len()
            ),
        )
    } else {
        (
            false,
            format!(
                "not a non-empty solid: bytes={} has_iso={has_iso} has_solid_token={has_solid}",
                bytes.len()
            ),
        )
    }
}

fn check_stl_vs_mesh(
    stl: &[u8],
    mesh_bbox: [f64; 6],
    baseline: Option<&kernel::engine::ModelOutput>,
) -> (bool, String, Option<[f64; 6]>) {
    if baseline.is_none() {
        return (false, "no viewport mesh (execute failed)".into(), None);
    }
    if stl.len() < 84 {
        return (false, format!("STL too small ({} bytes)", stl.len()), None);
    }
    let tri = u32::from_le_bytes(stl[80..84].try_into().unwrap()) as usize;
    if tri == 0 {
        return (false, "STL triangle count is 0".into(), None);
    }
    let Some(stl_bb) = stl_bbox(stl) else {
        return (false, "could not parse binary STL vertices".into(), None);
    };
    let ok_bbox = bbox_close(mesh_bbox, stl_bb, STL_BBOX_TOL_MM);
    if ok_bbox {
        (
            true,
            format!(
                "non-empty ({tri} tris, {} bytes); STL bbox {:?} matches mesh bbox {:?} within {STL_BBOX_TOL_MM} mm",
                stl.len(),
                fmt_bb(stl_bb),
                fmt_bb(mesh_bbox)
            ),
            Some(stl_bb),
        )
    } else {
        (
            false,
            format!(
                "non-empty ({tri} tris) but bbox mismatch: mesh {:?} vs STL {:?} (tol {STL_BBOX_TOL_MM} mm)",
                fmt_bb(mesh_bbox),
                fmt_bb(stl_bb)
            ),
            Some(stl_bb),
        )
    }
}

fn stl_bbox(stl: &[u8]) -> Option<[f64; 6]> {
    if stl.len() < 84 {
        return None;
    }
    let tri = u32::from_le_bytes(stl[80..84].try_into().ok()?) as usize;
    let need = 84 + tri * 50;
    if stl.len() < need {
        return None;
    }
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut any = false;
    for i in 0..tri {
        let off = 84 + i * 50;
        // skip 12-byte normal
        for v in 0..3 {
            let b = off + 12 + v * 12;
            let x = f32::from_le_bytes(stl[b..b + 4].try_into().ok()?) as f64;
            let y = f32::from_le_bytes(stl[b + 4..b + 8].try_into().ok()?) as f64;
            let z = f32::from_le_bytes(stl[b + 8..b + 12].try_into().ok()?) as f64;
            bb[0] = bb[0].min(x);
            bb[1] = bb[1].min(y);
            bb[2] = bb[2].min(z);
            bb[3] = bb[3].max(x);
            bb[4] = bb[4].max(y);
            bb[5] = bb[5].max(z);
            any = true;
        }
    }
    if any {
        Some(bb)
    } else {
        None
    }
}

fn bbox_close(a: [f64; 6], b: [f64; 6], tol: f64) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
}

fn fmt_bb(b: [f64; 6]) -> String {
    format!(
        "[{:.4}, {:.4}, {:.4}, {:.4}, {:.4}, {:.4}]",
        b[0], b[1], b[2], b[3], b[4], b[5]
    )
}

fn check_fillet(
    fillet: &Result<kernel::engine::ModelOutput, String>,
    baseline: Option<&MetricsData>,
    head_base: Option<&HexHead>,
) -> (
    bool,
    String,
    Option<MetricsData>,
    Option<HexHead>,
) {
    match fillet {
        Err(e) => (
            false,
            format!("fillet run FAILED (not a silent no-op): {e}"),
            None,
            None,
        ),
        Ok(out) => {
            let Some(base) = baseline else {
                return (
                    false,
                    "cannot compare fillet: baseline execute failed".into(),
                    Some(out.metrics.clone()),
                    Some(hex_head_metrics(&out.mesh, SHANK_R_MM)),
                );
            };
            let head = hex_head_metrics(&out.mesh, SHANK_R_MM);
            let dvol = (out.metrics.volume - base.volume).abs();
            let dbbox = base
                .bbox
                .iter()
                .zip(out.metrics.bbox.iter())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f64, f64::max);
            let d_max_r = head_base
                .map(|h| (h.max_r - head.max_r).abs())
                .unwrap_or(0.0);
            let d_dz = head_base.map(|h| (h.dz - head.dz).abs()).unwrap_or(0.0);
            let changed = dvol > FILLET_VOLUME_EPS_MM3
                || dbbox > FILLET_LEN_EPS_MM
                || d_max_r > FILLET_LEN_EPS_MM
                || d_dz > FILLET_LEN_EPS_MM;
            if changed {
                (
                    true,
                    format!(
                        "hex-head metrics changed vs unfilleted bolt: Δvolume={dvol:.4} mm³ Δbbox_max={dbbox:.4} mm Δmax_r={d_max_r:.4} mm Δhead_dz={d_dz:.4} mm (radius {FILLET_RADIUS_MM} mm, edges=all after hex extrude)"
                    ),
                    Some(out.metrics.clone()),
                    Some(head),
                )
            } else {
                (
                    false,
                    format!(
                        "SILENT NO-OP fillet (FAIL): execute succeeded but hex-head metrics unchanged (Δvolume={dvol:.6} Δbbox={dbbox:.6} Δmax_r={d_max_r:.6} Δhead_dz={d_dz:.6}; eps volume {FILLET_VOLUME_EPS_MM3} mm³ / length {FILLET_LEN_EPS_MM} mm). Kernel was not patched."
                    ),
                    Some(out.metrics.clone()),
                    Some(head),
                )
            }
        }
    }
}

struct ReportData {
    all_pass: bool,
    step_pass: bool,
    stl_pass: bool,
    fillet_pass: bool,
    step_detail: String,
    stl_detail: String,
    fillet_detail: String,
    step_bytes: u64,
    stl_bytes: u64,
    obj_bytes: u64,
    baseline_metrics: Option<MetricsData>,
    mesh_bbox: [f64; 6],
    stl_bbox: Option<[f64; 6]>,
    fillet_metrics: Option<MetricsData>,
    head_base: Option<HexHead>,
    head_fillet: Option<HexHead>,
    uses_occt: bool,
    log: Vec<String>,
    failed_cmds: Vec<String>,
}

fn mark(p: bool) -> &'static str {
    if p {
        "PASS"
    } else {
        "FAIL"
    }
}

fn render_markdown(r: &ReportData) -> String {
    let overall = if r.all_pass { "PASS" } else { "FAIL" };
    let mut s = String::new();
    s.push_str("# Inspector report: golden M8×40 fillet + STEP/STL\n\n");
    s.push_str(&format!("**Overall: {overall}**\n\n"));
    s.push_str("Inspector only. No kernel/web/OCCT-WASM edits. Fillet robustness was not patched. Helical grooves were not written into STEP.\n\n");
    s.push_str("## How to run\n\n");
    s.push_str("```bash\ncargo run --release --manifest-path tests/reports/Cargo.toml --features occt\n```\n\n");
    s.push_str("## Pass / fail\n\n");
    s.push_str("| Check | Result | Detail |\n|---|---|---|\n");
    s.push_str(&format!(
        "| a) STEP is a non-empty solid | {} | {} |\n",
        mark(r.step_pass),
        escape_md(&r.step_detail)
    ));
    s.push_str(&format!(
        "| b) STL non-empty AND same bbox as mesh (±{} mm) | {} | {} |\n",
        STL_BBOX_TOL_MM,
        mark(r.stl_pass),
        escape_md(&r.stl_detail)
    ));
    s.push_str(&format!(
        "| c) fillet changes hex-head metrics (silent no-op = FAIL) | {} | {} |\n",
        mark(r.fillet_pass),
        escape_md(&r.fillet_detail)
    ));
    s.push_str("\n## File sizes\n\n");
    s.push_str(&format!(
        "| File | Bytes |\n|---|---|\n| `out/m8_x40.obj` (viewport mesh) | {} |\n| `out/m8_x40.stl` (`kernel::export::to_stl`) | {} |\n| `out/m8_x40.step` (`Engine::export` STEP) | {} |\n",
        r.obj_bytes, r.stl_bytes, r.step_bytes
    ));
    s.push_str("\n## B-Rep / mesh metrics\n\n");
    s.push_str(&format!("`Engine::uses_occt` = {}\n\n", r.uses_occt));
    if let Some(m) = &r.baseline_metrics {
        s.push_str(&format!(
            "Golden M8 execute: volume = **{:.4}** mm³, is_solid = {}, kernel bbox = `{:?}`, mesh bbox = `{}`\n\n",
            m.volume,
            m.is_solid,
            m.bbox,
            fmt_bb(r.mesh_bbox)
        ));
    } else {
        s.push_str("Golden M8 execute: **did not produce metrics**.\n\n");
    }
    if let Some(bb) = r.stl_bbox {
        s.push_str(&format!("STL parsed bbox: `{}`\n\n", fmt_bb(bb)));
    }
    if let Some(h) = &r.head_base {
        s.push_str(&format!(
            "Hex-head (r > {:.2} mm): n={}, z=[{:.4}, {:.4}] dz={:.4} max_r={:.4} min_r={:.4}\n\n",
            SHANK_R_MM + 0.45,
            h.count,
            h.z0,
            h.z1,
            h.dz,
            h.max_r,
            h.min_r
        ));
    }
    if let Some(m) = &r.fillet_metrics {
        s.push_str(&format!(
            "Filleted execute: volume = **{:.4}** mm³, is_solid = {}, bbox = `{:?}`\n\n",
            m.volume, m.is_solid, m.bbox
        ));
    }
    if let Some(h) = &r.head_fillet {
        s.push_str(&format!(
            "Filleted hex-head: n={}, z=[{:.4}, {:.4}] dz={:.4} max_r={:.4} min_r={:.4}\n\n",
            h.count, h.z0, h.z1, h.dz, h.max_r, h.min_r
        ));
    }
    s.push_str("## IR\n\n");
    s.push_str("Golden: `tests/reports/m8_x40.json` (canonical `m8_hex_head_bolt_40mm_builds`).\n\n");
    s.push_str(&format!(
        "Fillet variant: same features with `{{ op: fillet, radius: {FILLET_RADIUS_MM}, edges: all }}` inserted after the hex extrude (hex-head fillet, existing IR op).\n\n"
    ));
    s.push_str("## Failed commands / why (not faked)\n\n");
    if r.failed_cmds.is_empty() {
        s.push_str("None.\n\n");
    } else {
        for c in &r.failed_cmds {
            s.push_str(&format!("- `{c}`\n"));
        }
        s.push('\n');
    }
    s.push_str("## Log\n\n");
    for line in &r.log {
        s.push_str(&format!("- {line}\n"));
    }
    s.push('\n');
    s
}

fn escape_md(t: &str) -> String {
    t.replace('|', "\\|").replace('\n', " ")
}

fn report_json(r: &ReportData) -> serde_json::Value {
    json!({
        "overall": mark(r.all_pass),
        "uses_occt": r.uses_occt,
        "checks": {
            "step_nonempty_solid": { "result": mark(r.step_pass), "detail": r.step_detail },
            "stl_bbox_matches_mesh": { "result": mark(r.stl_pass), "detail": r.stl_detail },
            "fillet_changes_hex_head": { "result": mark(r.fillet_pass), "detail": r.fillet_detail },
        },
        "files": {
            "obj_bytes": r.obj_bytes,
            "stl_bytes": r.stl_bytes,
            "step_bytes": r.step_bytes,
        },
        "baseline": r.baseline_metrics.as_ref().map(|m| json!({
            "volume": m.volume,
            "bbox": m.bbox,
            "is_solid": m.is_solid,
            "surface_area": m.surface_area,
            "mesh_bbox": r.mesh_bbox,
        })),
        "stl_bbox": r.stl_bbox,
        "fillet": r.fillet_metrics.as_ref().map(|m| json!({
            "volume": m.volume,
            "bbox": m.bbox,
            "is_solid": m.is_solid,
        })),
        "hex_head_baseline": r.head_base.as_ref().map(|h| json!({
            "count": h.count, "z0": h.z0, "z1": h.z1, "dz": h.dz, "max_r": h.max_r, "min_r": h.min_r
        })),
        "hex_head_fillet": r.head_fillet.as_ref().map(|h| json!({
            "count": h.count, "z0": h.z0, "z1": h.z1, "dz": h.dz, "max_r": h.max_r, "min_r": h.min_r
        })),
        "failed": r.failed_cmds,
        "log": r.log,
    })
}
