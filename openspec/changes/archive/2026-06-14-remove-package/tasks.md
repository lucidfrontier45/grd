## 1. State Module Changes

- [x] 1.1 Add `destination: Option<String>` field to `CachedRelease` with `#[serde(default)]`
- [x] 1.2 Add `remove_cached(repo: &str) -> Option<CachedRelease>` method to `State` that removes and returns the entry
- [x] 1.3 Update `set_cached` to accept and persist a `destination` parameter
- [x] 1.4 Update the install flow in `main.rs` to pass `args.destination` to `set_cached`

## 2. CLI Changes

- [x] 2.1 Add `--remove` boolean flag to `Args` struct in `cli.rs`
- [x] 2.2 Add early-return path in `main.rs` for `--remove` flag (before download logic)

## 3. Remove Logic

- [x] 3.1 Implement remove flow: load state, look up entry, resolve binary path from destination + repo-derived name
- [x] 3.2 Add `fs::remove_file` call with warning if file already missing
- [x] 3.3 Call `remove_cached` and `state.save()` to persist the deletion
- [x] 3.4 Handle edge case: no state entry → warn and exit cleanly
- [x] 3.5 Handle edge case: destination is `None` and no `--destination` override → warn and only clean state

## 4. Tests

- [x] 4.1 Unit test `remove_cached` on populated state
- [x] 4.2 Unit test `remove_cached` on empty state (returns None)
- [x] 4.3 Unit test `remove_cached` idempotency (double remove returns None second time)
- [x] 4.4 Unit test `CachedRelease` deserialization with and without `destination` field
- [x] 4.5 Integration test: `grd --remove owner/repo` removes file and state entry
- [x] 4.6 Integration test: `grd --remove owner/repo` warns when binary already missing
- [x] 4.7 Integration test: `grd --remove owner/repo` warns when no cached entry
- [x] 4.8 Verify `cargo test` passes, `cargo clippy` is clean
