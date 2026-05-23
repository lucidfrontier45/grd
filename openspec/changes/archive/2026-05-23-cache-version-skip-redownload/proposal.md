## Why

`grd` always re-downloads the same release on every invocation, even when the binary hasn't changed. Wastes bandwidth and time. Need persistent version tracking to skip redundant downloads.

## What Changes

- After successful download, save release tag_name to `~/.grd/state.toml` keyed by repo (owner/name)
- Before fetching latest release (no `--tag`), compare saved version against latest — skip download if match
- Add `--force` flag to `grd` CLI to bypass cache check and force re-download
- Create `~/.grd/` directory on first use if absent

## Capabilities

### New Capabilities
- `download-version-cache`: Persist last-downloaded release version per repo in `~/.grd/state.toml`. On `grd owner/repo` (latest), skip download if stored version matches latest GitHub release. `--force` bypasses cache.

### Modified Capabilities
<!-- No existing specs change behavior -->
- (none)

## Impact

- **New file:** `src/state.rs` — read/write `~/.grd/state.toml`
- **Modified:** `src/cli.rs` — add `--force` flag
- **Modified:** `src/main.rs` — insert version check after fetch, persist after download
- **New dir:** `~/.grd/` created on first download
- **Dependency:** `dirs` crate for platform config dir (or use `std::env::var("HOME")`)
