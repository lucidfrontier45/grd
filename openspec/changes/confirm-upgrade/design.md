## Context

`grd` downloads release binaries from GitHub repositories. When run as `grd lucidfrontier45/grd`, it fetches the latest release, checks a local state cache (`~/.grd/state.toml`), and either skips (already cached) or proceeds to download without any user confirmation. The download replaces the existing binary in `~/.grd/store/`.

The existing interactive patterns use simple `println!` + `stdin` readline (see `asset.rs:interactive_select`). No dialog/modal crate is used.

## Goals / Non-Goals

**Goals:**
- Add a confirmation step before downloading when the cached version differs from the remote version
- Display current version and target version in the prompt
- Add `-y` / `--yes` flag to skip confirmation dialog (for scripting/unattended use)
- `--force` (force download) skips the cache check entirely, so confirmation is naturally bypassed
- Keep the "Already at version X" fast-path unchanged

**Non-Goals:**
- Introduce a new UI crate or dialog framework
- Semver-aware comparison (string equality is sufficient)
- Self-upgrade detection of the running binary (CARGO_PKG_VERSION) — the existing cache mechanism suffices

## Decisions

1. **Use existing stdin prompt pattern** over a new dialog crate. The codebase already uses `println!` + `io::stdin().read_line()` in `asset.rs`. Adding `dialog` or `inquire` would introduce unnecessary dependency weight. A simple `Y/n` prompt is familiar to CLI users and consistent with `collect_selection()`.

2. **Insert confirmation between cache-check and download** (between steps 11 and 12 in `main.rs`). This is the narrowest insertion point — all version/asset resolution is already done, and the download hasn't started. No refactoring of existing logic is needed.

3. **Add `-y` / `--yes` flag** to skip confirmation. This is needed for scripting and CI use where stdin is not available. `--force` already skips the cache check entirely, so confirmation is naturally bypassed in that case — `--yes` is the orthogonal flag for "I've seen the version diff, proceed."

4. **Show version diff** in the prompt: `"Upgrade from v0.6.0 to v0.7.0? [Y/n]"`, where "from" is the cached tag and "to" is the remote `release.tag_name`. This gives the user actionable information.

## Risks / Trade-offs

- **Risk**: User accidentally presses Enter (default accept) and upgrades unintentionally → Mitigation: Default to "no" (require explicit `y`/`Y`). This is safer for a destructive action (binary replacement).
- **Risk**: No output when `--force` is used without a version change → Mitigation: Keep existing "Already at ..." message; force implies "do it anyway" not "be quiet."

## Open Questions

- Prompt default: "no" (safety) or "yes" (convenience)? Defaulting to "no" is safer, especially since `-y` provides an explicit opt-in for scripting.
