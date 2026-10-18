# Goal 1: Stability and CI Gate (G3)

*Machine-drafted by Qwen 2026-10-16, reviewed and corrected by Claude 2026-10-17.*

> **Target:** Fix all open crash reports and run `cargo xtask ci` on every pull request.
> **Source:** [ROADMAP.md](../../ROADMAP.md), [docs/gaps.md](../gaps.md) gaps 3–4.
> **Date created:** 2026-10-16
> **Status:** in progress

## Context

EffectCraft's "Never crash" rule ([AGENTS.md](../../AGENTS.md)) outranks all features. Nearly every crate (33 of 36)
already uses `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, ...)]`, but six
crash reports remain open and no CI workflow runs `cargo xtask ci` on pull requests.

### Current state

- `cargo xtask ci` runs locally: `cargo fmt --check`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `cargo test --workspace --release`, `pdf_deps::run`, `layers()`, `assets()`, `wasm()` (`xtask/src/main.rs` `fn ci`).
- `.github/workflows/` exists with `release.yml`, `packaging-lint.yml`, `freebsd.yml`, `windows-arm64.yml` and `nix.yml`, but no PR gate (gaps.md row 4 predates `nix.yml`).
- Project files are fuzzed (`fuzz_file_open_never_crashes` in `effectcraft-engine`); media, expressions, scripts, and control-channel/MCP inputs are not yet fuzzed.
- One crash was found and fixed: deeply nested XML overflowing the stack (now refused).

## Open crash reports (gap 3)

| Issue | Description | Status | Priority |
|---:|---|---|---|
| #580 | Chromatic Aberrations pan/tilt crash | open | P0 |
| #501 | Crash on startup | open | P0 |
| #426 | Timeline drag while playing crash | open | P0 |
| #341 | Quit on Intel Mac crash | open | P1 |
| #234 | macOS 27 exit crash | open | P1 |
| #879 | `--gpu` export aborts on failed allocation | open | P0 |

**Closed crashes (for reference):** #591, #306, #511 — each fixed with a regression test.

### Fix strategy for each crash

1. **Reproduce:** Create a minimal synthetic test case (or use the reporter's project if provided).
2. **Locate:** Trace the panic path; add `Result<T, E>` propagation where the crash occurs.
3. **Fix:** Replace the panic path with graceful error handling or a safe fallback.
4. **Verify:** Add a regression test that panics before the fix and passes after.
5. **Close:** Reference the fix PR in the issue.

## CI gate on pull requests (gap 4)

### Current gap

No workflow runs `cargo xtask ci` on PRs. Regressions reach `main` unseen. 49 community PRs
landed in one batch (#598) without automated checks.

### Required workflow

Create a CI workflow file in `.github/workflows/` (e.g. `ci-pr.yml`) that:

1. **Triggers:** on `pull_request_target` (for safe secret access if needed) or `pull_request`.
2. **Runs on:** `ubuntu-latest`, `macos-latest`, `windows-latest` (matrix).
3. **Steps:**
   - Set up the Rust stable toolchain explicitly (the repo has no `rust-toolchain.toml`), e.g. `dtolnay/rust-toolchain@stable` with `targets: wasm32-unknown-unknown`.
   - Install system dependencies (X11/Wayland libs on Linux, CUDA toolkit stubs if needed).
   - Run `cargo xtask ci` (which runs fmt, clippy, tests, pdf-deps, layers, assets, wasm).
   - Upload test artifacts on failure (screenshots, logs).
4. **Cache:** Rust dependency cache via `actions/cache` or `Swatinem/rust-cache`.
5. **Timeout:** 30 minutes per job; fail fast on matrix.

### Workflow skeleton

```yaml
name: CI (PR gate)

on:
  pull_request:
    branches: [main]
  merge_group:

jobs:
  ci:
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      - name: Add the wasm target
        run: rustup target add wasm32-unknown-unknown
      - name: Install system dependencies (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
            libxkbcommon-dev libssl-dev libwayland-dev libx11-dev libxi-dev libgl1-mesa-dev
      - name: Install system dependencies (macOS)
        if: runner.os == 'macOS'
        run: brew install openssl@3
      - name: Install system dependencies (Windows)
        if: runner.os == 'Windows'
        run: choco install openssl
      - name: Run cargo xtask ci
        run: cargo xtask ci
```

### Additional CI considerations

- **WASM target:** `cargo xtask ci` runs `wasm()` which builds for `wasm32-unknown-unknown`. The target is not installed by default on the runners: add it with `rustup target add wasm32-unknown-unknown` (or the toolchain action's `targets:`) before `cargo xtask ci`.
- **Layer check:** `layers()` validates dependency layering; no special setup needed.
- **Assets check:** `assets()` verifies `.attribution` sidecars and `ATTRIBUTION.md` index; no special setup needed.
- **Test coverage:** `cargo test --workspace --release` is the full test suite (~2,704 tests as of 10 October).

## Fuzzing expansion (future, beyond this goal)

This goal focuses on fixing existing crashes and adding the CI gate. Fuzzing expansion is
out of scope but tracked here for continuity:

- [x] Project files: `.ecproj` JSON and `.ecprojx` XML fuzzing in `effectcraft-engine`.
- [ ] Imported media: image, video, audio formats.
- [ ] Expressions: malformed expression strings.
- [ ] Scripts: ExtendScript/JavaScript with invalid input.
- [ ] Control-channel/MCP: malformed requests.

## Acceptance criteria

- [ ] All 6 open crash reports (#580, #501, #426, #341, #234, #879) are closed with regression tests.
- [ ] A CI workflow in `.github/workflows/` runs `cargo xtask ci` on every PR.
- [ ] CI is green on `main` before merging the workflow file.
- [ ] Every crash fix has a regression test in the same PR.

## Effort estimate

- Crash fixes: 10–25 hours total (per gaps.md gap 3).
- CI workflow: 4–8 hours total (per gaps.md gap 4).
- **Total: 14–33 hours** (agent wall-clock hours, calibrated in [target-app-parity.md](../target-app-parity.md#calibration-of-the-hours)).

## Related workstreams

- **G1:** Measure fidelity against After Effects (crash fixes may reveal fidelity issues).
- **G2:** Real-user reliability on every platform (CI gate helps catch platform-specific regressions).
- **G5:** Performance at real-world scale (CI gate catches performance regressions via benchmarks).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-16 | initial | Created goal map; listed 6 open crash reports and CI gap |
