# Rules for agents in this repo (our pipeline)

Upstream's `AGENTS.md` and `CLAUDE.md` are upstream's: read them and follow them for code. These are our pipeline rules on top. Full pipeline: the owner's private pipeline repo (`PIPELINE.md`, `RULES.md`, `RUBRIC.md`); this is the short copy.

1. **Find the stage** in `PIPELINE.md`. Spec: `SPEC.md` (upstream's roadmap + our delta). Code map: `docs/code-map.md`.
2. **Work from an issue.** Take one that is not `in-progress` and not blocked; add `in-progress` before you start. Its body is your brief; an Easy issue has a "Qwen brief" section.
3. **Your own platform's issues first.** Mac, Windows and Linux all matter: say which you built on.
4. **Check that it builds**, then open the PR against `main`. CI and a review by another satellite with another model do the rest.
5. **Label the PR** `by:<machine>` and `model:<model>`, and write `Closes #N`.
6. **A failed job** gets a comment on the issue saying what happened, and the issue moves up one size.
7. **No private details** in code, commits, issues or PR text: no names, e-mail addresses, home paths, machine names, project files or footage. Issues and reports carry counts only. Commit as the no-reply identity; never add the owner to contributors, AUTHORS or a changelog credit.

## This project

- **Licence wall:** MIT/Apache. GPL code is reference only and never enters the tree. Our `.aep` reader is written from public descriptions and measurements only: no Adobe code, no other reader's source.
- **Upstream:** do not rebuild what upstream has open. Solid small fixes (a test that fails without them, suites green on upstream `main`, nothing private, no open upstream PR for the same thing) may be offered upstream and reported afterwards; behaviour changes, features and designs need the owner's yes. Never comment on, review or close anything upstream.
- **The owner's After Effects projects and the media they link to are read-only:** tests run on copies.
- **Measure every frame, never samples.** Allow one frame for Adobe H.264 exports until the FilmCraft pin carries the MP4 fix (#15).
- **Builds and tests:** debug test builds are huge; cap parallel jobs (`-j 4`) and test crates alone (`cargo test -p effectcraft-host -j 4`). GPU tests: `cargo test -p effectcraft-gpu --lib`. Known failing tests and the full test notes are in issue #33.
- **Switches:** `EFFECTCRAFT_FONT_DIRS`, `EFFECTCRAFT_CONFIG_DIR` (own settings folder), `RAYON_NUM_THREADS`, `RUST_LOG`.
