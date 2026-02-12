## 1. Core Implementation

- [ ] 1.1 Remove `--first` flag from CLI arguments in src/cli.rs
- [ ] 1.2 Add `--select` flag to CLI arguments in src/cli.rs
- [ ] 1.3 Update src/main.rs to remove `--first` flag handling and add `--select` flag handling
- [ ] 1.4 Modify `select_asset` function signature to remove `first` parameter and add `force_select` parameter
- [ ] 1.5 Modify `select_asset` function to always prompt when no match or `force_select` is true
- [ ] 1.6 Add match score calculation function with OS (exact +2, alias +1) and architecture (+1) scoring rules
- [ ] 1.7 Add sorting function that sorts by score (stable sort, no tie-breaker)
- [ ] 1.8 Add helper functions to asset.rs: `show_all_assets` and `collect_selection`
- [ ] 1.9 Add `force_select` logic to `select_asset` function for zero-match case with manual selection

## 2. Testing

- [ ] 2.1 Add unit test for exact OS match (+2 points)
- [ ] 2.2 Add unit test for platform alias OS match (+1 point)
- [ ] 2.3 Add unit test for exact architecture match (+1 point)
- [ ] 2.4 Add unit test for cross-arch match (0 points)
- [ ] 2.5 Add unit test for cross-OS match (0 points)
- [ ] 2.6 Add unit test for sorting assets by score
- [ ] 2.7 Add unit test for assets with same score (preserve order)
- [ ] 2.8 Add unit test for manual selection when no assets match (default behavior)
- [ ] 2.9 Add unit test for manual selection when `--select` flag is used
- [ ] 2.10 Add unit test for user entering valid selection
- [ ] 2.11 Add unit test for user entering invalid input
- [ ] 2.12 Add integration test for manual selection flow
- [ ] 2.13 Test that manual selection respects `--exclude` filter
- [ ] 2.14 Verify sorting order when multiple matches exist with different scores
- [ ] 2.15 Test that `--first` flag is not accepted (error case)
- [ ] 2.16 Test that `--select` flag forces manual selection even when assets match

## 3. Code Quality

- [ ] 3.1 Run `cargo check` to verify compilation
- [ ] 3.2 Run `cargo clippy` and fix any warnings
- [ ] 3.3 Run `cargo test` to ensure all tests pass
- [ ] 3.4 Update documentation/comments if needed
