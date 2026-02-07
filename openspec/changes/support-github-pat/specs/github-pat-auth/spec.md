# GitHub PAT Authentication Specification

## ADDED Requirements

### Requirement: Load PAT from environment variables
The system SHALL read the GitHub Personal Access Token from environment variables, supporting both `GITHUB_PAT` and `GITHUB_TOKEN` variable names with `GITHUB_PAT` taking precedence.

#### Scenario: PAT from GITHUB_PAT environment variable
- **WHEN** `GITHUB_PAT` environment variable is set
- **THEN** system SHALL use the value from `GITHUB_PAT` as the authentication token

#### Scenario: PAT from GITHUB_TOKEN as fallback
- **WHEN** `GITHUB_PAT` is not set but `GITHUB_TOKEN` is set
- **THEN** system SHALL use the value from `GITHUB_TOKEN` as the authentication token

#### Scenario: No PAT configured
- **WHEN** neither `GITHUB_PAT` nor `GITHUB_TOKEN` environment variables are set
- **THEN** system SHALL continue without authentication (unauthenticated mode)

### Requirement: Load PAT from .env file
The system SHALL support reading GitHub Personal Access Token from a `.env` file in the current working directory, checking for `GITHUB_PAT` or `GITHUB_TOKEN` keys.

#### Scenario: PAT loaded from .env file
- **WHEN** `.env` file exists in current directory and contains `GITHUB_PAT` or `GITHUB_TOKEN`
- **THEN** system SHALL load and use the token value from the file

#### Scenario: Environment variables take precedence over .env
- **WHEN** both environment variables and `.env` file contain PAT values
- **THEN** system SHALL prioritize environment variable values over `.env` file values

#### Scenario: .env file not found
- **WHEN** `.env` file does not exist in current directory
- **THEN** system SHALL continue normally without error

### Requirement: Attach PAT to GitHub API requests
The system SHALL attach the loaded Personal Access Token as a Bearer token in the Authorization header for all GitHub API HTTP requests.

#### Scenario: Authenticated request with valid PAT
- **WHEN** a valid PAT is configured
- **THEN** system SHALL include `Authorization: Bearer <token>` header in all GitHub API requests

#### Scenario: Request without PAT
- **WHEN** no PAT is configured
- **THEN** system SHALL make requests without Authorization header (unauthenticated)

#### Scenario: PAT format validation
- **WHEN** PAT value does not match expected format (e.g., empty string, invalid characters)
- **THEN** system SHALL log a warning and proceed without authentication

### Requirement: Provide clear error messages for authentication failures
The system SHALL detect and report authentication-related errors from GitHub API, including rate limit errors and invalid token errors.

#### Scenario: Rate limit error with unauthenticated request
- **WHEN** GitHub API returns 403 status with rate limit exceeded message
- **THEN** system SHALL display helpful message suggesting PAT configuration

#### Scenario: Invalid token error
- **WHEN** GitHub API returns 401 status (Unauthorized)
- **THEN** system SHALL display error message indicating invalid or expired PAT

#### Scenario: Successful authenticated request
- **WHEN** PAT is valid and request succeeds
- **THEN** system SHALL proceed normally without authentication-related warnings

## MODIFIED Requirements

### Requirement: GitHub API client configuration
The existing `Agent` configuration in `src/github.rs` SHALL be modified to support optional PAT authentication while maintaining backward compatibility with unauthenticated requests.

#### Scenario: Agent creation with PAT
- **WHEN** creating ureq Agent with PAT configured
- **THEN** Agent SHALL be configured to include Authorization header in all requests

#### Scenario: Agent creation without PAT
- **WHEN** creating ureq Agent without PAT
- **THEN** Agent SHALL be created without Authorization header (current behavior)