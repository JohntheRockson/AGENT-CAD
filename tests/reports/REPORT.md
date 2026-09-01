# Inspector report: look-right golden M8×40

**Overall: FAIL**

Inspector only. No kernel/web/OCCT-WASM edits. Kernel owns STEP implementation. A silent fillet no-op is FAIL. AABB-only STL of a smooth rod is FAIL. STEP that is empty/crash **or** ≈ the uncut hex+shank while the viewport is threaded is FAIL.

## How to run

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

Look-right unit tests (no OCCT):

```bash
cargo test --manifest-path tests/reports/Cargo.toml
```

## Pass / fail

| Check | Result | Detail |
|---|---|---|
| 0) ISO caliper golden (AF 13, Ø8, P 1.25, L 40, head ~5.3) | PASS | locked ISO caliper: AF 13, Ø8, P 1.25, L 40, head ~5.3 |
| 1) viewport look-right (helix / ISO-V / no sliver) | PASS | helix + ISO-V + no sliver: variation=0.238 spread=0.359 yaws=11 |
| 2) STL look-right (not AABB-only; smooth rod = FAIL) | PASS | non-empty (77990 tris); bbox [-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0000] matches mesh within 0.05 mm; helix/ISO-V/sliver ok |
| 3) STEP honesty (empty/crash = FAIL; uncut host while viewport threaded = FAIL) | FAIL | export failed (honest FAIL): Engine::export_document Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude. |
| 4) fillet under-head / named R (Δvol-only = FAIL; silent no-op = FAIL) | PASS | fillet R + hex look: named/hex-corner R≈0.8 mm (median err 0.124 mm, n=462); ΔAF=0.000 Δmin_r=0.924 Δmax_r=0.000 Δvolume=6.015 (Δvol alone is not sufficient) |

## File sizes

| File | Bytes |
|---|---|
| `out/m8_x40.obj` (viewport mesh) | 19559534 |
| `out/m8_x40.stl` (`kernel::export::to_stl`) | 3899584 |
| `out/m8_x40.step` (`Engine::export_document` STEP) | 0 |

## B-Rep / mesh metrics

`Engine::uses_occt` = true

Golden M8 execute: volume = **2519.9112** mm³, is_solid = true, kernel bbox = `[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.0]`, mesh bbox = `[-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0000]`

Look-right numbers: variation=0.2384 spread=0.3591 distinct_yaws=11

STL parsed bbox: `[-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0000]`

Hex-head (r > 4.45 mm): n=54, z=[0.0000, 5.3000] dz=5.3000 max_r=7.5056 min_r=7.5056 AF=13.0000

Filleted execute: volume = **2513.8961** mm³, is_solid = true, bbox = `[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.0]`

Filleted hex-head: n=462, z=[0.0000, 5.3000] dz=5.3000 max_r=7.5056 min_r=6.5818 AF=13.0000

## IR

Golden: `tests/reports/m8_x40.json` — locked ISO caliper **AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3**. Shared with `crates/kernel/tests/occt_geometry.rs` (`iso_m8_x40_golden_document`). See `GOLDEN.md`.

Fillet variant: same features with `{ op: fillet, radius: 0.8 }` inserted after the Ø8 cylinder (under-head junction if topology names edges; otherwise named `all`). Δvolume alone is not a pass.

## Failed commands / why (not faked)

- `Engine::export_document Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.`
- `STEP honesty: export failed (honest FAIL): Engine::export_document Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.`

## Log

- cwd=/workspace crate_dir=/workspace/tests/reports
- rustc=rustc 1.95.0 (59807616e 2026-04-14)
- ISO golden: AF 13, Ø8, P 1.25, L 40, head ~5.3 (see GOLDEN.md)
- built with feature `occt` (kernel/occt)
- golden IR: locked ISO caliper: AF 13, Ø8, P 1.25, L 40, head ~5.3
- Engine::uses_occt = true
- Engine::warmup ok in 11.52s
- Engine::execute_document (golden M8 AF13) ok in 10.80s  volume=2519.911 bbox=[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.0] is_solid=true verts=233970
- wrote m8_x40.obj (19559534 bytes)
- wrote m8_x40.stl (3899584 bytes) via kernel::export::to_stl
- STEP probe hex-only (sketch+extrude) FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- STEP probe hex+shank (no thread) FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- uncut hex+shank execute: volume=2519.911 bbox=[-7.505553499465135, -6.5, 0.0, 7.505553499465135, 6.500000000000001, 40.0]
- uncut hex+shank STEP failed: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- Engine::export_document Step FAILED: OCCT kernel error: export_step: internal CAD kernel crash (wasm memory). Body rotation and cylinders on X/Y are valid — do not drop those ops. Start each body with box, cylinder, sphere, cone, torus, fuse, or sketch+extrude.
- also wrote m8_x40.export.stl (3899584 bytes) via Engine::export_document Stl (look-right uses to_stl mesh)
- list_topology(hex+shank): faces=10 edges=21 tip="Use face: \"largest\"|\"top\"|\"bottom\"|<index> on cut/fuse/hole/sketch. Use edges: \"all\"|\"top\"|\"longest\"|[indices] on fillet/chamfer. Pattern holes with scope:\"feature\" after hole/cut."
- under-head junction edge indices: [1, 5, 7, 13, 14, 15]
- Engine::execute (M8 + under-head/named fillet r=0.8) ok in 10.85s  volume=2513.896 bbox=[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.0] is_solid=true

