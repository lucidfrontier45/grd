# Platform Alias Matching

## Purpose

Define requirements for normalizing platform identifier aliases and performing case-insensitive matching for asset selection.

## Requirements

### Requirement: Platform alias normalization
The system SHALL normalize platform identifier aliases to canonical OS names before matching assets.

#### Scenario: Windows variant alias matching
- **WHEN** an asset is tagged with platform identifier `win`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Windows 32-bit alias matching
- **WHEN** an asset is tagged with platform identifier `win32`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Windows 64-bit alias matching
- **WHEN** an asset is tagged with platform identifier `win64`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Canonical windows identifier matching
- **WHEN** an asset is tagged with platform identifier `windows`
- **THEN** the system SHALL match it to `OS=windows`

### Requirement: Platform-specific exclusions
The system SHALL NOT match assets to incorrect operating systems based on substring matches.

#### Scenario: Darwin assets excluded from Windows matching
- **WHEN** an asset name contains "darwin" (e.g., "apple-darwin", "x86_64-darwin")
- **THEN** the system SHALL NOT match it to `OS=windows`

#### Scenario: Darwin assets excluded from win alias matching
- **WHEN** an asset name contains "darwin" and the platform identifier is `win`
- **THEN** the system SHALL NOT match it to `OS=windows`

### Requirement: Case-insensitive platform matching
The system SHALL perform case-insensitive matching for all platform identifiers.

#### Scenario: Lowercase alias matching
- **WHEN** an asset is tagged with platform identifier `win`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Uppercase alias matching
- **WHEN** an asset is tagged with platform identifier `WIN`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Mixed case alias matching
- **WHEN** an asset is tagged with platform identifier `Win32`
- **THEN** the system SHALL match it to `OS=windows`

#### Scenario: Mixed case with numbers matching
- **WHEN** an asset is tagged with platform identifier `WiN64`
- **THEN** the system SHALL match it to `OS=windows`
