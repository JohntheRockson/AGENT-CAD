# Inspector report: look-right golden M8×40

**Overall: FAIL**

Inspector only. No kernel/web/OCCT-WASM edits. Kernel owns STEP implementation. A silent fillet no-op is FAIL. Hex-corner R or Δvolume without under-head junction R is FAIL. AABB-only STL of a smooth rod is FAIL. A mid-shank helix/AABB bar with instance-window seams or a dead→thread entry notch is FAIL. STEP that is empty/crash **or** ≈ the uncut hex+shank while the viewport is threaded is FAIL.

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
| 1) viewport look-right (helix / ISO-V / no sliver) | PASS | helix + ISO-V + no sliver: variation=0.249 spread=0.351 yaws=5; continuous across instance windows; clean thread entry |
| 1b) instance-window helix continuity + clean thread entry | PASS | helix continuous across instance windows (worst < 0.10 turn, rms < 0.08); clean thread entry (first-turn groove on helix) |
| 2) STL look-right (not AABB-only; smooth rod / seamed slab = FAIL) | PASS | non-empty (99245 tris); bbox [-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0952] matches mesh within 0.05 mm; helix/ISO-V/sliver + continuous windows + clean entry ok |
| 3) STEP honesty (empty/crash = FAIL; uncut host while viewport threaded = FAIL) | PASS | non-empty STEP solid (84004491 bytes); groove signature present (not the uncut Ø8 host) |
| 4) fillet under-head junction R (hex-corner / Δvol-only = FAIL; silent no-op = FAIL) | FAIL | no measurable under-head junction R (FAIL): Δvolume=4.1234 hex_look=true under_head=none hex_corner=n=2820 err=0.121 ΔAF=0.0000 Δmin_r=0.9238. Fillet must show R≈0.8 mm at the head≈5.3 / Ø8 junction. Hex-corner R or Δvolume alone is not enough. |

## File sizes

| File | Bytes |
|---|---|
| `out/m8_x40.obj` (viewport mesh) | 24980219 |
| `out/m8_x40.stl` (`kernel::export::to_stl`) | 4962334 |
| `out/m8_x40.step` (`Engine::export_document` STEP) | 84004491 |

## B-Rep / mesh metrics

`Engine::uses_occt` = true

Golden M8 execute: volume = **2519.9112** mm³, is_solid = true, kernel bbox = `[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.095184326171875]`, mesh bbox = `[-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0952]`

Look-right numbers: variation=0.2491 spread=0.3509 distinct_yaws=5

STL parsed bbox: `[-7.5056, -6.5000, 0.0000, 7.5056, 6.5000, 40.0952]`

Hex-head (r > 4.45 mm): n=984, z=[0.0000, 5.3000] dz=5.3000 max_r=7.5056 min_r=7.5056 AF=13.0000

Filleted execute: volume = **2515.7878** mm³, is_solid = true, bbox = `[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.095184326171875]`

Filleted hex-head: n=2820, z=[0.0000, 5.3000] dz=5.3000 max_r=7.5056 min_r=6.5818 AF=13.0000

## IR

Golden: `tests/reports/m8_x40.json` — locked ISO caliper **AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3**. Shared with `crates/kernel/tests/occt_geometry.rs` (`iso_m8_x40_golden_document`). See `GOLDEN.md`.

Fillet variant: same features with `{ op: fillet, radius: 0.8 }` inserted after the Ø8 cylinder (under-head junction if topology names edges; otherwise named `all`). Acceptance requires measurable under-head junction R≈0.8 mm (head ~5.3 / Ø8). Hex-corner R or Δvolume alone is not a pass.

## Failed commands / why (not faked)

- `fillet R: no measurable under-head junction R (FAIL): Δvolume=4.1234 hex_look=true under_head=none hex_corner=n=2820 err=0.121 ΔAF=0.0000 Δmin_r=0.9238. Fillet must show R≈0.8 mm at the head≈5.3 / Ø8 junction. Hex-corner R or Δvolume alone is not enough.`

## Log

- cwd=/workspace crate_dir=/workspace/tests/reports
- rustc=rustc 1.95.0 (59807616e 2026-04-14)
- ISO golden: AF 13, Ø8, P 1.25, L 40, head ~5.3 (see GOLDEN.md)
- built with feature `occt` (kernel/occt)
- golden IR: locked ISO caliper: AF 13, Ø8, P 1.25, L 40, head ~5.3
- Engine::uses_occt = true
- Engine::warmup ok in 0.59s
- Engine::execute_document (golden M8 AF13) ok in 8.38s  volume=2519.911 bbox=[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.095184326171875] is_solid=true verts=297735
- wrote m8_x40.obj (24980219 bytes)
- wrote m8_x40.stl (4962334 bytes) via kernel::export::to_stl
- STEP probe hex-only (sketch+extrude): ok, 15542 bytes (ISO-10303=true)
- STEP probe hex+shank (no thread): ok, 134082 bytes (ISO-10303=true)
- uncut hex+shank execute: volume=2519.911 bbox=[-7.505553499465135, -6.5, 0.0, 7.505553499465135, 6.500000000000001, 40.0]
- uncut hex+shank STEP: 134082 bytes
- wrote m8_x40.step (84004491 bytes) via Engine::export_document Step
- also wrote m8_x40.export.stl (3958534 bytes) via Engine::export_document Stl (look-right uses to_stl mesh)
- list_topology(hex+shank): faces=10 edges=21 tip="Use face: \"largest\"|\"top\"|\"bottom\"|<index> on cut/fuse/hole/sketch. Use edges: \"all\"|\"top\"|\"longest\"|[indices] on fillet/chamfer. Pattern holes with scope:\"feature\" after hole/cut."
- under-head junction edge indices: [1, 5, 7, 13, 14, 15]
- Engine::execute (M8 + under-head/named fillet r=0.8) ok in 8.65s  volume=2515.788 bbox=[-7.505553722381592, -6.5, 0.0, 7.505553722381592, 6.5, 40.095184326171875] is_solid=true

