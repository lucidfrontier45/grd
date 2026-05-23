## ADDED Requirements

### Requirement: tar.xz format detection
The system SHALL detect `.tar.xz` file format by matching the filename suffix.

#### Scenario: tar.xz detected by filename suffix
- **WHEN** an asset filename ends with `.tar.xz`
- **THEN** the system SHALL route it to the `.tar.xz` extraction path

#### Scenario: other formats unaffected
- **WHEN** an asset filename ends with `.zip`, `.tar.gz`, `.tgz`, or an unrecognized extension
- **THEN** the system SHALL use the existing extraction paths, unchanged

### Requirement: tar.xz extraction
The system SHALL decompress `.tar.xz` archives using an xz decoder, then parse the resulting tar stream and extract the binary matching the target name. The extracted binary SHALL be placed at the destination directory with the correct name and executable permissions (Unix).

#### Scenario: successful tar.xz extraction
- **WHEN** a `.tar.xz` archive contains a file matching the target binary name
- **THEN** the system SHALL extract it to the destination directory with executable permissions (mode 0o755 on Unix)

#### Scenario: binary not found in tar.xz
- **WHEN** a `.tar.xz` archive does not contain a file matching the target binary name
- **THEN** the system SHALL fail with a clear error message indicating the binary was not found

#### Scenario: corrupted tar.xz handled gracefully
- **WHEN** a `.tar.xz` archive is corrupted or invalid
- **THEN** the system SHALL fail with a clear error message indicating the archive is corrupted

### Requirement: no-decompress interaction
When the `--no-decompress` flag is set, `.tar.xz` files SHALL be saved as raw files without any decompression, consistent with existing behavior for other formats.

#### Scenario: no-decompress saves tar.xz as-is
- **WHEN** `--no-decompress` is set and the asset is `.tar.xz`
- **THEN** the system SHALL save the raw bytes to the destination without decompressing
