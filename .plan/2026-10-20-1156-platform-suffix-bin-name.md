# Plan: `--bin-name` matches platform-suffixed binaries and names the installed file

## Goal

Today `--bin-name` is a *suffix* lookup key: it only matches an archive entry
whose path ends with the given name (at a path-component boundary). A release
that ships `app-bin-linux-x64` cannot be installed with `--bin-name app-bin`.

Desired:

- project `app`, archive entry `app-bin-<platform>` (`.exe` on Windows)
- `grd app --bin-name app-bin`
- → selects `app-bin-<platform>`, writes `app-bin` (`app-bin.exe` on Windows)

## Decisions (agreed)

1. **B** — the platform-suffix rule applies to the default lookup (repo
   basename) as well as to an explicit `--bin-name`.
2. **A** — the on-disk name follows the lookup name, not the archive entry
   name. `--rename` still wins over everything.
3. **A** — the `.exe` suffix follows the *target* platform (`--os`/host), not
   the matched entry.
4. **B** — when several suffixed candidates exist, rank them with the existing
   platform scoring in `src/asset.rs`; ties are an error with a listing.
6. **B** — `--bin-name` may contain a path (`pkg/bin/app`); only its last
   component becomes the on-disk name.
7. **A** — `--no-decompress` keeps writing the asset name (unless
   `--rename`); `--bin-name` stays out of it.
8. **Leave it** — an old install under a previous name is not deleted.

### On "which separator is the platform suffix boundary?"

Not needed to *find* the binary and not needed to *name* the installed file:
the saved name comes from the lookup name (decision 2), never from parsing the
entry. A separator only appears in one narrow place — rejecting junk
candidates. `--bin-name pi` must not match an entry called `pilot`. A
candidate qualifies when it either

- names the target OS (`os_score > 0`, reusing `asset.rs`'s vocabulary), or
- carries a `-`/`_` tag after the lookup name (`app-bin-v1.2.3`).

Both are cheap; neither is used to derive the saved name.

## Approach

### 1. `src/naming.rs`

- `base_name(name) -> &str`: the last path component (`a/b/c` → `c`, both
  `/` and `\\` separators).
- `installed_name(rename, lookup_name, os) -> String`: the on-disk name —
  `rename_output(rename, os)` when `--rename` is given, else
  `rename_output(base_name(lookup_name), os)`. Single source of truth, used
  by both `main.rs` (pre-extraction checks) and `extract.rs`.

### 2. `src/asset.rs`

Expose the scorer for reuse by the extractor:

- `platform_score(name, os, arch) -> i32`: `calculate_match_score` (os +
  arch + musl).
- `names_os(name, os) -> bool`: `os_score > 0`.

### 3. `src/extract.rs`

Lookup becomes two-tier:

- **Tier 1 (unchanged)** — `is_entry_match`: path-suffix match at a
  component boundary, with `.exe` stripped on Windows targets. First match in
  archive order wins.
- **Tier 2 (new)** — the entry's *basename* starts with the lookup basename
  and is longer (`app-bin` → `app-bin-linux-x64`). Candidates are filtered
  by `is_suffix_candidate` (names the target OS, or a `-`/`_` tag) and
  ranked by `platform_score`. The unique best wins; a tie is an error listing
  the candidates.

The saved name is `installed_name(rename, lookup_name, os)` — the entry's own
name is no longer used (decision 2), which also covers the "bare `.exe` asset"
path.

Implementation notes:

- `extract_and_save` gains an `arch` parameter (needed by the scorer).
- `DownloadSource` is wrapped in a replayable `ArchiveSource` (`Memory` /
  `Disk`) because tier 2 needs a second pass over a tar archive; the common
  path (tier 1) still reads the archive once.
- Zip keeps the entry index and re-opens it by index; tar re-opens the source
  and picks the chosen path.

### 4. `src/main.rs`

- Thread `arch` from `detect_platform` down to the extractor.
- `resolve_known_out_name` now always knows the name:
  `--no-decompress` → asset name (or `--rename`), otherwise
  `installed_name(...)`. The cached-filename fallback is no longer needed.
- `--rename` still wins over `--bin-name` everywhere.

### 5. Tests

- `src/extract.rs`: tier-2 match (`app-bin-linux-x64` → `app-bin`),
  Windows tier-2 (`app-bin-windows-x64.exe` → `app-bin.exe`), tier 1 still
  wins over tier 2, scoring picks the right platform among several
  candidates, a tie is an error, junk (`pilot`) is not a candidate.
- `src/main.rs`: name resolution (`--rename` > `--bin-name` > repo
  basename, Windows `.exe` rule).
- `src/tests.rs`: end-to-end shaped case.

### 6. Docs

`README.md`, `CONTEXT.md`: the new lookup rule and the new naming rule.

## Implementation notes (as built)

- `naming::Platform { os, arch }` carries the target platform through the
  pipeline: `extract_and_save` would otherwise take eight arguments and trip
  clippy's `too_many_arguments`.
- On a Windows target the `.exe` suffix is ignored when matching a suffixed
  candidate too, so `--bin-name app-bin.exe` finds `app-bin-windows-x64.exe`.
- `src/state.rs` now exposes one `env_lock()` shared with `src/tests.rs`:
  both modules write `GRD_STATE_PATH` and used two different mutexes, which
  made the suite flaky.

## Behaviour changes to call out

- The installed file is now named after the *lookup* name, not after the
  archive entry: `grd owner/Pi-Bolt` (entry `pi`) now writes `Pi-Bolt`.
- On a Windows target the installed name always ends in `.exe`.
- `--no-decompress` is unchanged.

## Trade-offs

- **Tier 2 before tier 1 (rejected)** — an exact match is what the user named;
  it must win over a better-scoring suffixed entry.
- **Stripping the suffix from the entry name (rejected)** — the saved name is
  the lookup name; parsing platform suffixes out of file names is guesswork.
- **Requiring the suffix to name a platform (rejected)** — `app-bin-v1.2.3`
  would not be found; the `-`/`_` tag fallback covers it.
- **Erroring on every tie (rejected)** — only ties error; a clear winner is
  installed.
