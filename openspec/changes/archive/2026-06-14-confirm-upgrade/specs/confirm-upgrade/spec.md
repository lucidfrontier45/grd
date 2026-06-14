## ADDED Requirements

### Requirement: Confirm upgrade when version differs

When the cached local version differs from the remote release version, the system SHALL prompt the user before proceeding with the download. The prompt SHALL display the current cached version and the target remote version.

#### Scenario: Versions differ — user confirms
- **WHEN** the cached version differs from the remote version
- **AND** `--yes` is not set
- **AND** the user enters `y` or `Y` at the prompt
- **THEN** the system SHALL proceed with the download

#### Scenario: Versions differ — user declines
- **WHEN** the cached version differs from the remote version
- **AND** `--yes` is not set
- **AND** the user enters anything other than `y` or `Y` at the prompt
- **THEN** the system SHALL exit with a message without downloading

#### Scenario: Prompt shows current and target versions
- **WHEN** the prompt is displayed
- **THEN** it SHALL show the cached version and the remote version in the message

### Requirement: Skip confirmation with `-y` / `--yes`

The system SHALL support a `-y` / `--yes` CLI flag that skips the confirmation prompt and proceeds with the download.

#### Scenario: `-y` flag set with version difference
- **WHEN** the cached version differs from the remote version
- **AND** `-y` or `--yes` is set
- **THEN** the system SHALL proceed with the download without prompting

#### Scenario: `-y` with `--force`
- **WHEN** both `-y` and `--force` are set
- **THEN** the system SHALL skip the cache check entirely (force download behavior) and no confirmation prompt is shown

### Requirement: No prompt when versions match

When the cached version matches the remote version, no confirmation prompt SHALL be shown and the existing fast-path message SHALL be displayed.

#### Scenario: Versions match — fast-path
- **WHEN** the cached version matches the remote version
- **THEN** the system SHALL print "Already at ..." and exit without downloading
- **AND** no confirmation prompt SHALL be shown
