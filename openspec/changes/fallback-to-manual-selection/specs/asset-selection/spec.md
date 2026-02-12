## ADDED Requirements

### Requirement: Assets are sorted by match score when multiple candidates exist

The system SHALL sort matching assets based on match score when multiple assets match the platform criteria.

#### Scenario: Assets sorted by OS match priority

- **WHEN** multiple assets match the specified OS but have different architecture matches
- **THEN** the system SHALL display assets with higher OS match score first

#### Scenario: Assets sorted by architecture match priority

- **WHEN** multiple assets match the specified architecture but have different OS matches
- **THEN** the system SHALL display assets with higher architecture match score first

#### Scenario: Assets sorted by combined score

- **WHEN** multiple assets match both OS and architecture criteria with different scores
- **THEN** the system SHALL display assets with higher combined match score first

#### Scenario: Assets with same score retain original order

- **WHEN** multiple assets have the same match score
- **THEN** the system SHALL preserve the original order from the list

### Requirement: Match score calculation rules

The system SHALL calculate match scores based on the following rules:

#### Scenario: Exact OS match gives +2 points

- **WHEN** asset name contains exact OS identifier (e.g., "linux" for linux target)
- **THEN** the system SHALL add +2 to the match score

#### Scenario: Platform alias OS match gives +1 point

- **WHEN** asset name contains platform alias (e.g., "win" for windows target)
- **THEN** the system SHALL add +1 to the match score

#### Scenario: Exact architecture match gives +1 point

- **WHEN** asset name contains exact architecture identifier (e.g., "x86_64" for x86_64 target)
- **THEN** the system SHALL add +1 to the match score

#### Scenario: Cross-arch match gives 0 points

- **WHEN** asset name contains different architecture than target (e.g., "aarch64" for x86_64 target)
- **THEN** the system SHALL add 0 to the match score

#### Scenario: Cross-OS match gives 0 points

- **WHEN** asset name contains different OS than target (e.g., "darwin" for linux target)
- **THEN** the system SHALL add 0 to the match score

#### Scenario: OS matching patterns

- **WHEN** target OS is windows
- **THEN** asset name patterns matching: "windows", "pc-windows", "win64", "win32", "win"

- **WHEN** target OS is macos
- **THEN** asset name patterns matching: "apple-darwin", "macos"

- **WHEN** target OS is linux
- **THEN** asset name patterns matching: "linux", "unknown-linux"

#### Scenario: Architecture matching patterns

- **WHEN** target architecture is x86_64
- **THEN** asset name patterns matching: "x86_64", "amd64", "x64"

- **WHEN** target architecture is aarch64
- **THEN** asset name patterns matching: "aarch64", "arm64"

### Requirement: Automatic download when only one asset matches

The system SHALL automatically download the asset when exactly one asset matches the platform criteria.

#### Scenario: Single matching asset downloaded automatically

- **WHEN** the asset selection process finds exactly one asset matching the detected platform (os-arch)
- **THEN** the system SHALL automatically download the asset without prompting the user
- **AND** the system SHALL proceed with the download and extraction process

#### Scenario: Non-terminal environment errors with multiple matches

- **WHEN** the asset selection process finds multiple assets matching but stdin is not a terminal (non-interactive environment)
- **THEN** the system SHALL return an error indicating multiple assets were found
- **AND** the error message SHALL suggest using --select flag or running in interactive terminal

### Requirement: Manual selection when multiple assets match

The system SHALL prompt the user to manually select an asset when multiple assets match the platform criteria, or when the `--select` flag is specified.

#### Scenario: User prompted when multiple assets match detected platform

- **WHEN** the asset selection process finds multiple assets matching the detected platform (os-arch)
- **THEN** the system SHALL display all matched assets with their names, sizes, and match scores
- **AND** the system SHALL wait for user input to select an asset

#### Scenario: User prompted when no assets match detected platform

- **WHEN** the asset selection process finds no assets matching the detected platform (os-arch)
- **THEN** the system SHALL display all available assets with their names, sizes, and match scores
- **AND** the system SHALL wait for user input to select an asset

#### Scenario: User forced to select with --select flag

- **WHEN** the asset selection process runs with the `--select` flag specified
- **THEN** the system SHALL prompt the user to select an asset from all available assets
- **AND** the system SHALL ignore any matching assets that would be automatically selected

#### Scenario: User successfully selects valid asset

- **WHEN** the system displays available assets and the user enters a valid number (1 to N) from the displayed list
- **THEN** the system SHALL return the selected asset
- **AND** the system SHALL proceed with the download and extraction process for the selected asset

#### Scenario: User enters invalid input

- **WHEN** the system displays available assets and the user enters invalid input (non-numeric, out of range, or empty)
- **THEN** the system SHALL display an error message indicating the input is invalid
- **AND** the system SHALL re-display the available assets and wait for new input

### Requirement: Manual selection respects filter criteria

The system SHALL only display assets that match the platform and exclusion filters when prompting for manual selection.

#### Scenario: Manual selection respects exclude filter

- **WHEN** user is prompted for manual selection with an exclude filter applied
- **THEN** the system SHALL only display assets that pass the exclusion filter

#### Scenario: Manual selection respects architecture filter

- **WHEN** user is prompted for manual selection with a specific architecture requirement
- **THEN** the system SHALL only display assets matching the specified architecture

#### Scenario: Manual selection respects OS filter

- **WHEN** user is prompted for manual selection with a specific OS requirement
- **THEN** the system SHALL only display assets matching the specified OS

## REMOVED Requirements

### Requirement: `--first` flag selection

The system SHALL allow selecting the first matching asset when `--first` flag is specified.

**Reason**: Removing `--first` flag to simplify user experience. Manual selection is now the default behavior, and `--select` flag provides explicit control when needed.

**Migration**: Users who previously used `--first` will now get manual selection for zero-match scenarios. Users with matching assets will continue to work as before. Users who want explicit manual selection can use the new `--select` flag.
