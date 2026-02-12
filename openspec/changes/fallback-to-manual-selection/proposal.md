## Why

When no asset matches the detected host platform, the current implementation exits with an error instead of offering alternatives. Users should be able to manually select from available assets when automatic platform matching fails, improving usability and flexibility.

## What Changes

- **BREAKING**: Remove `--first` flag functionality - manual selection is now always activated
- Add `--select` flag to explicitly force manual selection for all scenarios
- Modify `select_asset` function to sort matching assets based on best match score
- When no assets match (or `--select` flag is used): prompt user to select from all available assets
- Display all assets with their names, sizes, and match scores
- Collect user input and validate selection
- Return selected asset on valid input

## Capabilities

### New Capabilities
- `asset-selection`: Manual asset selection from multiple matches including fallback when no automatic match exists
- `force-selection`: Explicit flag to force manual selection for all scenarios

### Modified Capabilities
- No existing capabilities require spec-level changes

## Impact

- **src/asset.rs**: Modify `select_asset` function to remove `--first` flag, add `force_select` parameter, sort matching assets by match score, and handle zero matches with manual selection
- **src/main.rs**: Remove `--first` flag handling, add `--select` flag handling, always prompt user when no assets match or `--select` is used
- **src/cli.rs**: Remove `--first` flag, add `--select` flag to CLI arguments
- **src/tests.rs**: Add integration tests for manual selection fallback scenario and `--select` flag
