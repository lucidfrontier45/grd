# Implementation Tasks

## 1. Setup and Dependencies

- [x] 1.1 Add `dotenv = "0.15"` dependency to `Cargo.toml`
- [x] 1.2 Run `cargo check` to verify dependency resolves correctly

## 2. Configuration Module

- [x] 2.1 Create `src/config.rs` module with PAT loading logic
- [x] 2.2 Implement `load_pat_from_env()` function checking `GITHUB_PAT` then `GITHUB_TOKEN`
- [x] 2.3 Implement `load_dotenv()` function to load `.env` file if present
- [x] 2.4 Implement `validate_pat_format()` function for basic token validation (length check)
- [x] 2.5 Implement public `get_pat()` function that combines env var and .env loading with precedence
- [x] 2.6 Add `pub mod config;` to `src/main.rs`
- [x] 2.7 Write unit tests for PAT loading precedence logic in `src/config.rs`

## 3. Agent Configuration

- [x] 3.1 Create `configure_agent()` helper function in `src/config.rs` that builds ureq Agent with optional PAT
- [x] 3.2 Update `src/main.rs` to use `configure_agent()` instead of direct Agent creation
- [x] 3.3 Add conditional logic to inject `Authorization: Bearer <token>` header when PAT is present
- [x] 3.4 Add warning log when PAT format validation fails but allow continuation
- [x] 3.5 Update integration tests in `src/github.rs` to use `configure_agent()`
- [x] 3.6 Update integration tests in `src/tests.rs` to use `configure_agent()`

## 4. Error Handling

- [x] 4.1 Create error detection function for HTTP 401 (Unauthorized) responses
- [x] 4.2 Create error detection function for HTTP 403 (Rate Limit) responses
- [x] 4.3 Add helpful error message for 401: "GitHub PAT authentication failed. Verify your token is valid and has 'public_repo' scope."
- [x] 4.4 Add helpful error message for 403 rate limit: "GitHub API rate limit exceeded. Configure GITHUB_PAT environment variable for increased limits (5000/hour vs 60/hour unauthenticated)."
- [x] 4.5 Update error handling in `fetch_release_info()` to detect and format authentication errors
- [x] 4.6 Update error handling in `list_releases()` to detect and format authentication errors

## 5. Testing

- [x] 5.1 Add test verifying configured PAT is correctly set in HTTP request Authorization header

## 6. Documentation

- [x] 6.1 Add documentation comments to `src/config.rs` public functions
- [x] 6.2 Update README.md with PAT configuration instructions
- [x] 6.3 Add example `.env` file to repository root (in `.gitignore`)
- [x] 6.4 Add `.env.example` file with GITHUB_PAT placeholder and comments

## 7. GitHub Actions Integration

- [x] 7.1 Check existing GitHub Actions workflows in `.github/workflows/`
- [x] 7.2 Add `env: GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` to workflow steps that run grd
- [x] 7.3 Verify the `GITHUB_TOKEN` secret is properly mapped to the `GITHUB_TOKEN` environment variable
- [x] 7.4 Test CI/CD pipeline to ensure `GITHUB_TOKEN` environment variable is set and used for authentication

## 8. Verification and Cleanup

- [x] 8.1 Run `cargo check` to ensure compilation succeeds
- [x] 8.2 Run `cargo test` to verify all tests pass
- [x] 8.3 Run `cargo clippy -- -D warnings` to ensure no warnings
- [x] 8.4 Test manual workflow with valid PAT (verify authenticated requests work)
- [x] 8.5 Test manual workflow without PAT (verify unauthenticated mode still works)
- [x] 8.6 Test with invalid PAT (verify error messages are helpful)
- [x] 8.7 Remove any temporary debugging code or comments