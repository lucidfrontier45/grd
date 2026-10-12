# Refactor top-7 complexity hotspots (issue #29)

**Goal** — Refactor the 7 complexity hotspots from issue #29 and all new functions created in the process to be **as simple as possible — CC well below 10 wherever the code allows**, consolidating the triplicated asset-matching heuristics into one shared vocabulary, with zero behavior change.

**Background** — `CC = 1 + decision points` (issue's counting method: if / else-if / match arms / loops / `&&` / `||` / `?` / guards). Hotspots, verified against source:

| # | Function | File | CC (issue) |
|---|----------|------|------------|
| 1 | `main` | src/main.rs:47 | ~36 |
| 2 | `collect_matches` | src/asset.rs:615 | 33 |
| 3 | `has_explicit_arch_pattern` | src/asset.rs:97 | 24 |
| 4 | `calculate_match_score` | src/asset.rs:226 | 22 |
| 5 | `validate_repo` | src/github.rs:16 | 12 |
| 6 | `best_unique_match` | src/asset.rs:303 | 10 |
| 7 | `select_asset` | src/asset.rs:512 | 10 |

The OS/arch vocabulary is expressed three times and has drifted: `has_explicit_arch_pattern` (23 chained `||` substring tests + bare-ARM/EABI tokens), `calculate_match_score`'s `os_patterns`/`arch_patterns` tables, and `collect_matches`' filter closures (which special-case `win64`/`win32` for windows differently from the score table). Public API is frozen: `select_asset`, `SelectMode`, `AssetSelection`, and `Asset` signatures must not change.

**Approach** —

1. **Characterization tests first** — capture current `collect_matches` / `calculate_match_score` / `has_explicit_arch_pattern` behavior on the tricky cases (bare-ARM tokens and EABI triples, `win64`/`win32` on windows vs non-windows, musl bonus, gnu-neutrality, default-arch fallback) before touching anything.
2. **Shared vocabulary** — add static tables in `src/asset.rs`: `ARCH_ALIASES: &[(canonical, &[aliases])]` and `OS_ALIASES` + `OS_EXCLUSIONS`. Derive from these: `has_explicit_arch_pattern` (table lookup + existing `has_bare_arm_token`/EABI tokens), `collect_matches`' os/arch predicates (keeping the default-arch fallback logic intact), and `calculate_match_score`'s pattern lists. **Scoring weights stay local** — only the token lists are shared, so scores are byte-identical.
3. **Split `collect_matches`** — extract the filter closure into small named predicates (`os_matches`, `arch_matches`, …), each itself CC 1–4, not merely "under 10".
4. **Simplify `best_unique_match`** — extract the `.zip` tie-break and the top-score tracking into trivial helpers; the loop body should be a couple of comparison arms at most.
5. **Split `select_asset`** — one small helper per `SelectMode` arm (`select_interactive_all`, `select_filtered`, `select_default`); each helper decomposed further if it lands above ~CC 4.
6. **Split `main`** — extract the 5 subcommand handlers (`cmd_register`, `cmd_remove`, `cmd_info`, `cmd_list_installed`, `cmd_list_platform`) and the install pipeline into flat helpers (`resolve_dest`, `detect_platform`, `pick_asset`, `is_up_to_date`, `needs_upgrade_confirmation`, `run_install`, …), each an obviously-simple function. The 6-term upgrade chain (main.rs:255-274) becomes named single-purpose predicates, not one helper that keeps the chain.
7. **Simplify `validate_repo`** — hoist `is_valid_segment` to a named fn; flat owner/name checks.
8. **Validate** — `cargo check` → `cargo test` → `cargo clippy --fix --allow-dirty` → `cargo clippy -- -D warnings`. Existing tests must pass **unmodified** (behavior-identical guarantee). Then hand-recount CC for the 7 originals **and every new function**, iterating on any helper above ~CC 4 unless flattening it would hurt clarity.

**Trade-offs** —

- *Consolidate token lists only, not scoring*: a single "matcher" abstraction covering both filter and score was considered, but the filter and score encode genuinely different rules (filter is inclusive/existential, score is weighted with exclusions and musl bonus). Forcing them into one model risks behavior drift — sharing just the vocabulary gets the drift fix without the risk.
- *Decomposition floor*: splitting stops where further breakdown would produce functions that just shuttle data between each other. The goal is simplicity, not a pile of one-line wrappers. If a hotspot genuinely can't go below ~CC 6 without that, it stays at ~6 and the rationale is recorded.
- *File split of `asset.rs` (2215 lines) into `asset/` submodules*: deferred — not required by the issue and adds churn to the diff.
- *Stripping `main`'s dispatch arms into a command enum with handlers*: rejected — a 6-arm match is already near the CC floor for dispatch code.

**Open questions** — none.

**Next step** — Write the characterization tests (step 1), run them to lock in current behavior, then start the vocabulary consolidation.
