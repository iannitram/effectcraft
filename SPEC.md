# EffectCraft (our fork): spec

**Approval:** 2026-10-26, the owner: "The specs/roadmaps were developed by someone else. Nomalize them to our system" (spec = upstream roadmap + our delta, R18).

## Base: upstream's roadmap

EffectCraft is a fork of [storytold/effectcraft](https://github.com/storytold/effectcraft). Upstream's plan is the spec for everything we do not list below:

- [ROADMAP.md](https://github.com/storytold/effectcraft/blob/main/ROADMAP.md): parity with Adobe After Effects, stage and headline numbers
- [docs/roadmap.md](https://github.com/storytold/effectcraft/blob/main/docs/roadmap.md): the plan
- [docs/gaps.md](https://github.com/storytold/effectcraft/blob/main/docs/gaps.md): the ranked work list
- [docs/target-app-parity.md](https://github.com/storytold/effectcraft/blob/main/docs/target-app-parity.md): how parity is measured

Upstream's work is upstream's: we do not rebuild what upstream has open (check `gh pr list -R storytold/effectcraft` first). The daily fork sync brings it in.

## 1.0 for our fork

The owner's After Effects title work (two intros and seven title projects) opens in EffectCraft, looks as it did in After Effects and renders, so a real episode's titles can be made with our builds instead of Adobe; a practice episode first (owner, 2026-10-12 and 2026-10-15; issue #33). The numbers to hold or beat are the every-frame comparisons in #33. *To confirm with the owner:* the 95% feature list that gates it (#28) and the order toward real episode work (#20).

## Our delta

| # | Delta | Status | Where | Upstream? |
|---|---|---|---|---|
| D1 | **Our own `.aep` reader** (`crates/aep-native`), no external converter, kept joinable with upstream PR 860 | built; gaps tracked | #13, #14, #43 | the owner's call (a new design); say how the format knowledge was obtained |
| D2 | **Fidelity to the owner's projects:** every frame of both intros and the titles against their reference exports; Glow, mask feather, fade-out floor, fonts | measured; open items | #9-#12, #29, #30, #32 | fixes: send what's solid |
| D3 | **Exports that finish:** frames in flight follow free memory; the full-size GPU export and Time Displacement comps stop running out of memory | memory batching on main; out-of-memory open | #7, #8 | behaviour change: the owner's yes |
| D4 | **Live link with FilmCraft:** a title in a FilmCraft timeline re-renders on save; `--comp` opens one composition | `--comp` built; rest open | #19 | upstream-ready, offered only on the owner's word |
| D5 | **Self-logging** (`craft-log`, `craft-stage`, `sysmem`): log files, secret masking, diagnostics | built; size cap open | #22, #23 | keep on our fork |
| D6 | **Linux desktop:** Wayland window fit; Wayland file drops; footage follows a moved project folder | fit and folder-follow built; drops open | #24, #27 | send upstream |
| D7 | **GPU:** adjustment-layer blending in the shader | open | #16 | send upstream |
| D8 | **Releases through the Craft launcher:** public, labelled unofficial builds for Windows, macOS and Linux by GitHub Actions; the launcher owns updates | not built | #45 (blocked by the test gate, #44) | keep on our fork |
| D9 | **FilmCraft pin and H.265:** move the pin to take the MP4 fixes; help FilmCraft upstream's H.265 hardware-export PRs | open | #15, #18 | help upstream's PRs |

## Won'ts (owner rulings)

- No fix is written for After Effects projects outside the two intros and seven titles; the rest stay test material only (2026-10-13).
- Licensed fonts are never installed or copied; free look-alikes only after the owner approves them by eye (2026-10-13).
- No paid code signing for the apps (2026-10-15).
