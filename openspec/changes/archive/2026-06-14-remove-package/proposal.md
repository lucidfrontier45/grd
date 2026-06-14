## Why

`grd` can download and install binaries from GitHub releases, but there is no way to uninstall a previously installed package. Users must manually delete the binary and edit `~/.grd/state.toml`. This is error-prone and inconvenient.

## What Changes

- Add a `remove` subcommand (or `--remove` flag) that accepts a GitHub `owner/repo` argument
- Delete the installed binary from the filesystem
- Remove the corresponding `[versions]` entry from `~/.grd/state.toml`
- Gracefully handle cases where the binary is already deleted or the state entry is missing

## Capabilities

### New Capabilities
- `package-remove`: Remove an installed binary by `owner/repo`, cleaning up both the binary file and the state file entry

### Modified Capabilities

<!-- No existing spec-level behavior changes -->

## Impact

- **New CLI entry point**: Either a new flag (e.g., `--remove`) added to `src/cli.rs`, or a migration to clap subcommands
- **State module**: Add a `remove_cached()` method to `src/state.rs`
- **Install tracking**: The destination path is not currently persisted in state — this change may need to store the install destination, or derive it from convention (e.g., `--destination` default or the binary name derived from repo)
- **Error handling**: Idempotent removal — warn but don't error if binary or state entry is already absent
