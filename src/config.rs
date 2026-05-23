use std::env;

use anyhow::{Result, bail};
use ureq::{
    Agent, Body, SendBody,
    http::{Request, Response, header::HeaderValue},
    middleware::{Middleware, MiddlewareNext},
};

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
pub fn get_auth_token() -> Option<String> {
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
/// If a token is provided, it will be automatically added to all requests through middleware.
/// If token is None, no authentication header will be sent.
///
/// # Arguments
///
/// * `user_agent` - The user agent string to identify this client to servers
/// * `token` - An optional authentication token string. If None, no auth header is sent
///
/// # Returns
///
/// A configured `ureq::Agent` instance ready to make HTTP requests
pub fn configure_agent(user_agent: &str, token: Option<&str>) -> Agent {
    let auth_header = token.map(|t| format!("Bearer {t}"));

    let mut builder = ureq::Agent::config_builder().user_agent(user_agent);

    if auth_header.is_some() {
        builder = builder.middleware(AuthMiddleware { auth_header });
    }

    builder.build().into()
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
    use std::collections::HashMap;

    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct HeadersResponse {
        headers: HashMap<String, String>,
    }

    #[test]
    fn test_token_set_in_http_request() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            let _n = stream.read(&mut buf).unwrap();
            let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"headers\":{\"Authorization\":\"Bearer test_token\"}}";
            stream.write_all(response).unwrap();
        });

        let token = "test_token";
        let agent = configure_agent("test-agent", Some(token));

        let mut resp = agent
            .get(&format!("http://127.0.0.1:{port}/headers"))
            .call()
            .unwrap();

        let recieved_token: HeadersResponse = resp.body_mut().read_json().unwrap();
        let auth_header = recieved_token
            .headers
            .get("Authorization")
            .cloned()
            .unwrap();
        assert_eq!(auth_header, format!("Bearer {}", token));

        server.join().unwrap();
    }
}
