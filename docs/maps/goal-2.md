# Goal 2 — Fidelity harness against After Effects: rendered frames, a corpus case

*Machine-drafted by Qwen 2026-10-16, reviewed and corrected by Claude 2026-10-17.*

**Roadmap target:** G1 — "Fidelity harness against After Effects: rendered frames, a corpus case per
P0/P1 feature" (60–100 h, stage 2). See [ROADMAP.md](../../ROADMAP.md#upcoming) and
[docs/gaps.md](../gaps.md#g1-measure-fidelity-against-after-effects).

## What exists today

### Scalar keyframe oracle (not rendered frames)
- `examples/ae-ease-oracle.jsx` — AE ExtendScript that creates a scratch comp, sets ease
  parameters, samples Rotation property values, and prints them to the console.
- `examples/check-ae-ease.mjs` — Node.js harness that drives AEsync CLI, validates AE metadata
  (case names, ease settings, sample times), and compares every value with `1e-7` absolute tolerance.
- `crates/keyframe/examples/ae_ease_samples.rs` — evaluates the same keyframes in EffectCraft and
  prints JSON for comparison.
- `docs/fidelity/README.md` — documents the scalar measurement: 8 Rotation cases, 21 samples each
  (168 total), tolerance `1e-7` degrees. All pass after the influence-normalization fix.

### Rendered-frame helpers
- `crates/effects/src/fidelity.rs` — `FrameScore { compared_pixels, mean_absolute_error, max_error }`
  and `compare_frames(actual, reference) -> Result<FrameScore, String>`. Rejects size mismatches and
  NaN samples. Deterministic and testable without AE.
- `crates/render/src/lib.rs` — `render_frame(project: &Project, comp: ItemId, t: Tick, scale: f64) -> Image`
  is the production entry point, used by the render crate's tests across `tests.rs`, `tests_nested.rs`, `tests_mask_blur.rs`,
  `tests_collapse.rs`, `tests_color.rs`, `tests_styles.rs`, `tests_remap.rs`, `tests_aux.rs`,
  `crates/render/src/shapes/tests.rs`, `crates/render/src/text/tests.rs`, and
  `tests_frame_blend.rs`. Every test already exercises the full render pipeline (compositing,
  effects, masks, alpha, color).

### GPU-vs-CPU oracle (not AE)
- `crates/gpu/src/tests.rs` — the GPU-vs-CPU harness: renders each effect on the GPU and compares it
  with the CPU effects module (the oracle). `tolerance()` is ≤ 1/255 at 8 bpc and 1e-3 at 16/32 bpc;
  `effect_direct` checks at 1e-3. The per-family cases live in the sibling `tests_fx_*.rs` files
  (`tests_fx_color.rs` is the colour family only; others cover particles, LUT, VR, stylize,
  distort, light, gen2 and more).

### External oracle infrastructure
- `crates/export/tests/roundtrip.rs`, `vp9_roundtrip.rs`, `hevc_av1_roundtrip.rs`, `opus_roundtrip.rs`
  — use ffmpeg/ffprobe as external oracles when installed, skip otherwise. Pattern: `ffprobe` for
  stream metadata, `ffmpeg` for frame decode to RGBA, then compare. See `crates/export/tests/roundtrip.rs` for the pattern.

### Workflow checks (not fidelity)
- `docs/fidelity/workflow-checks.md` — UI interaction tests (selection, preview, RAM cache,
  Gaussian Blur adaptive resolution). Establish interaction behavior, not pixel-level AE parity.

## What is missing

1. **No rendered-frame corpus runner.** `compare_frames` exists but has no harness that drives AE to
   produce reference frames, loads them, and scores EffectCraft renders against them. The scalar
   ease harness (`check-ae-ease.mjs`) only samples properties, not rendered pixels.

2. **No AEsync bridge for rendered output.** The current AEscript (`ae-ease-oracle.jsx`) creates a
   scratch comp, sets properties, and samples scalar values. It does not render a frame and capture
   pixels. A new script (or extension) must invoke AE's render engine and export frames to a
   known format (PNG/TGA/EXR) for the harness to read.

3. **No corpus directory or fixture management.** `storytold/photocraft-corpus` holds shared binary
   fixtures for PhotoCraft (PSDs). EffectCraft needs a parallel corpus repo or subdirectory for AE
   reference renders, with sha256 verification and commit-pinned fetch (as `cargo xtask corpus` does
   for PhotoCraft).

4. **No scoring thresholds for rendered frames.** Scalar ease uses `1e-7` degrees. Rendered frames
   need channel-level thresholds: e.g., ≤ 1/255 (8 bpc) or ≤ 1e-3 (32 bpc) mean absolute error per
   pixel, with a configurable max error cap. The GPU-vs-CPU checks already use these values
   (`crates/gpu/src/tests.rs` `tolerance()`).

5. **No automated AE execution path.** The scalar harness requires a human to have AE running and
   answer AEsync. For CI or semi-automated corpus runs, a headless or scripted AE session is needed
   (possibly via AEsync's CLI mode or a dedicated render script).

6. **No per-feature corpus case structure.** The roadmap calls for "a corpus case per P0/P1 feature."
   Each case should define: feature name, AE script, EffectCraft project, reference frames,
   thresholds, and pass/fail criteria.

## First steps (3–6 small, testable)

### Step 1 — Add a rendered-frame corpus runner using existing `compare_frames`
Create `crates/render/tests/fidelity_corpus.rs` (or an example binary; it belongs in the render
crate or above, since `effectcraft-render` depends on `effectcraft-effects`, not the reverse) that:
- Accepts paths to an EffectCraft project (native `.ecproj`/`.ecprojx`, or an imported `.aep`), a reference frame PNG, and
  an output frame path.
- Loads the project, renders the composition at a specified tick, saves the output frame.
- Calls `compare_frames(output, reference)` and prints `FrameScore` as JSON.
- Exits nonzero if `mean_absolute_error > threshold` (configurable, default 1/255 for 8 bpc).
- **Why first:** reuses existing `compare_frames`, requires no AE changes, and produces a working
  binary that can be pointed at manually captured reference frames immediately.

### Step 2 — Write an AE ExtendScript that renders a comp to PNG
Create `examples/ae-render-frame.jsx` (parallel to `ae-ease-oracle.jsx`) that:
- Opens a known composition (passed as argument or created from scratch).
- Renders a single frame to a specified PNG path using AE's render queue or `app.project.renderQueue`.
- Exits with status 0 on success, nonzero on failure.
- Does not save, close, or modify the current project (same clean-room discipline as the scalar script).
- **Why second:** provides the reference frame capture mechanism. Can be tested manually with AEsync
  before integrating into the harness.

### Step 3 — Wire the harness: `examples/check-ae-render.mjs`
Create a Node.js harness (parallel to `check-ae-ease.mjs`) that:
- Invokes `ae-render-frame.jsx` via AEsync CLI to produce reference frames.
- Runs `effectcraft-cli` (or the Step 1 binary) to render the same composition at the same tick.
- Calls `compare_frames` on the pair and prints a per-case score.
- Loops over a list of corpus cases (defined in a JSON config file).
- Exits nonzero if any case exceeds its threshold.
- **Why third:** ties the AE capture and EffectCraft render together into an automated workflow.

### Step 4 — Define the first corpus case (one P0 feature)
Pick one P0 feature from `docs/gaps.md` (e.g., Gaussian Blur at default settings). Define:
- AE project/comp name, layer setup, effect parameters.
- EffectCraft project reproducing the same composition.
- Tick/frame number, resolution, color depth (start with 8 bpc, 64×64 or 128×128 for speed).
- Threshold: `mean_absolute_error ≤ 1/255`, `max_error ≤ 2/255`.
- Reference frame captured via Step 2, stored in a corpus directory (or fetched via `cargo xtask corpus`).
- **Why fourth:** establishes the template for all subsequent corpus cases.

### Step 5 — Add `cargo xtask fidelity` for corpus management
Create an xtask subcommand (parallel to `cargo xtask corpus` in PhotoCraft) that:
- Lists defined corpus cases and their status (pass/fail/skip).
- Fetches reference frames from the corpus repo (sha256-verified, commit-pinned).
- Runs the harness (`check-ae-render.mjs` or a Rust binary) for each case.
- Produces a summary report with per-case scores and aggregate pass rate.
- Skips cases where AE is unavailable (like ffmpeg oracle tests).
- **Why fifth:** operationalizes the harness for CI and developer workflows.

### Step 6 — Document corpus results in `docs/fidelity/rendered-frames.md`
Parallel to `docs/fidelity/README.md` (scalar ease), document:
- Each corpus case: feature, AE version, EffectCraft version, thresholds, scores, pass/fail.
- Methodology: how reference frames were captured, how EffectCraft renders were produced, comparison
  method (premultiplied RGBA, channel-wise MAE/max error).
- Known deviations: effects or features where EffectCraft differs from AE and by how much.
- **Why sixth:** makes progress visible and tracks parity over time.

## Paths summary

| Path | Role | Status |
|------|------|--------|
| `crates/effects/src/fidelity.rs` | `compare_frames`, `FrameScore` | Exists, tested |
| `crates/render/src/lib.rs` | `render_frame()` entry point | Exists |
| `examples/ae-ease-oracle.jsx` | AE property sampling script | Exists |
| `examples/check-ae-ease.mjs` | Node.js scalar harness | Exists |
| `crates/keyframe/examples/ae_ease_samples.rs` | Rust keyframe evaluation | Exists |
| `docs/fidelity/README.md` | Scalar ease documentation | Exists |
| `crates/gpu/src/tests.rs` | GPU-vs-CPU harness and tolerances | Exists |
| `crates/export/tests/roundtrip.rs` | ffmpeg external oracle pattern | Exists |
| `examples/ae-render-frame.jsx` | AE rendered-frame capture script | **New** (step 2) |
| `examples/check-ae-render.mjs` | Node.js rendered-frame harness | **New** (step 3) |
| `crates/render/tests/fidelity_corpus.rs` | Corpus runner test/example | **New** (step 1) |
| `xtask/src/fidelity.rs` | `cargo xtask fidelity` subcommand | **New** (step 5) |
| `docs/fidelity/rendered-frames.md` | Rendered-frame corpus documentation | **New** (step 6) |
| `corpus/aeframes/` | AE reference render fixtures | **New** (step 4) |

## Clean-room notes

- All AE scripts (`ae-render-frame.jsx`, harness) capture behavior only: they create scratch comps,
  set parameters, render frames, and read output. No Adobe implementation code, presets, or internal
  formats are read or copied.
- Reference frames are rendered by AE and stored as open formats (PNG/TGA/EXR). No proprietary AE
  project files or binary exports are committed.
- The corpus repo (if external) follows `storytold/photocraft-corpus` conventions: commit-pinned
  fetch, sha256 verification, `ATTRIBUTION.md` for each fixture.
- Thresholds are behavioral (pixel error bounds), not implementation-specific. They measure *that*
  EffectCraft matches AE output, not *how* AE produces it.
