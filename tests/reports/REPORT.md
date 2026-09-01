# Inspector report: look-right golden M8×40

**Overall: FAIL** (OCCT runner not yet re-executed on this revision; look-right *unit* tests passed)

Inspector only. No kernel/web/OCCT-WASM edits. Kernel owns STEP implementation.
A silent fillet no-op is FAIL. AABB-only STL of a smooth rod is FAIL.
STEP that is empty/crash **or** ≈ the uncut hex+shank while the viewport is
threaded is FAIL.

## How to run

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

Look-right unit tests (no OCCT):

```bash
cargo test --manifest-path tests/reports/Cargo.toml
```

## Pass / fail (acceptance, no OCCT)

| Check | Result | Detail |
|---|---|---|
| 0) ISO caliper golden (AF 13, Ø8, P 1.25, L 40, head ~5.3) | PASS | `m8_x40.json` locked; AF 10 rejected |
| 1) viewport look-right (helix / ISO-V / no sliver) | PASS* | synthetic ISO helix PASS; smooth rod FAIL |
| 2) STL look-right (not AABB-only; smooth rod = FAIL) | PASS* | AABB-matched smooth-rod STL FAIL |
| 3) STEP honesty (empty/crash = FAIL; uncut host while viewport threaded = FAIL) | PASS* | empty/crash FAIL; faceted uncut host FAIL; threaded faceted PASS |
| 4) fillet under-head / named R (Δvol-only = FAIL; silent no-op = FAIL) | PASS* | Δvol-only FAIL; torus R + hex look PASS |

\* Unit-test acceptance on synthetic fixtures. Re-run the OCCT command above
to fill viewport/STEP/fillet results on current main (STEP crash/empty remains
an honest FAIL until Kernel lands a *threaded* STEP, not an uncut host).

## IR

Golden: `tests/reports/m8_x40.json` — locked ISO caliper **AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3**.
Shared with `crates/kernel/tests/occt_geometry.rs` (`iso_m8_x40_golden_document`). See `GOLDEN.md`.
