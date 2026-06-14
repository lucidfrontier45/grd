## ADDED Requirements

### Requirement: Remove package via --remove flag

The system SHALL provide a `--remove` flag that removes an installed package identified by `owner/repo`.

The `owner/repo` positional argument SHALL be required and used to look up the cached entry.

The flag MUST be mutually exclusive with the default install behavior — when `--remove` is set, no download, extraction, or install occurs.

#### Scenario: Remove removes binary and state entry

- **GIVEN** package `owner/repo` was previously installed
- **GIVEN** the cached state has an entry for `owner/repo` with a valid `destination`
- **GIVEN** the binary file exists at the resolved install path
- **WHEN** user runs `grd --remove owner/repo`
- **THEN** the binary file at the cached destination is deleted
- **THEN** the `owner/repo` entry is removed from `~/.grd/state.toml`
- **THEN** a success message is printed to stdout

#### Scenario: Remove is idempotent when binary already missing

- **GIVEN** package `owner/repo` has a cached state entry
- **GIVEN** the binary file does NOT exist at the cached destination
- **WHEN** user runs `grd --remove owner/repo`
- **THEN** a warning is printed: "binary not found at {path}"
- **THEN** the state entry is still removed

#### Scenario: Remove warns when no state entry exists

- **GIVEN** package `owner/repo` has no cached state entry
- **WHEN** user runs `grd --remove owner/repo`
- **THEN** a warning is printed that no cached entry exists
- **THEN** no file deletion is attempted
- **THEN** the command exits successfully

### Requirement: Persist install destination in state

When a package is installed via the default download + install flow, the system SHALL persist the destination directory in the cached state entry alongside `tag` and `asset`.

The `destination` field MUST be a string representation of the `--destination` path used during installation.

The field MUST be optional (`Option<String>`) with `#[serde(default)]` so existing state files without the field load correctly.

#### Scenario: Destination is saved on install

- **GIVEN** user runs `grd owner/repo -d /custom/path`
- **WHEN** the install completes successfully
- **THEN** the state entry for `owner/repo` contains `destination = "/custom/path"`
- **THEN** the entry also contains the expected `tag` and `asset`

#### Scenario: Default destination is saved as "."

- **GIVEN** user runs `grd owner/repo` (no `--destination` flag)
- **WHEN** the install completes successfully
- **THEN** the state entry for `owner/repo` contains `destination = "."`

#### Scenario: Old state file without destination loads without error

- **GIVEN** an existing state file `~/.grd/state.toml` with an entry that has no `destination` field
- **WHEN** `State::load()` is called
- **THEN** it succeeds
- **THEN** the loaded entry has `destination = None`

### Requirement: Resolve binary path for removal

The system SHALL resolve the binary path to delete by combining the cached `destination` with the binary name derived from the repo name (last segment after `/`, e.g. `owner/repo` → `repo`).

On Windows, `.exe` SHALL be appended to the binary name.

If `destination` is `None` in the cached entry and no `--destination` override is provided on the remove command, the system SHALL warn and skip file deletion (but still remove the state entry).

#### Scenario: Binary path resolved on Windows

- **GIVEN** cached entry has `destination = "C:\tools"` and repo is `foo/bar`
- **WHEN** running on Windows
- **THEN** the resolved path is `C:\tools\bar.exe`

#### Scenario: Binary path resolved on Unix

- **GIVEN** cached entry has `destination = "/usr/local/bin"` and repo is `foo/bar`
- **WHEN** running on Linux or macOS
- **THEN** the resolved path is `/usr/local/bin/bar`
