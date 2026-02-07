## Why

Current asset matching logic incorrectly handles Windows platform identifiers. Assets tagged with `windows`, `win`, `win32`, and `win64` should all match `OS=windows`, but the system currently fails to recognize these variants consistently. Additionally, `win32` and `win64` should be recognized as `Arch=x86_64` architecture, improving asset detection for Windows platforms.

## What Changes

- Update asset matching logic to recognize `windows`, `win`, `win32`, and `win64` as equivalent OS identifiers
- Add architecture inference for `win32` and `win64` to map to `Arch=x86_64`
- Ensure case-insensitive matching for all Windows platform identifiers
- Update platform detection tests to cover these Windows variants

## Capabilities

### New Capabilities
- `platform-alias-matching`: Support for platform identifier aliases and normalization (e.g., `win`, `win32`, `win64` → `windows`)
- `arch-inference`: Architecture inference from platform identifiers (e.g., `win32`, `win64` → `x86_64`)

### Modified Capabilities
- None (implementation-only changes to existing platform detection logic)

## Impact

- Platform matching logic in asset detection system
- Tests for platform identifier matching
- No API changes - internal behavior only
