# Prefer `.zip` over `.tar.gz` in asset selection

## Goal
When a release publishes both a `.zip` and a same-target tar-family/`.exe` artifact,
`grd` auto-selects the `.zip` instead of returning `Selection::Multiple`, without
overriding OS/arch/musl preferences.

## Background
- `calculate_match_score` (`src/asset.rs:219`) ranks OS token (+2 exact / +1 alias),
  arch token (+1), and a Linux-only musl bonus (+1, line 279). Extension is unscored.
- Same-target `.zip` vs `.tar.gz` therefore ties → `best_unique_match` (line 294)
  returns `None` → `Selection::Multiple`
  (`test_select_asset_multiple_matches_without_force_select`, line 1017).
- **Resolved decisions:** global (all OSes); +1 tie-break strength; zip preferred over
  `.tar.gz`, `.tgz`, `.tar.xz`, and `.exe`; `.tgz` grouped with `.tar.gz`;
  **musl `.tar.gz` must still beat a glibc `.zip`**.
- **Why not a flat `+1` in `calculate_match_score`:** glibc zip (3+1=4) would tie
  musl tar.gz (3+1=4), violating the last decision. Bumping musl to +2 is rejected:
  it would let an arch-less `tool-linux-musl.tar.gz` outrank a precise
  `tool-linux-x86_64.tar.gz`. So the preference is applied as a **secondary key
  below the primary score**.

### Behavior matrix (primary score → winner)

| Tied candidates | Primary | Winner |
|---|---|---|
| glibc `.zip` vs glibc `.tar.gz`/`.tgz`/`.tar.xz`/`.exe` | all 3 | `.zip` |
| glibc `.zip` (3) vs musl `.tar.gz` (4) | 3 vs 4 | musl `.tar.gz` |
| musl `.zip` (4) vs musl `.tar.gz` (4) | all 4 | `.zip` |
| `.tar.gz` vs `.tar.xz` (no zip) | all 3 | `Multiple` (unchanged) |

## Approach
1. **Add `is_zip(name) -> bool`** in `src/asset.rs` next to `has_musl_token`
   (case-insensitive `.zip` suffix; reuse `extract_extension` if convenient).
2. **Primary score unchanged** — `calculate_match_score` and every OS/arch/musl number
   stay exactly as they are, so all existing musl tests remain valid.
3. **Secondary key in `best_unique_match`** (line 294): when the top-score count is
   `> 1`, return the unique `is_zip` candidate in that tied set; if the tied set has no
   zip, or more than one zip, stay `None` → `Multiple`. This produces every row of the
   behavior matrix above.
4. **Secondary key in `sort_by_score`** (line 286): sort `is_zip` first among equal
   primary scores, keeping `--select` / `Multiple` listings consistent with
   auto-selection (a boolean is a valid total order, so no transitivity risk).
5. **Opt-out needs no new flag**: `--exclude zip` forces tar/`.exe` selection via the
   existing substring blacklist (`src/asset.rs:419`). Document it instead of adding a
   switch.
6. **Docs**: add a "Format Preference" section to `README.md` after "libc Preference"
   (line 223) and a matching glossary entry in `CONTEXT.md`.
7. **Tests** in `src/asset.rs` `mod tests` (per AGENTS.md):
   - update `test_select_asset_multiple_matches_without_force_select` to expect
     `Exact(zip)`;
   - zip beats `.tgz`, `.tar.xz`, `.exe`;
   - musl `.tar.gz` beats glibc `.zip`;
   - precise `.tar.gz` beats vague `.zip` (primary score still wins);
   - non-zip-format tie stays `Multiple`;
   - Linux **and** Windows cases;
   - `sort_by_score` places zip first.
8. **Validate** in order: `cargo check -q` → `cargo test -q` →
   `cargo clippy -q --fix --allow-dirty` → `cargo clippy -q -- -D warnings`.

## Trade-offs
- **Rejected — flat `+1` in `calculate_match_score`:** violates the
  musl-over-glibc-zip decision.
- **Rejected — musl bonus `+2`:** regresses precise glibc vs vague musl.
- **Chosen — primary score + `is_zip` tie-break:** leaves all existing scores/tests
  untouched; format only decides genuinely tied candidates.
- **Consequence — non-zip formats stay mutually tied:** `.tar.gz` vs `.tar.xz` vs
  `.exe` with no zip still prompts, i.e. current behavior preserved.

## Open questions
None — all resolved.

## Next step
Add `is_zip` and the `best_unique_match` tie-break in `src/asset.rs`, then land the
tests that lock in `musl tar.gz` beating `glibc zip` and `precise tar.gz` beating
`vague zip`.
