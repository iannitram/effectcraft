# GPU effects map — `gpu-8`

*Machine-drafted by Qwen 2026-10-16, reviewed and corrected by Claude 2026-10-17.*

## What it is / callers

Twelve GPU effect test modules under `crates/gpu/src/` that compare the GPU-accelerated
render path (`effectcraft_render::Accelerator::effects`) against the CPU effect implementations
(the oracle, `effectcraft_effects`'s `render` closures). Each module exercises a family of
effects: direct on a buffer at full and half resolution (with and without adjustment-mode),
and composited at 8 and 32 bpc through a `Scene` that builds a project with footage layers,
backgrounds, masks, transforms, and auxiliary channels.

The harness lives in `tests.rs` (shared functions: `effect_direct`, `effect_case`, `Scene`,
`check`, `compare_at`, `diff`, `opts`, `n`, `c`, `set`) and `tests_fx_sim.rs` (simulation
harness: `direct`, `composited`, `case`; not one of the twelve files here). Of the twelve,
`tests_fx_pixel2` exports `pub(crate)` helpers (`e`, `on`, `off`, `pt`, `mask_points`, `Host`,
`direct_env`, `env_case`, `env_case_allow`, `env_case_at`) that `tests_fx_gen2` imports, and
`tests_fx_depth` exports `AuxHost`, `aux_scene` and `direct`. `tests_fx_gen2` exports nothing.
No file exports `pub` items beyond `pub(crate)`.

Callers: the Rust test runner (`cargo test --lib -p effectcraft-gpu`). Each `#[test]` fn
invokes one or more harness helpers with an effect ID (e.g. `"ec.color.vibrance"`) and a
slice of parameter overrides.

## Files

| File | Lines | Purpose / effect family |
|------|------:|-------------------------|
| `tests_fx_color.rs` | 204 | Colour: color balance, vibrance, tritone, channel mixer, black/white, colorama, selective color, lumetri, linear color key, key light, custom cycles |
| `tests_fx_depth.rs` | 263 | 3D Channel: channel extract, depth matte, fog, dof, IDs, cryptomatte, extractor; custom `AuxHost`, `AuxSource`, `aux_scene` for auxiliary channel injection |
| `tests_fx_distort.rs` | 160 | Distort/stylize: twirl, bulge, ripple, wavewarp, turbulent displace, CC Lens, edge pinning on shapes, mesh warp, mosaic, find edges, emboss, displacement map (self + layer) |
| `tests_fx_extra.rs` | 51 | Shape/bevel: circle, ellipse, iris wipe, bevel alpha, bevel edges, Gaussian blur legacy |
| `tests_fx_gen2.rs` | 176 | Generators part C: lightning (obsolete + advanced), beam, lens flare, radio waves, Vegas, strokes, scribble, write-on, paint bucket, eyedropper, CC glue gun, CC threads, audio spectrum, audio waveform, basic text, path text; imports helpers from `tests_fx_pixel2` |
| `tests_fx_generate.rs` | 200 | Blurs/wipes/generators: radial blur, CC radial fast blur, camera lens blur, various wipes, gradient wipe, generators (fire, stars, etc.), noise & grain |
| `tests_fx_key.rs` | 251 | Keying/matte/channel: colour luma difference/screen keys, luma key, range key, difference key, screen key, spill suppress, unmult, key cleaner, chokers, refine mattes, channel effects, arithmetic; defines `discontinuous_case` for threshold-sensitive effects |
| `tests_fx_light.rs` | 56 | CC lights: light rays, light burst, light sweep, light wipe |
| `tests_fx_lut.rs` | 345 | Colour management: apply color LUT, OCIO file transform, OCIO CDL/space/display/look, color profile converter, Lumetri LUTs/looks, OCIO custom config (v1 + v2); defines `case_allow` for NN lattice rounding exceptions, inline `lattice`, `cube_3d`, `cube_1d`, `csp` generators |
| `tests_fx_noise.rs` | 303 | Noise/blur/time: median, median legacy, dust & scratches, edge-preserving blurs, sharpen, unsharp mask, channel/compound blur, minimax, turbulent noise, fractal noise, noise alpha & HLS, remove grain, echo, posterize time, median levels, dust threshold; uses `Scene`, `check`, `compare_at` for compositing; some tests build full `Comp`/`LayerSource` pipelines |
| `tests_fx_particles.rs` | 166 | Particle simulation passes: CC Particle World types/modes/camera, CC Particle Systems II types/modes, CC Particle Playground, Ball Action / Pixel Polly / Scatterize, curl noise; imports `case`, `composited` from `tests_fx_sim` |
| `tests_fx_pixel2.rs` | 251 | Pixel ports part C: CC cross/radial/vector blur, reduce interlace flicker, CC composite, color link, Cineon converter, HDR compander/compression, grow bounds, CC overbrights, CC block load, CC burn film, CC glass, CC hex tile, CC Mr. Smoothie, CC plastic, inner/outer key, CC simple wire removal, Basic 3D; defines `Host` (mask + layer + audio env), `direct_env`, `env_case` family; exports helpers used by `tests_fx_gen2` |

## How it fits together

```
tests.rs (shared harness)
├─ effect_direct(id, vals, adjustment)   GPU kernels vs CPU effect on a bare buffer,
│                                         scales 1.0 and 0.5; returns at once without a GPU
├─ effect_case(id, vals)                 effect_direct(false) + effect_direct(true)
│                                         + composited Scene at Bpc8 / Bpc32, full and half
├─ Scene::new / footage / solid / push / effect, set(l, path, value)
└─ compare_at(scene, opts, t) → check(label, diff, allow)

tests_fx_pixel2.rs                       tests_fx_depth.rs
├─ Host: EffectHost (masks, layer, audio) ├─ AuxHost: EffectHost (aux channels)
├─ direct_env(id, vals, allow)            ├─ AuxSource: FootageSource
├─ env_case / env_case_allow / env_case_at├─ aux_scene(w, h, scale)
│     └─ imported by tests_fx_gen2        ├─ direct(id, vals, allow)
└─ own tests use these + effect_case      └─ case / composited (private)

tests_fx_key.rs: discontinuous_case      tests_fx_lut.rs: case_allow
tests_fx_particles.rs ── imports case, composited from tests_fx_sim.rs
```

**Data flow per test:**
1. `pattern(seed, w, h)` generates a deterministic test image.
2. The harness creates `Params` with default values, then overrides with the test's `vals`.
3. CPU side: the effect's `render` closure runs on the pattern image via `EffectCtx`.
4. GPU side: `Accelerator::effects` runs the same effect spec on the same image through the
   GPU backend.
5. `diff(cpu, gpu, tol)` computes per-pixel absolute differences; the test passes if pixels
   exceeding `tol` stay below the allowed fraction.
6. For composited tests: a `Scene` builds a multi-layer project (background + footage + effect
   layer + optional masks/transforms), then `compare_at` renders the full comp on both CPU and
   GPU and diffs the results.

**Control flow:**
- `effect_direct(id, vals, adjustment)` → one adjustment mode (the caller picks it), at scale 1.0
  and 0.5, tolerance 1e-3, no pixels allowed over it (a small fraction on GL).
- `effect_case` → calls `effect_direct` twice (both adjustment modes).
- `env_case` / `env_case_allow` / `env_case_at` → `direct_env` (masks + layer + audio env)
  then composited at both bit depths.
- `discontinuous_case` (tests_fx_key) → `effect_direct` twice + composited at Bpc8 and Bpc32
  with a 0.1 % tolerance for threshold-edge pixels.
- `case_allow` (tests_fx_lut) → `effect_direct` once (adjustment=false) + composited at both depths with a
  custom `allow` for nearest-neighbour lattice rounding exceptions.
- `tests_fx_sim` harness: `direct` at both scales → `composited` at both depths.

## Watch out for

- **Without a GPU these tests pass vacuously.** `effect_direct`, `direct_env` and the other
  harnesses start with `let Some(g) = gpu() else { return };`, and `check` returns on a `None`
  diff, so on a machine (or CI runner) with no adapter every test here passes without comparing
  anything. Look for the `no GPU (…), skipping GPU comparisons` line in the output, or set
  `EFFECTCRAFT_REQUIRE_GPU=1` to make a missing GPU fail the run.

- **No `pub` items** — all helpers are `pub(crate)`. `tests_fx_gen2` imports `e`, `on`, `off`,
  `pt`, `env_case` family from `tests_fx_pixel2` via `use crate::tests_fx_pixel2::{…}`.
- **Auxiliary channels** (`tests_fx_depth`) require custom `EffectHost` (`AuxHost`) and
  `FootageSource` (`AuxSource`) impls; `aux_scene` builds depth ramps, Cryptomatte layers,
  UVs, normals, material IDs, and coverage at a configurable scale.
- **Mask + layer + audio environment** (`tests_fx_pixel2`) uses a `Host` struct implementing
  `EffectHost` to supply pentagon/quad/open-curve masks, a layer reference, and synthetic
  audio — tested via `direct_env` / `env_case` family.
- **Threshold-sensitive effects** (`tests_fx_key::discontinuous_case`): 8-bit quantisation
  and bitwise ops may flip a few pixels on the boundary after float rounding in layer
  transforms; tolerance is 0.1 % of pixels.
- **Nearest-neighbour lattice rounding** (`tests_fx_lut::case_allow`): GPU division may pick
  the adjacent lattice point when an input lands exactly on a rounding boundary; `allow`
  parameter documents the exception.
- **Simulation time** (`tests_fx_particles`): particle tests use `tests_fx_sim::case` and
  `composited` which accept a time parameter `t`; tests pick frames where simulations have
  visible output.
- **Effect IDs**: effects are looked up by string ID (e.g. `"ec.color.vibrance"`) via
  `effectcraft_effects::find(id)`. If an effect is not registered, `unwrap()` panics — but
  all IDs in these tests reference effects that exist in the effect host.
- **Resolution scaling**: the direct harnesses run at scale 1.0 and 0.5; composited cases run full
  and half. `direct_env` (tests_fx_pixel2) also pads its half-resolution buffer (`buf.pad(3)`).
- **Bit depth**: composited tests run at both `Bpc8` and `Bpc32`; some effects behave
  differently across depths (e.g. quantisation in keying effects).
