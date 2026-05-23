## 1. Dependencies

- [x] 1.1 Add `toml` crate to `Cargo.toml`

## 2. State Module

- [x] 2.1 Create `src/state.rs` with `State` struct containing `versions: HashMap<String, String>`
- [x] 2.2 Implement `State::load()` — read/parse `~/.grd/state.toml`, handle missing/corrupt file
- [x] 2.3 Implement `State::save()` — serialize and write to `~/.grd/state.toml`, create dir if absent
- [x] 2.4 Implement `State::get_version(repo)` and `State::set_version(repo, tag)`
- [x] 2.5 Add `state` module to `src/lib.rs`

## 3. CLI: --force Flag

- [x] 3.1 Add `force: bool` field to `Args` struct in `src/cli.rs` using `clap`

## 4. Main Pipeline: Version Check

- [x] 4.1 After `fetch_release_info` in `main.rs`: if `--tag` is None, load state and compare versions
- [x] 4.2 Print "Already at latest version <tag>" and exit 0 when cached version matches
- [x] 4.3 After successful `extract_and_save`: save `release.tag_name` to state file
- [x] 4.4 Skip state write for `--list` and `--list-platforms` commands
- [x] 4.5 Pass `--force` through download pipeline to bypass version check

## 5. Tests

- [x] 5.1 Test state load/save roundtrip with temp dir
- [x] 5.2 Test corrupt state file handled gracefully
- [x] 5.3 Test `--force` flag parsed correctly in cli tests
