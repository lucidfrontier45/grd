# Implementation Tasks

## 1. Setup and Dependencies

- [ ] 1.1 Add `dotenv = "0.15"` dependency to `Cargo.toml`
- [ ] 1.2 Run `cargo check` to verify dependency resolves correctly

## 2. Configuration Module

- [ ] 2.1 Create `src/config.rs` module with PAT loading logic
- [ ] 2.2 Implement `load_pat_from_env()` function checking `GITHUB_PAT` then `GITHUB_TOKEN`
- [ ] 2.3 Implement `load_dotenv()` function to load `.env` file if present
- [ ] 2.4 Implement `validate_pat_format()` function for basic token validation (length check)
- [ ] 2.5 Implement public `get_pat()` function that combines env var and .env loading with precedence
- [ ] 2.6 Add `pub mod config;` to `src/main.rs`
- [ ] 2.7 Write unit tests for PAT loading precedence logic in `src/config.rs`

## 3. Agent Configuration

- [ ] 3.1 Create `configure_agent()` helper function in `src/config.rs` that builds ureq Agent with optional PAT
- [ ] 3.2 Update `src/main.rs` to use `configure_agent()` instead of direct Agent creation
- [ ] 3.3 Add conditional logic to inject `Authorization: Bearer <token>` header when PAT is present
- [ ] 3.4 Add warning log when PAT format validation fails but allow continuation
- [ ] 3.5 Update integration tests in `src/github.rs` to use `configure_agent()`
- [ ] 3.6 Update integration tests in `src/tests.rs` to use `configure_agent()`

## 4. Error Handling

- [ ] 4.1 Create error detection function for HTTP 401 (Unauthorized) responses
- [ ] 4.2 Create error detection function for HTTP 403 (Rate Limit) responses
- [ ] 4.3 Add helpful error message for 401: "GitHub PAT authentication failed. Verify your token is valid and has 'public_repo' scope."
- [ ] 4.4 Add helpful error message for 403 rate limit: "GitHub API rate limit exceeded. Configure GITHUB_PAT environment variable for increased limits (5000/hour vs 60/hour unauthenticated)."
- [ ] 4.5 Update error handling in `fetch_release_info()` to detect and format authentication errors
- [ ] 4.6 Update error handling in `list_releases()` to detect and format authentication errors

## 5. Testing

- [ ] 5.1 Add unit test for `load_pat_from_env()` with only `GITHUB_PAT` set
- [ ] 5.2 Add unit test for `load_pat_from_env()` with only `GITHUB_TOKEN` set
- [ ] 5.3 Add unit test for `load_pat_from_env()` with both set (verify precedence)
- [ ] 5.4 Add unit test for `load_pat_from_env()` with neither set (returns None)
- [ ] 5.5 Add unit test for `validate_pat_format()` with valid token
- [ ] 5.6 Add unit test for `validate_pat_format()` with invalid short token
- [ ] 5.7 Add unit test for .env file loading when file exists
- [ ] 5.8 Add unit test for .env file loading when file doesn't exist (no error)
- [ ] 5.9 Add integration test for authenticated API call (marked `#[ignore]`, requires PAT)
- [ ] 5.10 Add integration test for unauthenticated API call still working
- [ ] 5.11 Run `cargo test` to verify all tests pass
- [ ] 5.12 Run `cargo clippy -- -D warnings --fix` to check code quality
- [ ] 5.13 Remove `#[ignore]` attribute from integration tests that require GitHub API access (now that PAT is available in CI/CD)

## 6. Documentation

- [ ] 6.1 Add documentation comments to `src/config.rs` public functions
- [ ] 6.2 Update README.md with PAT configuration instructions
- [ ] 6.3 Add example `.env` file to repository root (in `.gitignore`)
- [ ] 6.4 Add `.env.example` file with GITHUB_PAT placeholder and comments

## 7. GitHub Actions Integration

- [ ] 7.1 Check existing GitHub Actions workflows in `.github/workflows/`
- [ ] 7.2 Add `env: GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` to workflow steps that run grd
- [ ] 7.3 Verify the `GITHUB_TOKEN` secret is properly mapped to the `GITHUB_TOKEN` environment variable
- [ ] 7.4 Test CI/CD pipeline to ensure `GITHUB_TOKEN` environment variable is set and used for authentication

## 8. Verification and Cleanup

- [ ] 8.1 Run `cargo check` to ensure compilation succeeds
- [ ] 8.2 Run `cargo test` to verify all tests pass
- [ ] 8.3 Run `cargo clippy -- -D warnings` to ensure no warnings
- [ ] 8.4 Test manual workflow with valid PAT (verify authenticated requests work)
- [ ] 8.5 Test manual workflow without PAT (verify unauthenticated mode still works)
- [ ] 8.6 Test with invalid PAT (verify error messages are helpful)
- [ ] 8.7 Remove any temporary debugging code or comments