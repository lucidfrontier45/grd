# Plan: `github_pat` in `~/.grd/state.toml`

## Goal

Let users store their GitHub PAT as `github_pat` in `~/.grd/state.toml` (managed via
`grd config github_pat set/unset`), used whenever `GITHUB_PAT`/`GITHUB_TOKEN` env
vars are absent.

## Background

- `src/config.rs::get_auth_token()` resolves the PAT from env only (`GITHUB_PAT` →
  `GITHUB_TOKEN` → test-only `testing.env`), validates length ≥ 20, and
  `configure_agent()` injects it as a `Bearer` header.
- `src/state.rs::State` is the settings file: `~/.grd/state.toml` with `versions` and
  `default_install_dir`, loaded/saved via `State::load()/save()`.
- `src/cli.rs::Command` has flat subcommands (`register`, `remove`, `info`, …);
  `main.rs::run_command` dispatches them.

## Decisions (confirmed by user)

1. Setting file: existing `~/.grd/state.toml`.
2. Precedence: env vars win over the file token.
3. CLI: `grd config github_pat set <token>` and `grd config github_pat unset`.
4. No special security treatment (no chmod/permission handling).
5. README documentation included.

## Approach

1. **`src/state.rs`** — add `github_pat: Option<String>` to `State` with
   `#[serde(default, skip_serializing_if = "Option::is_none")]` (old state files stay
   valid, unset key stays out of the file). Add `get_github_pat()`,
   `set_github_pat(&str)`, `clear_github_pat()`.
2. **`src/cli.rs`** — add `Config { command: ConfigCommand }` subcommand;
   `ConfigCommand::GithubPat { action: GithubPatAction }` (renamed so the literal word
   is `github_pat`); `GithubPatAction::Set { token: String } | Unset`. Yields exactly:
   `grd config github_pat set <token>` / `grd config github_pat unset`.
3. **`src/main.rs`** — `cmd_config` dispatch: `set` → `State::load()`, validate via
   existing `validate_pat_format` (reject too-short tokens at write time),
   `set_github_pat`, `save`, print confirmation **without echoing the token`;
   `unset` → clear + save, or warn "not set" (mirrors `remove`'s style).
4. **`src/config.rs`** — restructure token resolution into a pure
   `resolve_auth_token(env_pat, file_pat)` (env wins, then file, then test-only
   `testing.env` fallback); `get_auth_token()` reads env +
   `state::State::load().get_github_pat()` and feeds the winner through the existing
   `validate_pat_format` warning path. `prepare_request` in main.rs keeps calling
   `get_auth_token()` unchanged.
5. **Tests (all inside `src/`)**
   - `state.rs`: github_pat roundtrip / default-None / serialized-only-when-set /
     clear (uses existing `StatePathEnvGuard`).
   - `cli.rs`: parse tests for `set` / `unset` / bad verb.
   - `config.rs`: `resolve_auth_token` precedence unit tests (pure fn, no env
     mutation).
   - `main.rs`: `cmd_config` set → load → unset integration test via
     `GRD_STATE_PATH`.
6. **README.md** — new subsection under auth/env docs: `grd config github_pat set
   <token>` usage, precedence (`GITHUB_PAT`/`GITHUB_TOKEN` env → state file), and a
   one-line note that the token is stored in plaintext.
7. **Workflow** — `cargo check -q` → `cargo test -q` → `cargo clippy -q --fix
   --allow-dirty` → `cargo clippy -q -- -D warnings`.

## Trade-offs

- **State file vs. new `settings.toml`** — reuse `state.toml` (user's call): one
  file, one load path; rejected separate file to avoid a second config loader.
- **Token as positional arg vs. stdin prompt** — arg is simplest and scriptable; note
  it lands in shell history — acceptable per "no special treatment". stdin prompt
  (`-` for stdin) left as future work.
- **Nested `config github_pat set` vs. flat `config-set`** — nested matches the
  requested syntax and extends naturally to future settings keys.
- **Validation at `set` time** — reuses the existing ≥20-char check so garbage is
  rejected before it is persisted; keeps read path unchanged.

## Open questions

- Should `grd info` / a future bare `grd config` show whether a PAT is configured
  (masked, e.g. `github_pat: ghp_****abcd`)? Not included; revisit on request.

## Next step

Implement step 1 (`state.rs` field + accessors) and run `cargo check -q`.
