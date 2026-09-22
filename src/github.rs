use anyhow::{Context, Result, bail};
use serde::Deserialize;
use ureq::{Agent, Error as UreqError};

fn is_unauthorized(status: u16) -> bool {
    status == 401
}

fn is_rate_limited(status: u16) -> bool {
    status == 403
}

/// Validate that `repo` is a syntactically valid `owner/repo` identifier and
/// return its components. Rejects characters that would be unsafe to interpolate
/// directly into a URL path (whitespace, `#`, `?`, etc.).
fn validate_repo(repo: &str) -> Result<(&str, &str)> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("Repository must be in 'owner/repo' format (got '{repo}')")
    })?;

    // GitHub allows alphanumeric, '.', '_', '-' in owner and repo names.
    let is_valid_segment = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    };

    if !is_valid_segment(owner) || !is_valid_segment(name) || name.contains('/') {
        bail!(
            "Invalid repository '{repo}'. Owner and repo must each contain only \
             alphanumeric, '-', '_', or '.' characters."
        );
    }

    Ok((owner, name))
}

#[derive(Deserialize, Debug)]
pub struct Release {
    pub tag_name: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub fn list_releases(agent: &Agent, repo: &str) -> Result<Vec<Release>> {
    validate_repo(repo)?;
    let url = format!("https://api.github.com/repos/{repo}/releases");
    let mut response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(UreqError::StatusCode(status_code)) => {
            if is_unauthorized(status_code) {
                bail!(
                    "GitHub PAT authentication failed. Verify your token is valid and has 'public_repo' scope."
                );
            }
            if is_rate_limited(status_code) {
                bail!(
                    "GitHub API rate limit exceeded. Configure GITHUB_PAT environment variable for increased limits (5000/hour vs 60/hour unauthenticated)."
                );
            }
            bail!("Failed to list releases for {}: HTTP {}", repo, status_code);
        }
        Err(e) => return Err(e).context("Failed to list releases"),
    };

    response
        .body_mut()
        .read_json()
        .context("Failed to parse releases")
}

pub fn fetch_release_info(agent: &Agent, repo: &str, tag: Option<&str>) -> Result<Release> {
    validate_repo(repo)?;
    let url = match tag {
        Some(t) => format!(
            "https://api.github.com/repos/{repo}/releases/tags/{}",
            urlencoding::encode(t)
        ),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };

    let mut response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(UreqError::StatusCode(status_code)) => {
            if is_unauthorized(status_code) {
                bail!(
                    "GitHub PAT authentication failed. Verify your token is valid and has 'public_repo' scope."
                );
            }
            if is_rate_limited(status_code) {
                bail!(
                    "GitHub API rate limit exceeded. Configure GITHUB_PAT environment variable for increased limits (5000/hour vs 60/hour unauthenticated)."
                );
            }
            bail!(
                "Failed to fetch release info for {}/{}: HTTP {}",
                repo,
                tag.unwrap_or("latest"),
                status_code
            );
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "Failed to fetch release info for {}/{}",
                    repo,
                    tag.unwrap_or("latest")
                )
            });
        }
    };

    response
        .body_mut()
        .read_json()
        .context("Failed to parse release info")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{configure_agent, get_auth_token};

    #[test]
    fn test_asset_size_display() {
        let asset = Asset {
            name: "test.tar.gz".to_string(),
            browser_download_url: "https://example.com/test.tar.gz".to_string(),
            size: 1024 * 1024,
        };
        assert_eq!(asset.size, 1048576);
    }

    #[test]
    fn test_release_struct() {
        let release = Release {
            tag_name: "v1.0.0".to_string(),
            assets: vec![],
        };
        assert_eq!(release.tag_name, "v1.0.0");
        assert!(release.assets.is_empty());
    }

    #[test]
    fn test_asset_clone() {
        let asset = Asset {
            name: "test.zip".to_string(),
            browser_download_url: "https://example.com/test.zip".to_string(),
            size: 500,
        };
        let cloned = asset.clone();
        assert_eq!(asset.name, cloned.name);
        assert_eq!(asset.size, cloned.size);
    }

    #[test]
    fn test_fetch_release_from_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let token = get_auth_token();
        let agent = configure_agent(&ua, token.as_deref());

        let result = fetch_release_info(&agent, "lucidfrontier45/grd", None);
        assert!(result.is_ok());

        let release = result.unwrap();
        assert!(!release.tag_name.is_empty());
        assert!(!release.assets.is_empty());
    }

    #[test]
    fn test_list_releases_from_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let token = get_auth_token();
        let agent = configure_agent(&ua, token.as_deref());

        let result = list_releases(&agent, "lucidfrontier45/grd");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_repo_valid() {
        assert!(validate_repo("owner/repo").is_ok());
        assert!(validate_repo("lucidfrontier45/grd").is_ok());
        assert!(validate_repo("a.b-c_d/x.y-z_w").is_ok());
        // Single-char segments are allowed by GitHub.
        assert!(validate_repo("a/b").is_ok());
    }

    #[test]
    fn test_validate_repo_missing_slash() {
        assert!(validate_repo("owneronly").is_err());
        assert!(validate_repo("").is_err());
    }

    #[test]
    fn test_validate_repo_empty_component() {
        assert!(validate_repo("/repo").is_err());
        assert!(validate_repo("owner/").is_err());
    }

    #[test]
    fn test_validate_repo_too_many_slashes() {
        // Only the first slash splits; remaining slashes must not appear in name.
        assert!(validate_repo("owner/repo/extra").is_err());
    }

    #[test]
    fn test_validate_repo_unsafe_characters() {
        assert!(validate_repo("own er/repo").is_err());
        assert!(validate_repo("owner/re po").is_err());
        assert!(validate_repo("owner/re#po").is_err());
        assert!(validate_repo("owner/re?po").is_err());
        assert!(validate_repo("owner/re%20po").is_err());
        assert!(validate_repo("owner/repo/../etc").is_err());
    }

    #[test]
    fn test_fetch_release_info_rejects_invalid_repo_before_network() {
        // Should fail with a validation error, not a network error.
        let ua = "test-agent";
        let agent = configure_agent(ua, None);
        let result = fetch_release_info(&agent, "invalid repo", None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Invalid repository") || msg.contains("'owner/repo' format"),
            "expected validation error, got: {msg}"
        );
    }
}
