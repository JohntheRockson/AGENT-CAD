# Inspector report: golden M8×40 fillet + STEP/STL

**Overall: FAIL**

Inspector only. No kernel/web/OCCT-WASM edits. Fillet robustness was not patched. Helical grooves were not written into STEP.

## How to run

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

## Pass / fail

| Check | Result | Detail |
|---|---|---|
| a) STEP is a non-empty solid | FAIL | export failed: Engine::export Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude. |
| b) STL non-empty AND same bbox as mesh (±0.05 mm) | PASS | non-empty (77564 tris, 3878284 bytes); STL bbox "[-5.7735, -5.0000, 0.0000, 5.7735, 5.0000, 40.0000]" matches mesh bbox "[-5.7735, -5.0000, 0.0000, 5.7735, 5.0000, 40.0000]" within 0.05 mm |
| c) fillet changes hex-head metrics (silent no-op = FAIL) | PASS | hex-head metrics changed vs unfilleted bolt: Δvolume=6.9973 mm³ Δbbox_max=0.1238 mm Δmax_r=0.0000 mm Δhead_dz=0.0000 mm (radius 0.8 mm, edges=all after hex extrude) |

## File sizes

| File | Bytes |
|---|---|
| `out/m8_x40.obj` (viewport mesh) | 19448758 |
| `out/m8_x40.stl` (`kernel::export::to_stl`) | 3878284 |
| `out/m8_x40.step` (`Engine::export` STEP) | 0 |

## B-Rep / mesh metrics

`Engine::uses_occt` = true

Golden M8 execute: volume = **2210.4731** mm³, is_solid = true, kernel bbox = `[-5.773502826690674, -5.0, 0.0, 5.773502826690674, 5.0, 40.0]`, mesh bbox = `[-5.7735, -5.0000, 0.0000, 5.7735, 5.0000, 40.0000]`

STL parsed bbox: `[-5.7735, -5.0000, 0.0000, 5.7735, 5.0000, 40.0000]`

Hex-head (r > 4.45 mm): n=100, z=[0.0000, 5.5000] dz=5.5000 max_r=5.7735 min_r=5.7735

Filleted execute: volume = **2203.4758** mm³, is_solid = true, bbox = `[-5.649742126464844, -5.0, -1.1102230246251565e-16, 5.649742126464844, 5.0, 40.0]`

Filleted hex-head: n=2008, z=[-0.0000, 5.5000] dz=5.5000 max_r=5.7735 min_r=4.8497

## IR

Golden: `tests/reports/m8_x40.json` (canonical `m8_hex_head_bolt_40mm_builds`).

Fillet variant: same features with `{ op: fillet, radius: 0.8, edges: all }` inserted after the hex extrude (hex-head fillet, existing IR op).

## Failed commands / why (not faked)

- `Engine::export Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.`
- `STEP check: export failed: Engine::export Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.`

## Log

- cwd=/workspace crate_dir=/workspace/tests/reports
- rustc=rustc 1.98.0 (88d9e12ae 2026-08-18)
- built with feature `occt` (kernel/occt)
- Engine::uses_occt = true
- Engine::warmup ok in 0.45s
- Engine::execute (golden M8) ok in 10.43s  volume=2210.473 bbox=[-5.773502826690674, -5.0, 0.0, 5.773502826690674, 5.0, 40.0] is_solid=true verts=232692
- wrote m8_x40.obj (19448758 bytes)
- wrote m8_x40.stl (3878284 bytes) via kernel::export::to_stl
- STEP probe hex-only (sketch+extrude) FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- STEP probe hex+shank (no thread) FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- Engine::export Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- also wrote m8_x40.export.stl (3878284 bytes) via Engine::export Stl (B-Rep tessellation; check b uses to_stl mesh)
- list_topology(hex head): faces=8 edges=18 tip="Use face: \"largest\"|\"top\"|\"bottom\"|<index> on cut/fuse/hole/sketch. Use edges: \"all\"|\"top\"|\"longest\"|[indices] on fillet/chamfer. Pattern holes with scope:\"feature\" after hole/cut."
- Engine::execute (M8 + hex-head fillet r=0.8 edges=all after extrude) ok in 10.68s  volume=2203.476 bbox=[-5.649742126464844, -5.0, -1.1102230246251565e-16, 5.649742126464844, 5.0, 40.0] is_solid=true

