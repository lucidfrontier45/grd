## Context

`grd` downloads GitHub release binaries. No state persistence. Every `grd owner/repo` fetches latest release info and downloads the asset unconditionally. Repetitive for unchanged releases.

## Goals / Non-Goals

**Goals:**
- Persist last-downloaded release tag per repo
- Skip download when latest tag matches persisted tag
- `--force` flag to bypass cache
- Zero behavior change when no state file exists (fresh install)

**Non-Goals:**
- Cache the binary itself (bandwidth is cheap, state lookups are fast)
- Version pinning or lockfile semantics
- Concurrent access safety (single-user CLI tool)

## Decisions

1. **State file: `~/.grd/state.toml`**
   - TOML chosen because serde already in deps; `toml` crate is minimal addition
   - Location: `{home_dir}/.grd/state.toml`
   - Home resolution: `HOME` env var on Unix, `USERPROFILE` on Windows. No new dep needed.
   - Structure:
     ```toml
     [versions]
     "owner/repo" = "v1.0.0"
     "other/repo" = "v2.3.1"
     ```

2. **Check point: after fetch, before download**
   - `main.rs`: after `fetch_release_info` returns, if `--tag` is None, read state file, compare `release.tag_name` against stored version
   - If match AND no `--force`: print "Already at latest version vX.Y.Z" and exit 0
   - If no match or `--force`: proceed to download

3. **Save point: after successful extract**
   - `main.rs`: after `extract_and_save` succeeds, update state file with `release.tag_name` for this repo
   - If `--list` or `--list-platforms`: no state write (no download occurred)

4. **`--force` flag**
   - `cli.rs`: add `--force` bool flag to `Args`
   - Unlike most booleans in Clap, this defaults to `false` and is explicitly set by user

5. **Error handling**
   - State file not found → download (fresh start)
   - State file corrupt (invalid TOML) → log warning, download, overwrite
   - Can't write state → log warning, proceed (non-fatal)

## Risks / Trade-offs

- **Race condition**: if `grd` is killed between state read and download completion, next run will re-download. Acceptable for CLI tool. Acceptable.
- **HOME unset**: crash on state file path construction. Mitigation: print clear error ("Set HOME or USERPROFILE env var"), exit 1.
- **`toml` dep**: adds a compile-time dependency. Trade-off for human-readable state file vs rolling custom parser.
