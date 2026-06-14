## Why

Currently the program upgrades to the latest version automatically without any user confirmation. This can disrupt ongoing work. Users need control over when upgrades happen.

## What Changes

- When an upgrade is available (local version differs from remote), show a confirmation dialog before proceeding
- If the user cancels, skip the upgrade silently
- If local and remote versions match, upgrade proceeds without prompt (no-op)
- The confirmation dialog displays the current and target version numbers

## Capabilities

### New Capabilities
- `confirm-upgrade`: Upgrade confirmation dialog that appears when local and remote versions differ, allowing the user to accept or cancel the upgrade

### Modified Capabilities
- *(none — no existing spec changes)*

## Impact

- Upgrade flow in the main application entry point
- No new dependencies required (leverage existing dialog/UI infrastructure)
