## Purpose

List all packages installed via grd by reading the local state file (`~/.grd/state.toml`) and printing each entry with its version and asset information.

## ADDED Requirements

### Requirement: List installed packages via --list-installed flag

The system SHALL provide a `--list-installed` flag that prints all packages tracked in the local state file (`~/.grd/state.toml`).

When `--list-installed` is set, no positional `owner/repo` argument SHALL be required.

The flag MUST be mutually exclusive with the default install flow — no download, extraction, or install occurs.

#### Scenario: List installed packages prints repo tag and asset

- **GIVEN** the state file contains entries for `owner/repo` (tag `v1.0.0`, asset `foo-linux.tar.gz`) and `other/app` (tag `v2.3.1`, asset `bar-macos.zip`)
- **WHEN** user runs `grd --list-installed`
- **THEN** stdout contains `owner/repo`
- **THEN** stdout contains `v1.0.0`
- **THEN** stdout contains `foo-linux.tar.gz`
- **THEN** stdout contains `other/app`
- **THEN** stdout contains `v2.3.1`
- **THEN** stdout contains `bar-macos.zip`

#### Scenario: List installed shows message when nothing is installed

- **GIVEN** the state file is empty or does not exist
- **WHEN** user runs `grd --list-installed`
- **THEN** stdout contains a message indicating no packages are installed
- **THEN** the command exits successfully

#### Scenario: List installed with extra positional arg prints error

- **GIVEN** the user provides both `--list-installed` and a positional `owner/repo` argument
- **WHEN** user runs `grd --list-installed owner/repo`
- **THEN** the command exits with an error message about conflicting arguments

#### Scenario: List installed does not modify state file

- **GIVEN** the state file has known contents
- **WHEN** user runs `grd --list-installed`
- **THEN** the state file contents are unchanged after the command completes
