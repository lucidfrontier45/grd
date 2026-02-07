use anyhow::{bail, Result};
use std::env;
use ureq::http::{header::HeaderValue, Request, Response};
use ureq::middleware::Middleware;
use ureq::middleware::MiddlewareNext;
use ureq::Agent;
use ureq::{Body, SendBody};

fn load_pat_from_env() -> Option<String> {
    env::var("GITHUB_PAT")
        .ok()
        .or_else(|| env::var("GITHUB_TOKEN").ok())
}

fn load_dotenv() {
    let _ = dotenv::dotenv();
}

fn validate_pat_format(token: &str) -> Result<()> {
    if token.len() < 20 {
        bail!("GitHub PAT appears too short (expected 40+ characters)");
    }
    Ok(())
}

/// Get the GitHub Personal Access Token (PAT) from environment variables or .env file.
///
/// This function checks for the `GITHUB_PAT` environment variable first, then falls back to
/// `GITHUB_TOKEN`. If neither is set in the environment, it attempts to load them from a
/// `.env` file in the current directory.
///
/// Returns `Some(token)` if a valid token is found, or `None` if no token is configured
/// or if the token format validation fails.
pub fn get_pat() -> Option<String> {
    load_dotenv();

    if let Some(token) = load_pat_from_env() {
        if let Err(e) = validate_pat_format(&token) {
            eprintln!("Warning: {}", e);
            eprintln!("Proceeding without authentication");
            return None;
        }
        Some(token)
    } else {
        None
    }
}

/// Create a configured ureq Agent with the specified user agent and optional PAT authentication.
///
/// This function creates an HTTP client (Agent) configured with the provided user agent string.
/// If a GitHub PAT is available via environment variables or .env file, it automatically adds
/// the Authorization header to all requests through middleware.
///
/// # Arguments
///
/// * `user_agent` - The user agent string to identify this client to servers
///
/// # Returns
///
/// A configured `ureq::Agent` instance ready to make HTTP requests, with automatic PAT authentication
/// if configured
pub fn configure_agent(user_agent: &str) -> Agent {
    let auth_header = get_auth_header();

    let mut builder = ureq::Agent::config_builder().user_agent(user_agent);

    if auth_header.is_some() {
        builder = builder.middleware(AuthMiddleware { auth_header });
    }

    builder.build().into()
}

/// Get the Authorization header value for GitHub API requests.
///
/// This function retrieves the PAT using `get_pat()` and formats it as a Bearer token
/// suitable for use in the `Authorization` header. Returns `None` if no PAT is configured.
///
/// # Returns
///
/// * `Some("Bearer <token>")` if a PAT is available
/// * `None` if no PAT is configured
pub fn get_auth_header() -> Option<String> {
    get_pat().map(|token| format!("Bearer {token}"))
}

struct AuthMiddleware {
    auth_header: Option<String>,
}

impl Middleware for AuthMiddleware {
    fn handle(
        &self,
        mut request: Request<SendBody>,
        next: MiddlewareNext,
    ) -> Result<Response<Body>, ureq::Error> {
        if let Some(ref header) = self.auth_header {
            let headers = request.headers_mut();
            headers.insert("Authorization", HeaderValue::from_str(header).unwrap());
        }
        next.handle(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_token_set_in_http_request() {
        let original_pat = env::var("GITHUB_PAT").ok();

        unsafe {
            env::set_var("GITHUB_PAT", "ghp_test_token_123456789012345678901234567");
        }

        let received_header = Arc::new(Mutex::new(None::<String>));

        let agent = configure_agent("test-agent");

        let received_header_clone = Arc::clone(&received_header);
        let _guard = agent
            .get("https://httpbin.org/headers")
            .call()
            .ok()
            .and_then(|mut resp| {
                let body = resp.body_mut().read_to_string().ok()?;
                let json: serde_json::Value = serde_json::from_str(&body).ok()?;
                let auth = json["headers"]["Authorization"].as_str()?;
                let _ = received_header_clone
                    .lock()
                    .unwrap()
                    .insert(auth.to_string());
                Some(())
            });

        let header = received_header.lock().unwrap();
        assert_eq!(
            header.as_ref().unwrap(),
            "Bearer ghp_test_token_123456789012345678901234567"
        );

        drop(header);

        match original_pat {
            Some(val) => unsafe { env::set_var("GITHUB_PAT", val) },
            None => unsafe { env::remove_var("GITHUB_PAT") },
        }
    }
}
