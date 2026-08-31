# Inspector report: golden M8×40 fillet + STEP/STL

**Overall: NOT RUN** (template — replace by running the one command)

This file is overwritten by:

```bash
cargo run --release --manifest-path tests/reports/Cargo.toml --features occt
```

Do not treat this template as a pass. If the command cannot run (missing OCCT/WASM, old rustc, compile error), keep the failure text below and do not fake PASS.

## Pass / fail

| Check | Result | Detail |
|---|---|---|
| a) STEP is a non-empty solid | NOT RUN | |
| b) STL non-empty AND same bbox as mesh | NOT RUN | |
| c) fillet changes hex-head metrics (silent no-op = FAIL) | NOT RUN | |

## Failed commands / why

- Not executed yet.
