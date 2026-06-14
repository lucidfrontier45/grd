## Context

`grd` tracks installed packages in `~/.grd/state.toml` via the `State` struct (`src/state.rs`). The `State.versions` field is a `HashMap<String, CachedRelease>` mapping `owner/repo` to its tag, asset name, and optional destination. Currently there is no CLI command to list these entries.

## Goals / Non-Goals

**Goals:**
- Add a `--list-installed` flag that reads `State` and prints entries
- Work standalone (no `owner/repo` argument required)
- Mutually exclusive with install/remove paths

**Non-Goals:**
- No formatting flags (JSON, table, etc.) — plain text output only
- No filtering or sorting options
- No remote fetching — purely local state inspection

## Decisions

- **Flag name `--list-installed`** follows existing `--list` (list releases) and `--remove` conventions. Unlike `--remove` which requires a repo, this flag is standalone.

- **Early-return in `main`** (same pattern as `--list`, `--list-platforms`, `--remove`): avoid needing `clap::ArgGroup` or conflict validation. If `--list-installed` and a positional arg are both given, we print an error and exit — simple and explicit.

- **No new public API** needed: `State::load()` + iterating `state.versions` is sufficient. A helper `fn display(&self)` on `State` or `CachedRelease` keeps the printing logic testable.

- **Output format**: one entry per line as `owner/repo (tag: v1.0.0, asset: foo.tar.gz)`. Consistent with existing `--list` output style.

## Risks / Trade-offs

- **Empty state edge case**: guard with a friendly "No installed packages found." message rather than printing nothing — avoids silent empty output confusion.
