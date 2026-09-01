# One ISO caliper golden — M8×40

Inspector and Kernel lock **the same** fixture: [`m8_x40.json`](m8_x40.json).

Do not drift this file from `crates/kernel/tests/occt_geometry.rs`
(`iso_m8_x40_golden_document` via `include_str!`). The old AF **10** /
head **5.5** recipe is not the golden.

| Caliper | Value | Notes |
|---|---|---|
| Across flats (AF) | **13 mm** | ISO 4014 / 4017 wrench size (`head_width`) |
| Shank / major Ø | **8 mm** | M8 |
| Pitch P | **1.25 mm** | ISO 261 coarse (`size: "M8"`) |
| Overall length L | **40 mm** | `bolt_length` |
| Head height | **~5.3 mm** | `head_height` (ISO hex cap ≈ 5.3) |

Feature recipe (mm, +Z shank):

1. Sketch hex `across_flats` 13 on XY
2. Extrude depth 5.3
3. Cylinder Ø8 × 35.7 at `[0, 0, 4.3]` (overlaps the head)
4. External thread M8 × 34.7 at `[0, 0, 5.3]`

Look-right (viewport + STL) must see a helix / ISO-V groove, not stacked
ticks or a smooth Ø8 rod that merely shares this AABB.
