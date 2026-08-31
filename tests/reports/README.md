# Inspector: golden M8×40 fillet + STEP/STL

Standalone package. It calls public `kernel` APIs only — no edits to
`crates/kernel/src`, `vendor/occt-wasm`, or `apps/web`.

Canonical recipe (non-binding; verified against
`crates/kernel/tests/occt_geometry.rs` test `m8_hex_head_bolt_40mm_builds`):

- hex sketch `across_flats` 10 → extrude 5.5
- overlapping cylinder diameter 8 height 35.5 at `[0,0,4.5]`
- thread kind external size M8 length 34.5 at `[0,0,5.5]`
- units mm

IR: [`m8_x40.json`](m8_x40.json)

## One command

From the repo root (Rust **1.95+** / current `stable`; `occt-wasm` needs it):

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

`--release` is strongly recommended: debug-mode wasmtime compiling the OCCT
WASM module is very slow. The `rust-toolchain.toml` in this directory pins
`stable` so the invocation does not pick up an older default toolchain.

Outputs (gitignored meshes, committed report):

- `tests/reports/out/m8_x40.obj` — viewport mesh
- `tests/reports/out/m8_x40.stl` — `kernel::export::to_stl` of that mesh
- `tests/reports/out/m8_x40.step` — B-Rep STEP via `Engine::export`
- `tests/reports/REPORT.md` — pass/fail, sizes, bbox/volume
- `tests/reports/report.json` — same facts as JSON

Exit code 0 only if STEP, STL, and fillet checks all pass. A silent no-op
fillet is **FAIL**. This inspector does not patch the kernel.

Long external threads may instance rods on an uncut host. STEP of the B-Rep
host without helical grooves is in-scope-OK (out of scope to put grooves
into STEP).
