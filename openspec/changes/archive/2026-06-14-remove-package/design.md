## Context

`grd` has a flat CLI (`--list`, `--list-platforms`) with a positional `repo` argument. Installed packages are tracked in `~/.grd/state.toml` mapping each `owner/repo` to `{tag, asset}`. However, the install destination path is **not persisted** — it defaults to current directory or `--destination` at install time. There is no uninstall capability.

## Goals / Non-Goals

**Goals:**
- Add a `--remove` flag that deletes the installed binary for a given `owner/repo`
- Remove the corresponding entry from `~/.grd/state.toml`
- Persist installation destination in cached state so removal knows where the binary lives
- Be idempotent — warn but don't error if binary or state entry is already absent

**Non-Goals:**
- Interactive prompt for confirmation (can be added later; for now a flag is enough)
- Remove archive files or temporary download artifacts
- Bulk remove or remove-all

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| CLI shape | `--remove` flag (not a subcommand) | Consistent with existing `--list` pattern; minimal refactor. The positional `repo` argument is reused to specify which package to remove. |
| Binary location | Store `destination` in `CachedRelease` | On install, persist the destination path alongside tag and asset. On remove, read it back. Backwards-compatible — `destination` is `Option<String>` with `#[serde(default)]` so old state files don't break. |
| State mutation | Add `State::remove_cached(repo) -> Option<CachedRelease>` | Returns the entry so the caller can read the destination before removing it. Single source of truth for the removal. |
| Binary deletion | `fs::remove_file()` on the resolved path | Derive binary name the same way install does — from repo name (or `--bin-name` override, but that's not stored). On Windows append `.exe`. |
| Error handling | Soft warnings, never hard errors | If the binary file is already gone, warn and still clean up the state entry. If no state entry exists, warn and skip file deletion. |

**Alternatives considered:**
- **Explicit `--destination` on remove**: Rejected — user shouldn't need to remember where they installed something. Persisting destination in state is more reliable.
- **Separate `remove` subcommand**: Rejected — too much CLI restructuring for this change. Can migrate later if the tool grows more commands.
- **Search PATH for the binary**: Rejected — fragile, and the binary might not be on PATH (user may have installed to a custom dir with `-d`).

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| Old state files lack `destination` field | Field is `Option<String>` with `#[serde(default)]` = `None`. Remove with `--destination` flag fallback; if neither exists, warn and only clean state. |
| Binary name may differ from repo name (user used `--bin-name`) | The `--bin-name` override is **not** stored in state. A follow-up change should add it, but for now the remove command infers binary name from repo name (same logic as install path when no `--bin-name` was given). This is correct for the common case. |
| Multiple installs of same repo to different destinations | State only tracks one entry per repo (last install wins). Remove cleans the last known location. |
| Binary was never installed (state entry only, no file) | Warn "binary not found at {path}" and still clean up the state entry. |

## Migration Plan

- **For existing state files**: The new `destination` field is optional (`Option<String>`), so old files load fine. They will have `destination = None` until the user re-installs a package (which will persist the destination going forward).
- **No deployment steps**: This is a CLI tool, users rebuild from source.

## Open Questions

- Should `--remove` also accept a `--destination` override for old state files that lack the persisted path? (Design assumes yes, as a fallback.)
