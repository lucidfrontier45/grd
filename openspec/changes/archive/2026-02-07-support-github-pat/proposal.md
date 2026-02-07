## Why

GitHub API has rate limits that significantly impact unauthenticated users (60 requests/hour vs 5000/hour with authentication). To ensure reliable operation and avoid hitting rate limits during development and production use, we need to support GitHub Personal Access Token (PAT) authentication.

## What Changes

- Add configuration to read GitHub Personal Access Token from environment variable or `.env` file
- Update HTTP client to attach PAT as Bearer token in GitHub API requests
- Update GitHub Actions workflow to use default `GITHUB_TOKEN` environment variable
- Add validation for PAT format and provide clear error messages for missing/invalid tokens

## Capabilities

### New Capabilities
- `github-pat-auth`: GitHub Personal Access Token authentication for API requests, including token loading from environment variables, .env file support, and Bearer token attachment to HTTP requests

### Modified Capabilities
- None (this is a new feature addition, not a modification to existing requirements)

## Impact

- **Configuration**: New environment variable support (e.g., `GITHUB_PAT` or `GITHUB_TOKEN`)
- **HTTP Client**: Updates to request headers to include Authorization header
- **GitHub Actions**: No workflow changes needed (uses default `GITHUB_TOKEN` provided by GitHub Actions)
- **Error Handling**: Enhanced error messages for authentication failures and rate limit issues
- **Testing**: New tests for token loading, validation, and authenticated requests