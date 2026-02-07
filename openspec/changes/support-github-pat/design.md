## Context

GRD is a GitHub release downloader that interacts with the GitHub API to fetch release information and download assets. Currently, all API requests are made without authentication, which limits the rate limit to 60 requests/hour for unauthenticated users compared to 5000 requests/hour for authenticated requests.

The application uses:
- **ureq 3.1.4**: Minimal synchronous HTTP client for API requests
- **Existing Agent creation**: Currently created in `src/main.rs` with only user-agent configuration
- **No external secrets management**: All configuration currently comes from CLI arguments

The codebase has 3 locations where `Agent` is instantiated:
1. `src/main.rs:22` - Main application entry point
2. `src/github.rs:91, 105` - Integration tests
3. `src/tests.rs:15, 32` - Additional integration tests

## Goals / Non-Goals

**Goals:**
- Enable PAT authentication to increase GitHub API rate limits from 60 to 5000 requests/hour
- Support flexible PAT configuration via environment variables and .env files
- Maintain backward compatibility (unauthenticated mode should still work)
- Provide clear error messages for authentication failures
- Minimal code changes and dependency additions

**Non-Goals:**
- OAuth flow or interactive authentication
- PAT generation or management UI
- Token caching or persistence beyond .env file
- Support for other GitHub authentication methods (JWT, app authentication)

## Decisions

### 1. Use existing `dotenv` crate for .env file support
**Decision**: Add `dotenv = "0.15"` dependency to read `.env` files

**Rationale**: 
- Lightweight, well-maintained crate (1M+ downloads)
- Single-purpose: only reads .env files into environment
- No additional dependencies pulled in
- Industry standard for Rust .env handling

**Alternatives considered**:
- Manual .env parsing: Would require more code and edge-case handling
- `dotenvy`: Newer fork, but `dotenv` is more stable and widely-used

### 2. Environment variable precedence: GITHUB_PAT > GITHUB_TOKEN
**Decision**: Check `GITHUB_PAT` first, then `GITHUB_TOKEN`, with environment variables taking precedence over .env file

**Rationale**:
- `GITHUB_PAT` is more specific and avoids confusion with GitHub's official `GITHUB_TOKEN` used in Actions
- Following 12-factor app principles: environment variables override config files
- Matches common tooling patterns (e.g., `gh` CLI uses `GH_TOKEN`)

**Alternatives considered**:
- Only support `GITHUB_TOKEN`: Too ambiguous with GitHub Actions context
- Only support `GITHUB_PAT`: Less discoverable for users familiar with GitHub conventions

### 3. Configure PAT at Agent creation time
**Decision**: Modify `Agent::config_builder()` to add `Authorization` header when PAT is available

**Rationale**:
- ureq 3.x supports middleware-style configuration via `config_builder()`
- Header is applied to all requests automatically
- No changes needed to individual API call sites (`fetch_release_info`, `list_releases`)

**Implementation**:
```rust
let mut builder = Agent::config_builder().user_agent(&ua);

if let Some(token) = get_pat() {
    builder = builder.header("Authorization", &format!("Bearer {token}"));
}

let agent = builder.build().into();
```

**Alternatives considered**:
- Per-request header injection: Would require modifying every API function
- Custom ureq middleware: More complex, not needed for simple header injection

### 4. PAT validation: Basic format check + GitHub API response handling
**Decision**: Validate PAT format (non-empty, reasonable length) and provide helpful error messages for 401/403 responses

**Rationale**:
- GitHub PATs are typically 40+ characters, start with `ghp_`, `github_pat_`, or are classic tokens
- Pre-validation catches obvious errors before API call
- API response handling provides actionable feedback for authentication failures

**Implementation**:
```rust
fn validate_pat(token: &str) -> Result<()> {
    if token.len() < 20 {
        bail!("GitHub PAT appears too short (expected 40+ characters)");
    }
    Ok(())
}
```

### 5. Error messages: Contextual and actionable
**Decision**: Detect rate limit (403) and bad credentials (401) errors, provide specific guidance

**Rationale**:
- Users need to know why requests fail and how to fix it
- Unauthenticated rate limiting is the primary pain point this change addresses

**Example messages**:
- 403 + rate limit: "GitHub API rate limit exceeded. Configure GITHUB_PAT environment variable for increased limits."
- 401: "GitHub PAT authentication failed. Verify your token is valid and has 'public_repo' scope."

## Risks / Trade-offs

### Risk 1: Token exposure in logs or error messages
**Mitigation**: Never log full PAT value, only log whether authentication is enabled

### Risk 2: Breaking existing workflows that rely on unauthenticated mode
**Mitigation**: Unauthenticated mode remains fully functional; PAT is always optional

### Risk 3: Token scope insufficient for private repos
**Mitigation**: Documentation will specify required scopes (`public_repo` for public repos, `repo` for private)

### Risk 4: Test failures when no PAT is configured
**Mitigation**: Integration tests already use `#[ignore]` attribute; will update comments to clarify PAT requirements

## Migration Plan

### Phase 1: Implementation
1. Add `dotenv` dependency to `Cargo.toml`
2. Create `src/config.rs` module for PAT loading logic
3. Update `src/main.rs` to load .env file and configure Agent with PAT
4. Update test files to optionally use PAT if available

### Phase 2: Testing

#### Integration Tests with Online Echo Servers
Use public HTTP echo services to verify Authorization header is correctly set:

1. **Primary test endpoint**: `https://postman-echo.com/{method}`
   - GET/POST requests with configured PAT
   - Verify `Authorization: Bearer <token>` header appears in response
   - Test with random token to verify header transmission

2. **Fallback endpoint**: `https://httpbin.org/{method}`
   - Use if postman-echo.com is unavailable
   - Same verification: check Authorization header in response
   - Try another endpoint if primary fails

**Test Strategy**:
- Randomly select from available echo servers
- If request fails, retry with next available server
- Verify token format in echoed headers
- Test both authenticated and unauthenticated modes
- Ensure token doesn't leak into logs (only check presence)

#### GitHub API Tests
1. Test authenticated API calls with valid PAT (actual GitHub API)
2. Test error handling with invalid PAT (401/403 responses)
3. Verify unauthenticated mode still works
4. Verify rate limit improvement with authentication

### Phase 3: GitHub Actions Integration
1. Add `GITHUB_PAT` secret to repository settings
2. Update `.github/workflows/*.yml` to inject PAT from secrets
3. Test CI/CD pipeline with authentication

### Rollback Strategy
- No database changes or persistent state changes
- Revert commit to remove PAT support
- Unauthenticated mode remains available as fallback

## Open Questions

1. **Should we support PAT from CLI argument?**
   - **Current decision**: No, environment variables are more secure (don't appear in process list)
   - **Revisit**: If users request it in GitHub issues

2. **Should we cache PAT to reduce file reads?**
   - **Current decision**: No, .env is read once at startup, caching adds complexity
   - **Revisit**: If performance profiling shows .env parsing is a bottleneck

3. **What minimum Rust version should we require for `dotenv`?**
   - **Current decision**: dotenv 0.15 supports Rust 1.70+, aligns with Edition 2024
   - **Revisit**: If dependency conflicts arise