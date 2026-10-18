# Goal 6 — Localization Depth (G9)

*Machine-drafted by Qwen 2026-10-16, reviewed and corrected by Claude 2026-10-17.*

> Maps existing i18n infrastructure, concrete gaps, and small first steps for effect names,
> panels, and Korean/Italian support.

## 1. Goal summary

| Field | Value |
|---|---|
| **Goal** | G9 — Localization depth: translate effect names, parameter names, and remaining panel strings; add Korean (`ko`) and Italian (`it`) to the 10-language set After Effects ships. |
| **Parity doc** | [localization-parity.md](../localization-parity.md) |
| **Gap row** | Gap 16 in [gaps.md](../gaps.md) |
| **Current coverage** | ≈ 18 % of ≈ 3,900 user-visible strings (menus 10 L, panels 7 L, effect names 1 L) |
| **Target** | 100 % of menus + panels + effect names + parameter names in all 10 non-English AE languages |
| **Estimate** | 60–110 h infrastructure + translation; ≈ 5–9 h per additional language |

## 2. Existing infrastructure (what to build on)

### 2.1 Menu catalogs — `crates/ui-egui/src/i18n.rs`

- One `const` table per language, keyed by `(command, English label)`.
- **624 rows** per language at `upstream/main` 470ea7a3.
- Languages with full menu catalogs (10, English excluded: it is the source): `ja`, `zh-hans`, `zh-hant`, `uk`, `es`, `pt-br`, `ru`, `fr`, `de`, `cs`.
- Test `every_translation_key_exists_in_the_actual_menu_tree` fails if any fixed menu entry is untranslated or a row is stale.
- `CATALOG_CODES` lists those 10 codes; `en` is the fallback and has no entry.
- Language selection: Settings ▸ General ▸ Language or Match System.

### 2.2 Panel / dialog catalogs — `crates/ui-egui/src/i18n/ui.rs`

- `tr()` and `tr_args()` functions keyed by the English source string.
- English fallback: unknown strings pass through verbatim.
- `CONVERTED`, a const in the `tests` module, lists every panel source file that has been migrated (56 entries).
- Catalogs exist for 7 languages (rows counted at `origin/ours`, 2026-10-17):

| Language | Rows | Notes |
|---|---|---|
| `zh-hans` | 689 | Most complete |
| `zh-hant` | 689 | Mirrors Simplified Chinese |
| `ja` | 519 | |
| `uk` | 518 | |
| `es` | 508 | |
| `cs` | 512 | |
| `pt-br` | 411 | |

- Test `every_row_is_used_by_a_converted_panel` fails if a catalog row is orphaned.
- Test `helper_labels_are_translated` catches `Checkbox::new`/`RadioButton::new` literals not routed through `tr()`.

### 2.3 Effect name catalog — `crates/ui-egui/src/i18n/effects.rs`

- Keyed by **effect id** (not English name). Effect ids are stable across languages.
- Simplified Chinese: **306 rows** (one per effect in the registry) + 21 category rows.
- Drawing sites (`effects_presets.rs`, `effect_controls.rs`, `timeline.rs`) call `effects::name()`, `effects::group()`, `effects::category()`.
- A repeated effect (`Gaussian Blur 2`) keeps the engine-appended index; a user rename stays verbatim.
- Search matches either the English registry name or the translated name.
- Two guard lists for languages being filled in:
  - `PENDING`: tables in place but empty (`zh-hant` only). Test ensures rows only use known effect ids.
  - `PARTIAL`: tables with some rows (`ja`).
- A new language needs two `const` tables and one `catalog()` arm — no panel changes.

### 2.4 Font fallback — `crates/ui-egui/src/theme.rs`

- System fallbacks probed by script: Kana, Han, Arabic, Hebrew, Devanagari, Bengali, Gurmukhi, Gujarati, Tamil, Telugu, Kannada, Malayalam, Thai, Hangul.
- Hangul probe added in #599 (closed).

## 3. Concrete gaps

### 3.1 Effect parameter names — ~1,650 strings

- **Current state:** No language has parameter name translations. All draw the English registry name.
- **Location:** Parameter names live in `crates/effects`; they are not routed through a catalog.
- **Impact:** Effect Controls panel shows English parameter names even when the rest of the UI is translated.
- **Work needed:** Route parameter names through a catalog (new module or extension of `effects.rs`), add `tr()` calls in the Effect Controls panel.

### 3.2 Panel strings — 56 converted files, but 3 menu languages have no panels

- **Missing panel catalogs:** `ru`, `fr`, `de` have menus but no panel translations. (`es` has panels; `it` and `ko` have no menus either, see 3.3 and 3.4.)
- **Unconverted panels:** Some panel files still contain raw English literals (not in `CONVERTED`).
- **Helper labels:** `Checkbox::new`/`RadioButton::new` labels must be routed through `tr()` (guarded by test).

### 3.3 Korean (`ko`) — 0 % coverage

