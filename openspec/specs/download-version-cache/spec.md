# Download Version Cache

## Purpose

Persist the last-downloaded release (version + asset name) per repository in `~/.grd/state.toml`
to skip redundant downloads when the same release artifact for that repo hasn't changed.

## Requirements

### Requirement: State persistence
The system SHALL persist the last-downloaded release (tag and asset name) for each repository in a TOML state file. Each repo SHALL have at most one entry — a subsequent download overwrites the previous.

#### Scenario: Save version and asset after successful download
- **WHEN** a release is downloaded, extracted, and saved successfully
- **THEN** the system SHALL write both `release.tag_name` and `asset.name` to `~/.grd/state.toml` under `[versions]` keyed by `owner/repo`
- **AND** the directory `~/.grd/` SHALL be created if absent
- **AND** any previous entry for that repo SHALL be overwritten

#### Scenario: Skip state write on list commands
- **WHEN** the `--list` or `--list-platforms` flag is used
- **THEN** the system SHALL NOT write to the state file

### Requirement: Version check on latest download
The system SHALL compare the latest release version and chosen asset name against the cached values before downloading.

#### Scenario: Skip download when asset and version match
- **WHEN** `--tag` is NOT specified
- **AND** `~/.grd/state.toml` exists
- **AND** the chosen `asset.name` and latest release `tag_name` both match the stored values for that repo
- **AND** `--force` is NOT set
- **THEN** the system SHALL print "Already at <asset_name> version <tag_name>"
- **AND** exit with code 0
- **AND** SHALL NOT download or extract anything

#### Scenario: Download when no cached version
- **WHEN** `--tag` is NOT specified
- **AND** no state file exists or repo not in state file
- **THEN** the system SHALL proceed with download normally

#### Scenario: Download same repo different asset
- **WHEN** `--tag` is NOT specified
- **AND** a cached entry exists for the same repo
- **AND** the chosen `asset.name` differs from the cached asset
- **THEN** the system SHALL proceed with download normally
- **AND** after successful download, the cached entry SHALL be overwritten with the new (asset, version) pair

#### Scenario: Download same repo different version
- **WHEN** `--tag` is NOT specified
- **AND** a cached entry exists for the same repo with a matching asset name
- **AND** `release.tag_name` differs from the cached tag
- **THEN** the system SHALL proceed with download normally
- **AND** after successful download, the cached entry SHALL be updated with the new version

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
