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

From the repo root (Rust **1.95+** / current `stable`; `occt-wasm` needs it):

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

Look-right acceptance (no OCCT; synthetic helix / smooth-rod / STEP / fillet R):

```bash
cargo test --manifest-path tests/reports/Cargo.toml
```

`--release` is strongly recommended for the runner: debug-mode wasmtime
compiling the OCCT WASM module is very slow. The `rust-toolchain.toml` in
this directory pins `stable`.

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
2. **STL look-right** — non-empty **and** same bbox as the viewport mesh **and**
   the same helix/ISO-V/sliver asserts. A smooth Ø8 rod with the same AABB
   must **FAIL** (AABB-only is not enough).
3. **STEP honesty** — empty or crash = **FAIL** (honest on current main).
   When STEP exists: if the viewport is threaded but STEP is essentially the
   uncut hex+shank (smooth Ø8 / no groove / volume≈uncut), **FAIL**.
   Inspector does not implement STEP.
4. **Fillet R** — under-head junction or named edges, measurable R and hex
   look change. Silent no-op = **FAIL**. Δvolume alone is **not** sufficient.
5. **ISO golden** — IR is AF 13 / Ø8 / P 1.25 / L 40 / head ~5.3.