- Menus: no catalog.
- Panels: no catalog.
- Effect names: no catalog.
- Font: Hangul probe exists (#599 closed).
- After Effects ships Korean; EffectCraft does not.

### 3.4 Italian (`it`) — 0 % coverage

- Menus: no catalog.
- Panels: no catalog.
- Effect names: no catalog.
- After Effects ships Italian; EffectCraft does not.

### 3.5 Native-speaker review

- No language has had native-speaker review of any catalog.
- All translations are AI-assisted or contributor-translated without formal review.

### 3.6 Engine messages and Home screen

- Engine-generated text (property names, errors, undo labels) is not cataloged.
- Home screen has unwrapped literals (#574 closed for some, but not all).

## 4. First steps (small, reviewable, testable)

### Step 1 — Add Italian and Korean menu catalogs

- **File:** `crates/ui-egui/src/i18n.rs`
- **Task:** Add `const ITALIAN` and `const KOREAN` tables with 624 rows each, mirroring the existing catalog structure. Translate from the plain meaning of the English label; never copy Adobe's strings.
- **Test:** `every_translation_key_exists_in_the_actual_menu_tree` passes.
- **Estimate:** 2–3 h per language.

### Step 2 — Add Italian and Korean panel catalogs

- **File:** `crates/ui-egui/src/i18n/ui.rs`
- **Task:** Add `const ITALIAN_PANEL` and `const KOREAN_PANEL` tables covering every row in `catalogs()`. Start with the 689 rows of the Simplified Chinese panel catalog as the source-of-truth for English strings.
- **Test:** `every_row_is_used_by_a_converted_panel` and `catalogs_are_unique_and_keep_the_english_shape` pass.
- **Estimate:** 3–4 h per language.

### Step 3 — Add Italian and Korean effect name catalogs

- **File:** `crates/ui-egui/src/i18n/effects.rs`
- **Task:** Add two `const` tables per language (effect names + categories) and one `catalog()` arm. Translate from the plain meaning of the English label; never copy Adobe's strings.
- **Test:** `a_pending_language_only_uses_known_ids` passes; `languages_without_rows_draw_the_registry_name` updates to show Italian/Korean drawing translations.
- **Estimate:** 2–3 h per language.

### Step 4 — Route parameter names through a catalog

- **File:** New module in `crates/ui-egui/src/i18n/` (or extend `effects.rs`).
- **Task:** Define a catalog keyed by `(effect_id, parameter_name)`. Add `tr()` calls in the Effect Controls panel where parameter labels are drawn. Start with Simplified Chinese (306 effects × ~5 params each ≈ 1,500 rows).
- **Test:** Effect Controls shows translated parameter names in `zh-hans`; English and unknown languages fall back to registry names.
- **Estimate:** 4–6 h infrastructure + 3–4 h translation.

### Step 5 — Unwrap remaining panel literals

- **File:** `crates/ui-egui/src/panels/` (the 56 converted files plus any unconverted ones).
- **Task:** Find raw English string literals in converted panels that should use `tr()`/`tr_args()`. Add them to the catalog and wire them up.
- **Test:** `every_row_is_used_by_a_converted_panel` passes; no regression in `helper_labels_are_translated`.
- **Estimate:** 2–3 h.

### Step 6 — Native-speaker review pipeline

- **Task:** Define a review checklist for each language catalog. Draft review steps in a new doc. Recruit native speakers for Korean and Italian (the two highest-priority missing languages).
- **Estimate:** 1–2 h setup + ongoing.

## 5. Risk and dependency map

| Risk | Mitigation |
|---|---|
| Large translation effort for ~1,650 parameter names | Phase 1: effect names only (306 rows per language). Phase 2: parameter names in top-20 effects. |
| No native reviewers for ko/it | Translate from the plain meaning (never Adobe's strings) + community translation; flag entries as "unreviewed" in docs. |
| Panel literal scanning misses strings | Run `helper_labels_are_translated` test after every panel edit; add lint for raw strings in converted panels. |
| RTL languages (Arabic, Hebrew) later | Keep catalog structure language-agnostic; RTL layout is a separate G9 sub-task. |

## 6. Verification

- `cargo test -p effectcraft-ui-egui i18n` — all existing i18n tests pass.
- `cargo test -p effectcraft-ui-egui languages_without_rows_draw_the_registry_name` — confirms ko/it draw translations after Steps 1–3.
- Manual: switch UI to `ko` and `it`; verify menus, panels, and effect names are translated.
- `cargo xtask ci` — full CI gate.

## 7. Progress tracking

| Step | Status | PR | Notes |
|---|---|---|---|
| 1. Italian/Korean menus | pending | — | |
| 2. Italian/Korean panels | pending | — | |
| 3. Italian/Korean effect names | pending | — | |
| 4. Parameter name catalog | pending | — | Infrastructure first |
| 5. Unwrap panel literals | pending | — | |
| 6. Review pipeline | pending | — | |

## 8. References

- [localization-parity.md](../localization-parity.md): detailed parity numbers, language-by-language breakdown.
- [gaps.md](../gaps.md) gap 16: tracks G9 localization depth.
- `crates/ui-egui/src/i18n.rs`: menu catalogs, language codes, `CATALOG_CODES`.
- `crates/ui-egui/src/i18n/ui.rs`: panel/dialog catalogs, `tr()`/`tr_args()`, `CONVERTED` list.
- `crates/ui-egui/src/i18n/effects.rs`: effect name catalogs, `PENDING`/`PARTIAL` guard lists.
- `crates/ui-egui/src/theme.rs`: font fallback probes.
- After Effects 2026 (26.5.0.89): 11 `.lproj` bundles — en, fr, de, it, ja, ko, pt-br, ru, es, zh-hans, zh-hant.
