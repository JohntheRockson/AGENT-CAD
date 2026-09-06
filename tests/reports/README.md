# Inspector: look-right golden M8×40

Standalone package. Public `kernel` APIs only — no edits to
`crates/kernel/src`, `vendor/occt-wasm`, or `apps/web`. Kernel owns STEP
implementation (faceted export / #15). Inspector must not fake a PASS.

**One ISO caliper golden** (do not use AF 10): see [`GOLDEN.md`](GOLDEN.md)
and [`m8_x40.json`](m8_x40.json). Locked with Kernel via
`iso_m8_x40_golden_document` in `crates/kernel/tests/occt_geometry.rs`.

| Caliper | Value |
|---|---|
| Across flats | **13 mm** |
| Shank / major Ø | **8 mm** |
| Pitch P | **1.25 mm** |
| Length L | **40 mm** |
| Head height | **~5.3 mm** |

## One command

From the repo root (Rust **1.95.0**; `occt-wasm` needs edition 2024 / rust-version 1.95):

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

Look-right acceptance (no OCCT; synthetic helix / smooth-rod / STEP / fillet R):

```bash
cargo test --manifest-path tests/reports/Cargo.toml
```

`--release` is strongly recommended for the runner: debug-mode wasmtime
compiling the OCCT WASM module is very slow. The `rust-toolchain.toml` in
this directory pins **1.95.0** (same as the repo root) so the invocation does
not pick up an older or drifting `stable` toolchain.

Outputs (gitignored meshes, committed report):

- `tests/reports/out/m8_x40.obj` — viewport mesh
- `tests/reports/out/m8_x40.stl` — `kernel::export::to_stl` of that mesh
- `tests/reports/out/m8_x40.step` — STEP via `Engine::export_document`
- `tests/reports/REPORT.md` — pass/fail
- `tests/reports/report.json` — same facts as JSON

Exit code 0 only if **all** checks pass.

## Pass / fail

1. **Viewport look-right** — helix (`angular_radius_spread`, `distinct_groove_yaws`),
   ISO-V profile, no vertical uncut strip. Stacked ticks fail.
   Also **instance-window continuity** (deep-root helix phase worst < 0.10 turn,
   rms < 0.08) and a **clean thread entry** (first turn is a groove on the helix,
   not leftover cylinder / pipe-entry notch). Mid-shank AABB/helix can no longer
   PASS with visible slab jumps — same bar as kernel #22
   (`assert_helix_continuous_across_instance_windows` /
   `assert_clean_thread_entry`).
2. **STL look-right** — non-empty **and** same bbox as the viewport mesh **and**
   the same helix/ISO-V/sliver **and** continuity/entry asserts. A smooth Ø8 rod
   or a seamed-slab helix with the same AABB must **FAIL** (AABB-only is not enough).
3. **STEP honesty** — empty or crash = **FAIL** (honest on current main).
   When STEP exists: if the viewport is threaded but STEP is essentially the
   uncut hex+shank (smooth Ø8 / no groove / volume≈uncut), **FAIL**.
   Inspector does not implement STEP.
4. **Fillet R** — measurable **under-head junction** R≈0.8 mm at head ~5.3 / Ø8.
   Named `"all"` / junction-edge indices still have to show that torus.
   Silent no-op = **FAIL**. Δvolume alone is **not** sufficient. Hex-corner
   XY R (AF13 vertex inset) is **not** sufficient.
5. **ISO golden** — IR is AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3.
