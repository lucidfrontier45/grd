use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use ureq::{Agent, Error as UreqError};

fn is_unauthorized(status: u16) -> bool {
    status == 401
}

fn is_rate_limited(status: u16) -> bool {
    status == 403
}

/// GitHub allows alphanumeric, '.', '_', '-' in owner and repo names.
fn is_valid_segment(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(is_valid_char)
}

fn is_valid_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
}

/// Validate that `repo` is a syntactically valid `owner/repo` identifier and
/// return its components. Rejects characters that would be unsafe to interpolate
/// directly into a URL path (whitespace, `#`, `?`, etc.).
fn validate_repo(repo: &str) -> Result<(&str, &str)> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| {
        anyhow::anyhow!("Repository must be in 'owner/repo' format (got '{repo}')")
    })?;

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
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

pub fn list_releases(agent: &Agent, repo: &str) -> Result<Vec<Release>> {
    validate_repo(repo)?;
    let endpoint = format!("https://api.github.com/repos/{repo}/releases?");
    let first_url = format!("{endpoint}per_page=100&page=1");
    collect_release_pages(Some(first_url), |url| {
        let mut response = match agent.get(url).call() {
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
        let next_url = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .map(|link| next_page_url(link, &endpoint))
            .transpose()?
            .flatten();
        let releases: Vec<Release> = response
            .body_mut()
            .read_json()
            .context("Failed to parse releases")?;
        Ok((releases, next_url))
    })
}

fn collect_release_pages<F>(mut next_url: Option<String>, mut fetch_page: F) -> Result<Vec<Release>>
where
    F: FnMut(&str) -> Result<(Vec<Release>, Option<String>)>,
{
    let mut visited = HashSet::new();
    let mut releases = Vec::new();

    while let Some(url) = next_url {
        if !visited.insert(url.clone()) {
            bail!("GitHub returned a repeated releases pagination link");
        }
        let (mut page, following_url) = fetch_page(&url)?;
        releases.append(&mut page);
        next_url = following_url;
    }

    Ok(releases)
}

fn next_page_url(link_header: &str, endpoint: &str) -> Result<Option<String>> {
    let Some(next_link) = link_header
        .split(',')
        .find(|link| link.contains("rel=\"next\""))
    else {
        return Ok(None);
    };
    let Some(url) = next_link.trim().split(';').next().and_then(|part| {
        part.trim()
            .strip_prefix('<')
            .and_then(|part| part.strip_suffix('>'))
    }) else {
        bail!("GitHub returned a malformed releases pagination link");
    };
    if !url.starts_with(endpoint) {
        bail!("GitHub returned an unexpected releases pagination URL");
    }
    Ok(Some(url.to_string()))
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

/// Fetch the newest release in the same tag family as an installed version.
/// GitHub's `/releases/latest` is owner-controlled and may point at another
/// version stream, so upgrades with a cached tag inspect the release list.
pub fn fetch_upgrade_release(
    agent: &Agent,
    repo: &str,
    cached_tag: Option<&str>,
) -> Result<Release> {
    let Some(cached_tag) = cached_tag else {
        return fetch_release_info(agent, repo, None);
    };

    let Some(cached_version) = parse_tag_version(cached_tag) else {
        return fetch_release_info(agent, repo, None);
    };
    let releases = list_releases(agent, repo)?;
    let selected = select_upgrade_release(releases, &cached_version);
    match selected {
        Some(release) => Ok(release),
        None => bail!(
            "No stable releases found for {repo} in the installed tag family '{}'; refusing to select an unrelated release",
            cached_version.family
        ),
    }
}

fn select_upgrade_release(releases: Vec<Release>, cached_version: &TagVersion) -> Option<Release> {
    releases
        .into_iter()
        .filter_map(|release| {
            if release.prerelease || release.draft {
                return None;
            }
            let version = parse_tag_version(&release.tag_name)?;
            (version.family == cached_version.family).then_some((version, release))
        })
        .max_by_key(|(version, _)| version.parts)
        .map(|(_, release)| release)
}

/// Whether `target` is an older version in the same tag family as `installed`.
pub fn is_downgrade(target: &str, installed: &str) -> bool {
    match (parse_tag_version(target), parse_tag_version(installed)) {
        (Some(target), Some(installed)) => {
            target.stream == installed.stream && target.parts < installed.parts
        }
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TagVersion {
    stream: String,
    family: String,
    parts: [u64; 3],
}

fn parse_tag_version(tag: &str) -> Option<TagVersion> {
    let version_start = tag.find(|character: char| character.is_ascii_digit())?;
    let (prefix, version) = tag.split_at(version_start);
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(TagVersion {
        stream: prefix.to_string(),
        family: format!("{prefix}{major}"),
        parts: [major, minor, patch],
    })
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
            prerelease: false,
            draft: false,
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
    fn test_tag_versions_compare_only_within_same_family() {
        assert!(is_downgrade("v1.19.5", "v2.2.7"));
        assert!(is_downgrade("v2.2.6", "v2.2.7"));
        assert!(!is_downgrade("v2.2.8", "v2.2.7"));
        assert!(!is_downgrade("process-daemon-v0.5.2", "v2.2.7"));
    }

    #[test]
    fn test_parse_tag_version_retains_tag_family() {
        assert_eq!(parse_tag_version("v2.2.7").unwrap().family, "v2");
        assert_eq!(
            parse_tag_version("process-daemon-v0.5.2").unwrap().family,
            "process-daemon-v0"
        );
        assert!(parse_tag_version("release-candidate").is_none());
    }

    #[test]
    fn test_next_page_url_follows_github_next_link() {
        let endpoint = "https://api.github.com/repos/owner/repo/releases?";
        let link = "<https://api.github.com/repos/owner/repo/releases?per_page=100&page=2>; rel=\"next\", <https://api.github.com/repos/owner/repo/releases?per_page=100&page=4>; rel=\"last\"";
        assert_eq!(
            next_page_url(link, endpoint).unwrap().as_deref(),
            Some("https://api.github.com/repos/owner/repo/releases?per_page=100&page=2")
        );
        assert_eq!(next_page_url("", endpoint).unwrap(), None);
        assert!(next_page_url("<https://example.com/evil>; rel=\"next\"", endpoint).is_err());
    }

    #[test]
    fn test_upgrade_selection_uses_later_page_and_skips_prereleases_and_drafts() {
        let cached_version = parse_tag_version("v2.2.7").unwrap();
        let page_one_url = "https://api.github.com/repos/owner/repo/releases?page=1";
        let page_two_url = "https://api.github.com/repos/owner/repo/releases?page=2";
        let releases = collect_release_pages(Some(page_one_url.to_string()), |url| {
            if url == page_one_url {
                Ok((
                    vec![test_release("v2.2.8", false, false)],
                    Some(page_two_url.to_string()),
                ))
            } else {
                assert_eq!(url, page_two_url);
                Ok((
                    vec![
                        test_release("v2.2.9", false, false),
                        test_release("v2.3.0-rc.1", true, false),
                        test_release("v2.4.0", false, true),
                    ],
                    None,
                ))
            }
        })
        .unwrap();
        let selected = select_upgrade_release(releases, &cached_version).unwrap();
        assert_eq!(selected.tag_name, "v2.2.9");
    }

    fn test_release(tag: &str, prerelease: bool, draft: bool) -> Release {
        Release {
            tag_name: tag.to_string(),
            assets: vec![],
            prerelease,
            draft,
        }
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
        assert!(validate_repo("../repo").is_err());
        assert!(validate_repo("owner/..").is_err());
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
