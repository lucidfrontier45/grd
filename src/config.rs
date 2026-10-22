use std::{collections::HashMap, env, path::Path};

use anyhow::{Context, Result, bail};
use ureq::{
    Agent, Body, SendBody,
    http::{Request, Response, header::HeaderValue},
    middleware::{Middleware, MiddlewareNext},
};

fn load_pat_from_env() -> Option<String> {
    env::var("GITHUB_PAT")
        .ok()
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .or_else(|| testing_env_var("GITHUB_PAT"))
        .or_else(|| testing_env_var("GITHUB_TOKEN"))
}

/// Parse a dotenv-style file and return the variables it defines.
///
/// The file is only read; the process environment is left untouched. Use
/// [`load_env_file`] to apply the variables to the environment.
pub fn read_env_file(path: &Path) -> Result<HashMap<String, String>> {
    let map = dotenv::EnvLoader::with_path(path)
        .sequence(dotenv::EnvSequence::InputOnly)
        .load()
        .with_context(|| format!("failed to load env file {}", path.display()))?;

    Ok(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

/// Load a dotenv-style file into the process environment.
///
/// Variables already present in the environment are not overridden. Nothing is
/// loaded automatically: call this with the path given by `--env-file`.
///
/// # Safety
///
/// This mutates the process environment, which is not thread-safe. Call it
/// before any other thread has been started.
pub unsafe fn load_env_file(path: &Path) -> Result<()> {
    // SAFETY: the caller guarantees that no other thread is running.
    unsafe {
        dotenv::EnvLoader::with_path(path)
            .load_and_modify()
            .with_context(|| format!("failed to load env file {}", path.display()))?;
    }
    Ok(())
}

/// Look up a variable in the optional `testing.env` file.
///
/// The test suite runs with the crate root as the working directory, so a
/// `testing.env` file placed there supplies `GITHUB_PAT`/`GITHUB_TOKEN` to the
/// tests that call the GitHub API. The file is parsed once and cached; it is
/// never merged into the process environment, so no test can observe a
/// half-applied environment.
#[cfg(test)]
fn testing_env_var(key: &str) -> Option<String> {
    static MAP: std::sync::OnceLock<dotenv::EnvMap> = std::sync::OnceLock::new();

    let map = MAP.get_or_init(|| testing_env_map(Path::new("testing.env")));

    map.get(key).cloned()
}

/// Parse an env file for the test suite; a missing file yields an empty map.
#[cfg(test)]
fn testing_env_map(path: &Path) -> dotenv::EnvMap {
    dotenv::EnvLoader::with_path(path)
        .required(false)
        .sequence(dotenv::EnvSequence::InputOnly)
        .load()
        .unwrap_or_else(|e| {
            eprintln!("Warning: failed to load {}: {e}", path.display());
            dotenv::EnvMap::default()
        })
}

/// Outside the test suite no env file is ever read.
#[cfg(not(test))]
fn testing_env_var(_key: &str) -> Option<String> {
    None
}

fn validate_pat_format(token: &str) -> Result<()> {
    if token.len() < 20 {
        bail!("GitHub PAT appears too short (expected 40+ characters)");
    }
    Ok(())
}

/// Get the GitHub Personal Access Token (PAT) from environment variables.
///
/// This function checks for the `GITHUB_PAT` environment variable first, then falls back to
/// `GITHUB_TOKEN`. No file is read automatically; use `--env-file` to populate the
/// environment from a dotenv-style file before this runs.
///
/// Returns `Some(token)` if a valid token is found, or `None` if no token is configured
/// or if the token format validation fails.
pub fn get_auth_token() -> Option<String> {
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
    fn test_read_env_file_parses_variables() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("custom.env");
        std::fs::write(&file, "GRD_TEST_A=one\nGRD_TEST_B=two\n").unwrap();

        let vars = read_env_file(&file).unwrap();
        assert_eq!(vars.get("GRD_TEST_A").map(String::as_str), Some("one"));
        assert_eq!(vars.get("GRD_TEST_B").map(String::as_str), Some("two"));
    }

    #[test]
    fn test_read_env_file_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("does-not-exist.env");
        let err = read_env_file(&file).unwrap_err();
        assert!(err.to_string().contains("failed to load env file"));
    }

    #[test]
    fn test_load_env_file_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("does-not-exist.env");
        // SAFETY: no other thread touches the environment in this test.
        let err = unsafe { load_env_file(&file) }.unwrap_err();
        assert!(err.to_string().contains("failed to load env file"));
    }

    #[test]
    fn test_testing_env_map_reads_variables() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("testing.env");
        std::fs::write(&file, "GITHUB_PAT=from_testing_env\n").unwrap();

        let map = testing_env_map(&file);
        assert_eq!(
            map.get("GITHUB_PAT").map(String::as_str),
            Some("from_testing_env")
        );
    }

    #[test]
    fn test_testing_env_map_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("missing-testing.env");

        let map = testing_env_map(&file);
        assert!(map.is_empty());
    }

    #[test]
    fn test_token_set_in_http_request() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };

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
