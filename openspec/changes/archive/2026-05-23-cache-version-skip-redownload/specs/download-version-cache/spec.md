# Download Version Cache

## Purpose

Persist last-downloaded release version per repository in `~/.grd/state.toml`
to skip redundant downloads when the latest release hasn't changed.

## Requirements

### Requirement: State persistence
The system SHALL persist the last-downloaded release tag for each repository in a TOML state file.

#### Scenario: Save version after successful download
- **WHEN** a release is downloaded, extracted, and saved successfully
- **THEN** the system SHALL write `release.tag_name` to `~/.grd/state.toml` under `[versions]` keyed by `owner/repo`
- **AND** the directory `~/.grd/` SHALL be created if absent

#### Scenario: Skip state write on list commands
- **WHEN** the `--list` or `--list-platforms` flag is used
- **THEN** the system SHALL NOT write to the state file

### Requirement: Version check on latest download
The system SHALL compare the latest release version against the cached version before downloading.

#### Scenario: Skip download when version matches
- **WHEN** `--tag` is NOT specified
- **AND** `~/.grd/state.toml` exists
- **AND** the latest release `tag_name` matches the stored value for that repo
- **AND** `--force` is NOT set
- **THEN** the system SHALL print "Already at latest version <tag_name>"
- **AND** exit with code 0
- **AND** SHALL NOT download or extract anything

#### Scenario: Download when no cached version
- **WHEN** `--tag` is NOT specified
- **AND** no state file exists or repo not in state file
- **THEN** the system SHALL proceed with download normally

#### Scenario: Always download with --tag
- **WHEN** `--tag` is explicitly specified
- **THEN** the system SHALL proceed with download normally
- **AND** SHALL NOT check the cache

#### Scenario: Force re-download
- **WHEN** `--force` flag is set
- **THEN** the system SHALL skip the cache check
- **AND** download the asset unconditionally

### Requirement: Graceful error handling
The system SHALL handle state file errors without crashing the download.

#### Scenario: Corrupt state file
- **WHEN** `~/.grd/state.toml` exists but is invalid TOML
- **THEN** the system SHALL print a warning to stderr
- **AND** proceed with download normally
- **AND** overwrite the file with valid state after successful download

#### Scenario: Unwritable state directory
- **WHEN** `~/.grd/` cannot be created or written to
- **THEN** the system SHALL print a warning to stderr
- **AND** proceed with download normally (non-fatal)
