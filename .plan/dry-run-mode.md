# Plan: `--dry-run` mode for grd

## Goal
`grd owner/repo --dry-run` resolves release + asset and prints what would happen, without downloading, extracting, writing state, or creating dirs.

## Background
Main flow in `src/main.rs` (lines ~105-242): resolve dest → `create_dir_all` → GitHub API fetch release → normalize os/arch → select asset → bin_name → force/upgrade check → `download_asset` → `extract_and_save` → `state.set_cached`+`save`. Flag lives in `src/cli.rs` `Args`.

## Approach
1. **`src/cli.rs`**: add `#[arg(long)] pub dry_run: bool` to `Args`. Doc: "Simulate installation without downloading". No alias, no conflicts. Add parse test (`--dry-run` true, default false).
2. **`src/main.rs`**: after `println!("Selected asset: {}", asset.name)` and bin_name resolution, insert:
   - `if args.dry_run { ... report ...; return Ok(()) }`
3. **Report content**:
   - `[dry-run] would install '{bin_name}' to {dest}`
   - `[dry-run] source: {repo} {release.tag_name} → {asset.name}`
   - already-installed detection: if `!args.force` and target exists + same version cached → `[dry-run] already at {asset.name} {tag}; no action` instead of would-install. (Reuse existing logic shape; dry-run never prompts, never returns non-zero.)
4. **Short-circuit placement**: before force/upgrade prompt AND before `create_dir_all` (line 112). Gate `create_dir_all` with `!args.dry_run`. `--force` ignored in dry-run (report "would re-download" via same would-install line).
5. **Tests**: unit test for flag parse in cli.rs; check `src/tests.rs` patterns for main-path coverage (likely flag-parse test only — no network in tests).

## Trade-offs
- **Gate `create_dir_all` vs leave it**: leaving it = dry-run creates dest dir = side effect, violates "no actual changes". Gated.
- **Early exit before force/prompt block**: keeps dry-run non-interactive, never blocks on stdin, never returns error for upgrade-prompt path. Matches "dry-run wins".
- **Keep API calls**: user confirmed — dry-run still resolves real asset, output trustworthy.

## Open questions
- None blocking. (Exact test strategy for main path pending `src/tests.rs` inspection.)

## Next step
Implement: edit `src/cli.rs` (flag + tests), edit `src/main.rs` (gate + report), run `cargo check -q`, `cargo test -q`, clippy.
