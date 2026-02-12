## Context

**Current State:**
- `asset::select_asset` filters assets by OS/architecture
- When `matches.len() == 0`, function calls `bail!("No matching asset found for {os}-{arch}")`
- This causes immediate exit with error message
- User has no way to manually select alternative asset if auto-detection fails
- `--first` flag allows selecting first match or erroring

**Constraints:**
- Must maintain compatibility with existing CLI arguments (except for removing `--first` and adding `--select`)
- Must handle interactive prompt without breaking non-interactive usage

**Stakeholders:**
- End users who may not have matching assets for their platform
- CLI users who want manual override when auto-selection fails
- Automation scripts that need predictable behavior

## Goals / Non-Goals

**Goals:**
- Remove `--first` flag - manual selection is now the default behavior
- Add `--select` flag to explicitly force manual selection for all scenarios
- Sort matching assets based on best match score when multiple candidates exist
- When no assets match or `--select` flag is used: prompt user to select from all available assets
- Display all assets with their names, sizes, and match scores for informed selection
- Validate user input before proceeding
- Maintain existing behavior for normal matching scenarios
- Support both interactive and non-interactive workflows

**Non-Goals:**
- Automatic fallback to alternative architectures (e.g., x86_64 → i686)
- Automatic fallback to alternative OS distributions
- Suppressing error messages completely
- Changing asset naming conventions

## Decisions

### Decision 1: Remove `--first` Flag

**Choice**: Remove `--first` flag functionality entirely

**Rationale**:
- Simplifies user experience - no need to decide between automatic and manual selection
- Reduces confusion about when to use `--first` vs. manual selection
- Makes the tool more predictable and easier to use
- Manual selection is now the default behavior

**Alternative Considered**: Keep `--first` flag
- Rejected: Adds complexity and confusion
- Rejected: Users don't understand when to use it
- Rejected: Manual selection is always better fallback

### Decision 2: Add `--select` Flag

**Choice**: Add `--select` flag to force manual selection for all scenarios

**Rationale**:
- Explicit control for automation scripts that want manual selection
- Clear intent when user wants to browse all available assets
- Consistent with CLI design patterns
- Allows both automatic (default) and manual (with flag) selection

**Alternative Considered**: Always prompt for all scenarios
- Rejected: Breaks automation scripts that rely on predictable behavior
- Rejected: Disrupts normal usage when automatic selection works

### Decision 3: Match Score Calculation

**Choice**: Calculate match score based on OS and architecture matches

**Rationale**:
- Provides better user experience by showing closest matches first
- Helps users understand which assets are most suitable
- Clear visual indicator of match quality

**Alternative Considered**: Random order or original order
- Rejected: Unpredictable and less helpful
- Rejected: Original order doesn't indicate relevance

**Score Logic**:
- Exact OS match: +2 points
- Platform alias match: +1 point
- Architecture match: +1 point
- Cross-arch match: 0 points
- Cross-OS match: 0 points

**OS Matching Patterns**:
- Windows: `windows`, `pc-windows`, `win64`, `win32`, `win`
- macOS: `apple-darwin`, `macos`
- Linux: `linux`, `unknown-linux`

**Architecture Matching Patterns**:
- x86_64: `x86_64`, `amd64`, `x64`
- aarch64: `aarch64`, `arm64`

**Example Scores**:
- User on Linux-x86_64 with `grd-linux-x86_64.tar.gz`: 2 (OS) + 1 (arch) = 3 points
- User on Linux-x86_64 with `grd-apple-darwin-x86_64.tar.gz`: 1 (OS alias) + 1 (arch) = 2 points
- User on Linux-x86_64 with `grd-darwin-aarch64.tar.gz`: 1 (OS alias) + 0 (cross-arch) = 1 point

### Decision 4: Manual Selection When No Match

**Choice**: Prompt user when:
- No assets match detected platform (default behavior)
- `--select` flag is specified

**Rationale**:
- `--first` flag is removed, so manual selection is the default fallback
- `--select` flag provides explicit control for automation and browsing
- Provides best user experience for edge cases
- Simpler code with clear behavior

**Alternative Considered**: Only prompt if stdin is available
- Rejected: Breaks automation scenarios where no interactive prompt is expected
- Rejected: Confusing behavior (some users get error, some get prompt)

### Decision 5: Prompt Behavior

**Choice**: Print formatted list with numbers, sizes, and match scores, validate 1..N input

**Rationale**:
- Clear user experience
- Easy to understand available options
- Simple validation prevents invalid input
- Match scores help users make informed decisions

**Alternative Considered**: Interactive prompts asking "Select asset #:"
- Rejected: Cumbersome for users
- Rejected: Requires more complex state management

### Decision 6: Display Format

**Choice**: Show match score as raw number in brackets

**Rationale**:
- Simple and easy to understand
- No ambiguity about scoring system
- Compact format fits multiple assets

**Display Format**:
```
1. asset-name.tar.gz (1024 KB) [3]
2. another-asset.zip (2048 KB) [2]
```

**Alternative Considered**: Stars (⭐⭐⭐, ⭐⭐, ⭐)
- Rejected: Requires mapping logic for display
- Rejected: Less precise than raw scores

### Decision 7: Tie-Breaker

**Choice**: No tie-breaker - keep original order

**Rationale**:
- Simpler implementation
- No significant UX difference
- Original order is predictable

**Alternative Considered**: Alphabetical sort
- Rejected: Adds complexity
- Rejected: Users expect to see most relevant matches first based on scores

### Decision 8: Exclude Filter in Display

**Choice**: Don't show excluded assets at all

**Rationale**:
- Cleaner display
- Users can't select filtered assets anyway
- Reduces confusion

**Alternative Considered**: Show with "EXCLUDED" marker
- Rejected: Adds visual clutter
- Rejected: No benefit to showing filtered assets

## Risks / Trade-offs

**Risk**: Interactive prompt requires valid stdin → Mitigation: Use `io::stdout().flush()` and error handling, treat as non-interactive if stdin unavailable

**Risk**: User enters invalid input → Mitigation: Validate input loop with clear error messages, limit to 1..N range

**Risk**: Breaking automation scripts → Mitigation: `--select` flag provides explicit control, automation scripts can use it for manual selection

**Risk**: Performance impact from sorting and interactive prompts → Mitigation: Only occurs when no automatic match or `--select` flag is used (rare edge case)

**Trade-off**: Removes `--first` flag (breaking change) → Acceptable: Improved UX and `--select` flag provides alternative for automation

## Migration Plan

1. Remove `--first` flag from CLI arguments in src/cli.rs
2. Add `--select` flag to CLI arguments in src/cli.rs
3. Update src/main.rs to remove `--first` flag handling and add `--select` flag handling
4. Modify `select_asset` function to accept `force_select` parameter
5. Add match score calculation function `calculate_match_score` to asset.rs
6. Add sorting function `sort_by_score` to asset.rs
7. Modify `select_asset` function to use sorted assets and handle zero-match case with manual selection
8. Add helper functions `show_all_assets` and `collect_selection` for zero-match case
9. Update error handling in `src/main.rs` to rely on new selection behavior
10. Add tests for manual selection and sorting scenarios in `src/tests.rs`
11. No migration needed - backward compatible for existing workflows (except removal of `--first` flag and addition of `--select` flag)

## Open Questions

- What happens if user enters non-numeric input?
  - **Decision**: Show error message and retry prompt
- Should we limit number of retry attempts?
  - **Decision**: No limit - infinite retries until valid input
- Should we show total match score or individual component scores?
  - **Decision**: Show total score for simplicity and clarity
