## 1. CLI Flag

- [x] 1.1 Add `-y` / `--yes` boolean flag to `Args` struct in `src/cli.rs`

## 2. Confirmation Prompt

- [x] 2.1 Add a `confirm_upgrade` function that prints the version diff prompt and reads stdin
- [x] 2.2 Return `true` on `y`/`Y`, `false` otherwise (default to no)

## 3. Wire Into Flow

- [x] 3.1 In `src/main.rs`, after the cache-miss check (line 90), insert confirmation prompt when version differs
- [x] 3.2 Skip prompt when `args.yes` is set
- [x] 3.3 Exit gracefully when user declines

## 4. Tests

- [x] 4.1 Add CLI test for `--yes` flag parsing
- [x] 4.2 Add integration test for confirmation flow
