## ADDED Requirements

### Requirement: Architecture inference from platform identifiers
The system SHALL infer architecture from platform-specific identifiers that contain implicit architecture information.

#### Scenario: win32 architecture inference
- **WHEN** an asset is tagged with platform identifier `win32`
- **THEN** the system SHALL infer `Arch=x86_64`
- **AND** the system SHALL also set `OS=windows`

#### Scenario: win64 architecture inference
- **WHEN** an asset is tagged with platform identifier `win64`
- **THEN** the system SHALL infer `Arch=x86_64`
- **AND** the system SHALL also set `OS=windows`

#### Scenario: Generic win identifier without architecture inference
- **WHEN** an asset is tagged with platform identifier `win`
- **THEN** the system SHALL NOT infer any architecture
- **AND** the system SHALL only set `OS=windows`

#### Scenario: Canonical windows identifier without architecture inference
- **WHEN** an asset is tagged with platform identifier `windows`
- **THEN** the system SHALL NOT infer any architecture
- **AND** the system SHALL only set `OS=windows`
