//! Golden M8×40 inspector: execute IR, dump mesh, write STEP/STL, try an
//! under-head / named-edge fillet, and emit an honest pass/fail report.
//!
//! Public kernel APIs only. Do not patch the kernel. Kernel owns STEP.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use inspect_m8::fillet_r::{
    check_fillet, insert_fillet_after_cylinder, under_head_edge_indices, FilletEdges,
};
use inspect_m8::golden::{
    check_golden_ir, load_golden_document, FILLET_RADIUS_MM, SHANK_R_MM,
};
use inspect_m8::look_right::{bbox_tol_mm, check_stl_look_right, check_viewport_look_right};
use inspect_m8::mesh_util::{bbox_from_mesh, fmt_bb, hex_head_metrics, HexHead};
use inspect_m8::step_honest::check_step_honest;
use kernel::engine::{Engine, ExportFormat, MetricsData};
use kernel::export::{to_obj, to_stl};
use kernel::ir::CadDocument;
use serde_json::json;

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
    log.push("ISO golden: AF 13, Ø8, P 1.25, L 40, head ~5.3 (see GOLDEN.md)".into());

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
    let document = load_golden_document(&ir_text)?;
    let (golden_pass, golden_detail) = check_golden_ir(&document);
    log.push(format!("golden IR: {golden_detail}"));
    if !golden_pass {
        failed_cmds.push(format!("ISO golden: {golden_detail}"));
    }

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

    let t0 = Instant::now();
    let baseline = match engine.execute_document(&document) {
        Ok(out) => match out.into_model_output() {
            Ok(model) => {
                log.push(format!(
                    "Engine::execute_document (golden M8 AF13) ok in {:.2}s  volume={:.3} bbox={:?} is_solid={} verts={}",
                    t0.elapsed().as_secs_f64(),
                    model.metrics.volume,
                    model.metrics.bbox,
                    model.metrics.is_solid,
                    model.mesh.positions.len() / 3
                ));
                Some(model)
            }
            Err(e) => {
                let msg = format!("golden document produced no mesh: {e}");
                log.push(msg.clone());
                failed_cmds.push(msg);
                None
            }
        },
        Err(e) => {
            let msg = format!(
                "Engine::execute_document (golden M8) FAILED in {:.2}s: {e}",
                t0.elapsed().as_secs_f64()
            );
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

    let obj_path = out_dir.join("m8_x40.obj");
    let obj_bytes = if let Some(ref out) = baseline {
        let s = to_obj(&out.mesh);
        fs::write(&obj_path, s.as_bytes()).map_err(|e| format!("write obj: {e}"))?;
        log.push(format!("wrote {} ({} bytes)", rel(&obj_path), s.len()));
        s.into_bytes()
    } else {
        Vec::new()
    };

    let stl_path = out_dir.join("m8_x40.stl");
    let stl_bytes = if let Some(ref out) = baseline {
        let b = to_stl(&out.mesh);
        fs::write(&stl_path, &b).map_err(|e| format!("write stl: {e}"))?;
        log.push(format!(
            "wrote {} ({} bytes) via kernel::export::to_stl",
            rel(&stl_path),
            b.len()
        ));
        b
    } else {
        Vec::new()
    };

    probe_step(&engine, &ir_value, 2, "hex-only (sketch+extrude)", &mut log);
    probe_step(&engine, &ir_value, 3, "hex+shank (no thread)", &mut log);

    let uncut_doc = truncate_after_cylinder(&document);
    let uncut_out = engine.execute_document(&uncut_doc).ok();
    let uncut_metrics = uncut_out.as_ref().map(|o| o.metrics.clone());
    if let Some(ref m) = uncut_metrics {
        log.push(format!(
            "uncut hex+shank execute: volume={:.3} bbox={:?}",
            m.volume, m.bbox
        ));
    }

    let (uncut_step, _) = match engine.export_document(&uncut_doc, &ExportFormat::Step) {
        Ok(b) => {
            log.push(format!("uncut hex+shank STEP: {} bytes", b.len()));
            (Some(b), None)
        }
        Err(e) => {
            log.push(format!("uncut hex+shank STEP failed: {e}"));
            (None, Some(e.to_string()))
        }
    };

    let step_path = out_dir.join("m8_x40.step");
    let (step_bytes, step_export_err) = match engine.export_document(&document, &ExportFormat::Step)
    {
        Ok(b) => {
            fs::write(&step_path, &b).map_err(|e| format!("write step: {e}"))?;
            log.push(format!(
                "wrote {} ({} bytes) via Engine::export_document Step",
                rel(&step_path),
                b.len()
            ));
            (b, None)
        }
        Err(e) => {
            let msg = format!("Engine::export_document Step FAILED: {e}");
            log.push(msg.clone());
            failed_cmds.push(msg.clone());
            (Vec::new(), Some(msg))
        }
    };

    match engine.export_document(&document, &ExportFormat::Stl) {
        Ok(b) => {
            let p = out_dir.join("m8_x40.export.stl");
            let _ = fs::write(&p, &b);
            log.push(format!(
                "also wrote {} ({} bytes) via Engine::export_document Stl (look-right uses to_stl mesh)",
                rel(&p),
                b.len()
            ));
        }
        Err(e) => log.push(format!("Engine::export_document Stl (optional) failed: {e}")),
    }

    let hex_shank_prog = uncut_doc.as_program();
    let mut under_head_edges: Vec<usize> = Vec::new();
    match engine.list_topology(&hex_shank_prog) {
        Ok(topo) => {
            log.push(format!(
                "list_topology(hex+shank): faces={} edges={} tip={:?}",
                topo.summary.face_count, topo.summary.edge_count, topo.summary.tip
            ));
            under_head_edges = under_head_edge_indices(&topo, inspect_m8::golden::HEAD_HEIGHT_MM, SHANK_R_MM);
            log.push(format!(
                "under-head junction edge indices: {under_head_edges:?}"
            ));
            let p = out_dir.join("hex_shank_topology.json");
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
                    },
                    "under_head_edges": under_head_edges,
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
        Err(e) => log.push(format!("list_topology(hex+shank) failed: {e}")),
    }

    let edges = if under_head_edges.is_empty() {
        log.push("no under-head edges from topology — fillet edges=\"all\" after cylinder (named fallback)".into());
        FilletEdges::Named("all".into())
    } else {
        FilletEdges::Indices(under_head_edges.clone())
    };
    let fillet_value = insert_fillet_after_cylinder(&ir_value, FILLET_RADIUS_MM, &edges);
    let fillet_ir_path = out_dir.join("m8_x40_fillet.json");
    let _ = fs::write(
        &fillet_ir_path,
        serde_json::to_string_pretty(&fillet_value).unwrap_or_else(|_| "{}".into()),
    );

    let t1 = Instant::now();
    let fillet_result = match CadDocument::from_json_value(fillet_value) {
        Ok(fp) => match engine.execute_document(&fp) {
            Ok(out) => match out.into_model_output() {
                Ok(model) => {
                    log.push(format!(
                        "Engine::execute (M8 + under-head/named fillet r={FILLET_RADIUS_MM}) ok in {:.2}s  volume={:.3} bbox={:?} is_solid={}",
                        t1.elapsed().as_secs_f64(),
                        model.metrics.volume,
                        model.metrics.bbox,
                        model.metrics.is_solid
                    ));
                    let obj = to_obj(&model.mesh);
                    let _ = fs::write(out_dir.join("m8_x40_fillet.obj"), obj.as_bytes());
                    Ok((model.metrics.clone(), model.mesh))
                }
                Err(e) => Err(format!("fillet document produced no mesh: {e}")),
            },
            Err(e) => {
                let msg = format!(
                    "Engine::execute (M8 + fillet) FAILED in {:.2}s: {e}",
                    t1.elapsed().as_secs_f64()
                );
                log.push(msg.clone());
                Err(msg)
            }
        },
        Err(e) => Err(format!("fillet CadDocument: {e}")),
    };

    let look = baseline
        .as_ref()
        .map(|o| check_viewport_look_right(&o.mesh))
        .unwrap_or_else(|| inspect_m8::look_right::LookRight {
            ok: false,
            detail: "no viewport mesh".into(),
            variation: 0.0,
            spread: 0.0,
            n_yaws: 0,
            sliver_ok: false,
            iso_v_ok: false,
        });
    let (stl_pass, stl_detail, stl_bbox) =
        check_stl_look_right(&stl_bytes, mesh_bbox, baseline.as_ref().map(|o| &o.mesh));
    let step = check_step_honest(
        &step_bytes,
        step_export_err.as_deref(),
        baseline.as_ref().map(|o| &o.mesh),
        uncut_metrics.as_ref(),
        uncut_step.as_deref(),
    );
    let (fillet_pass, fillet_detail, fillet_metrics, head_fillet) = check_fillet(
        &fillet_result,
        baseline.as_ref().map(|o| &o.metrics),
        head_base.as_ref(),
        baseline.as_ref().map(|o| &o.mesh),
    );

    if !look.ok {
        failed_cmds.push(format!("look-right: {}", look.detail));
    }
    if !stl_pass {
        failed_cmds.push(format!("STL look-right: {stl_detail}"));
    }
    if !step.ok {
        failed_cmds.push(format!("STEP honesty: {}", step.detail));
    }
    if !fillet_pass {
        failed_cmds.push(format!("fillet R: {fillet_detail}"));
    }

    let all_pass = golden_pass && look.ok && stl_pass && step.ok && fillet_pass;

    let report = ReportData {
        all_pass,
        golden_pass,
        look_pass: look.ok,
        step_pass: step.ok,
        stl_pass,
        fillet_pass,
        golden_detail,
        look_detail: look.detail.clone(),
        step_detail: step.detail.clone(),
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
        look_variation: look.variation,
        look_spread: look.spread,
        look_yaws: look.n_yaws,
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
        "# Inspector report: look-right golden M8×40\n\n\
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

fn truncate_after_cylinder(doc: &CadDocument) -> CadDocument {
    let mut doc = doc.clone();
    for body in &mut doc.bodies {
        if let Some(i) = body.features.iter().position(|f| {
            matches!(f, kernel::ir::Feature::Cylinder(_))
        }) {
            body.features.truncate(i + 1);
        }
    }
    doc
}

fn probe_step(
    engine: &Engine,
    ir: &serde_json::Value,
    n_features: usize,
    label: &str,
    log: &mut Vec<String>,
) {
    let mut v = ir.clone();
    let features = if let Some(arr) = v
        .get_mut("bodies")
        .and_then(|b| b.as_array_mut())
        .and_then(|b| b.first_mut())
        .and_then(|b| b.get_mut("features"))
        .and_then(|f| f.as_array_mut())
    {
        arr.truncate(n_features);
        true
    } else if let Some(arr) = v.get_mut("features").and_then(|f| f.as_array_mut()) {
        arr.truncate(n_features);
        true
    } else {
        false
    };
    if !features {
        log.push(format!("STEP probe {label}: no features array"));
        return;
    }
    match CadDocument::from_json_value(v) {
        Ok(p) => match engine.export_document(&p, &ExportFormat::Step) {
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

struct ReportData {
    all_pass: bool,
    golden_pass: bool,
    look_pass: bool,
    step_pass: bool,
    stl_pass: bool,
    fillet_pass: bool,
    golden_detail: String,
    look_detail: String,
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
    look_variation: f64,
    look_spread: f64,
    look_yaws: usize,
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
    s.push_str("# Inspector report: look-right golden M8×40\n\n");
    s.push_str(&format!("**Overall: {overall}**\n\n"));
    s.push_str(
        "Inspector only. No kernel/web/OCCT-WASM edits. Kernel owns STEP implementation. \
         A silent fillet no-op is FAIL. AABB-only STL of a smooth rod is FAIL. \
         STEP that is empty/crash **or** ≈ the uncut hex+shank while the viewport is threaded is FAIL.\n\n",
    );
    s.push_str("## How to run\n\n");
    s.push_str("```bash\ncargo run --release --manifest-path tests/reports/Cargo.toml --features occt\n```\n\n");
    s.push_str("Look-right unit tests (no OCCT):\n\n");
    s.push_str("```bash\ncargo test --manifest-path tests/reports/Cargo.toml\n```\n\n");
    s.push_str("## Pass / fail\n\n");
    s.push_str("| Check | Result | Detail |\n|---|---|---|\n");
    s.push_str(&format!(
        "| 0) ISO caliper golden (AF 13, Ø8, P 1.25, L 40, head ~5.3) | {} | {} |\n",
        mark(r.golden_pass),
        escape_md(&r.golden_detail)
    ));
    s.push_str(&format!(
        "| 1) viewport look-right (helix / ISO-V / no sliver) | {} | {} |\n",
        mark(r.look_pass),
        escape_md(&r.look_detail)
    ));
    s.push_str(&format!(
        "| 2) STL look-right (not AABB-only; smooth rod = FAIL) | {} | {} |\n",
        mark(r.stl_pass),
        escape_md(&r.stl_detail)
    ));
    s.push_str(&format!(
        "| 3) STEP honesty (empty/crash = FAIL; uncut host while viewport threaded = FAIL) | {} | {} |\n",
        mark(r.step_pass),
        escape_md(&r.step_detail)
    ));
    s.push_str(&format!(
        "| 4) fillet under-head / named R (Δvol-only = FAIL; silent no-op = FAIL) | {} | {} |\n",
        mark(r.fillet_pass),
        escape_md(&r.fillet_detail)
    ));
    s.push_str("\n## File sizes\n\n");
    s.push_str(&format!(
        "| File | Bytes |\n|---|---|\n| `out/m8_x40.obj` (viewport mesh) | {} |\n| `out/m8_x40.stl` (`kernel::export::to_stl`) | {} |\n| `out/m8_x40.step` (`Engine::export_document` STEP) | {} |\n",
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
    s.push_str(&format!(
        "Look-right numbers: variation={:.4} spread={:.4} distinct_yaws={}\n\n",
        r.look_variation, r.look_spread, r.look_yaws
    ));
    if let Some(bb) = r.stl_bbox {
        s.push_str(&format!("STL parsed bbox: `{}`\n\n", fmt_bb(bb)));
    }
    if let Some(h) = &r.head_base {
        s.push_str(&format!(
            "Hex-head (r > {:.2} mm): n={}, z=[{:.4}, {:.4}] dz={:.4} max_r={:.4} min_r={:.4} AF={:.4}\n\n",
            SHANK_R_MM + 0.45,
            h.count,
            h.z0,
            h.z1,
            h.dz,
            h.max_r,
            h.min_r,
            h.across_flats
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
            "Filleted hex-head: n={}, z=[{:.4}, {:.4}] dz={:.4} max_r={:.4} min_r={:.4} AF={:.4}\n\n",
            h.count, h.z0, h.z1, h.dz, h.max_r, h.min_r, h.across_flats
        ));
    }
    s.push_str("## IR\n\n");
    s.push_str(
        "Golden: `tests/reports/m8_x40.json` — locked ISO caliper **AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3**. \
         Shared with `crates/kernel/tests/occt_geometry.rs` (`iso_m8_x40_golden_document`). See `GOLDEN.md`.\n\n",
    );
    s.push_str(&format!(
        "Fillet variant: same features with `{{ op: fillet, radius: {FILLET_RADIUS_MM} }}` inserted after the \
         Ø8 cylinder (under-head junction if topology names edges; otherwise named `all`). \
         Δvolume alone is not a pass.\n\n"
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
    let _ = bbox_tol_mm();
    s
}

fn escape_md(t: &str) -> String {
    t.replace('|', "\\|").replace('\n', " ")
}

fn report_json(r: &ReportData) -> serde_json::Value {
    json!({
        "overall": mark(r.all_pass),
        "uses_occt": r.uses_occt,
        "golden": {
            "af_mm": 13.0,
            "shank_d_mm": 8.0,
            "pitch_mm": 1.25,
            "length_mm": 40.0,
            "head_height_mm": 5.3,
        },
        "checks": {
            "iso_caliper_golden": { "result": mark(r.golden_pass), "detail": r.golden_detail },
            "viewport_look_right": { "result": mark(r.look_pass), "detail": r.look_detail },
            "stl_look_right_not_aabb_only": { "result": mark(r.stl_pass), "detail": r.stl_detail },
            "step_honesty": { "result": mark(r.step_pass), "detail": r.step_detail },
            "fillet_under_head_or_named_r": { "result": mark(r.fillet_pass), "detail": r.fillet_detail },
        },
        "look_right": {
            "variation": r.look_variation,
            "spread": r.look_spread,
            "distinct_yaws": r.look_yaws,
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
            "count": h.count, "z0": h.z0, "z1": h.z1, "dz": h.dz,
            "max_r": h.max_r, "min_r": h.min_r, "across_flats": h.across_flats
        })),
        "hex_head_fillet": r.head_fillet.as_ref().map(|h| json!({
            "count": h.count, "z0": h.z0, "z1": h.z1, "dz": h.dz,
            "max_r": h.max_r, "min_r": h.min_r, "across_flats": h.across_flats
        })),
        "failed": r.failed_cmds,
        "log": r.log,
    })
}
