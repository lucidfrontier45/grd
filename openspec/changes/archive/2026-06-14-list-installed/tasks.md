## 1. CLI Flag

- [x] 1.1 Add `list_installed: bool` field with `#[arg(long)]` to `Args` in `src/cli.rs`
- [x] 1.2 Mark `repo` as not required when `--list-installed` is set (or keep it required and error at runtime)

## 2. Display Logic

- [x] 2.1 Add a `display_list()` method or standalone function that loads `State` and prints each entry as `owner/repo (tag, asset)`
- [x] 2.2 Handle the empty-state case — print "No installed packages found." when state is empty

## 3. Wire Up in main

- [x] 3.1 Add an early-return branch in `main.rs` for `args.list_installed` (before the `repo`-dependent logic)
- [x] 3.2 Error if both `--list-installed` and a positional repo arg are provided

## 4. Tests

- [x] 4.1 Add unit test for `display_list()` with multiple entries
- [x] 4.2 Add unit test for empty state output
- [x] 4.3 Add CLI arg test for `--list-installed` parsing
