## Why

Users currently have no way to see which packages they've installed via grd. The state file (`~/.grd/state.toml`) tracks installed repos, tags, and assets, but there's no CLI command to surface this information. This makes it hard to remember what's been installed or check which version is current.

## What Changes

- Add `--list-installed` flag to `grd` that reads `~/.grd/state.toml` and prints each installed package with its version and asset
- No positional `repo` argument required when `--list-installed` is set (it should work standalone)
- Mutually exclusive with the default install flow — no download, extraction, or install occurs

## Capabilities

### New Capabilities
- `list-installed`: Print all installed packages from the local state file with repo, tag, and asset info

### Modified Capabilities

*(none)*

## Impact

- `src/cli.rs`: Add `--list-installed` flag
- `src/main.rs`: Add early-return branch for `--list-installed` that loads state and prints entries
- `src/state.rs`: Possibly expose a display method or iterate over `State.versions` — minimal change since the data structure already exists
