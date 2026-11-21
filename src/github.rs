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

/// Every release of the repository, newest first, following GitHub's
/// pagination links to the end. This is the full walk `--list` needs;
/// version resolution uses the bounded [`scan_recent_releases`] instead.
pub fn list_releases(agent: &Agent, repo: &str) -> Result<Vec<Release>> {
    let endpoint = releases_endpoint(repo)?;
    collect_release_pages(
        Some(format!("{endpoint}per_page=100&page=1")),
        None,
        |url| fetch_releases_page(agent, repo, &endpoint, url),
    )
}

/// Walk the release list newest-first only until `scan_limit` stable
/// candidates ([`is_stable_candidate`]) have been seen, then stop. Every
/// release visited is returned — prereleases included — because the
/// selectors filter on their own; the limit only bounds how deep the walk
/// goes, so a repository that floods its list with prereleases (nightly
/// builds) cannot force a full scan.
fn scan_recent_releases(agent: &Agent, repo: &str, scan_limit: usize) -> Result<Vec<Release>> {
    let endpoint = releases_endpoint(repo)?;
    collect_release_pages(
        Some(format!("{endpoint}per_page=100&page=1")),
        Some(scan_limit),
        |url| fetch_releases_page(agent, repo, &endpoint, url),
    )
}

/// The releases endpoint URL (trailing `?` included) after validating `repo`.
fn releases_endpoint(repo: &str) -> Result<String> {
    validate_repo(repo)?;
    Ok(format!("https://api.github.com/repos/{repo}/releases?"))
}

/// Fetch one page of the release list plus the URL of the next page, if any.
fn fetch_releases_page(
    agent: &Agent,
    repo: &str,
    endpoint: &str,
    url: &str,
) -> Result<(Vec<Release>, Option<String>)> {
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
        .map(|link| next_page_url(link, endpoint))
        .transpose()?
        .flatten();
    let releases: Vec<Release> = response
        .body_mut()
        .read_json()
        .context("Failed to parse releases")?;
    Ok((releases, next_url))
}

/// Follow pagination links, collecting pages. With `stable_budget`, stop
/// as soon as that many stable candidates have been seen — the list is
/// newest-first, so the walk stays bounded without losing the front of the
/// release history.
fn collect_release_pages<F>(
    mut next_url: Option<String>,
    stable_budget: Option<usize>,
    mut fetch_page: F,
) -> Result<Vec<Release>>
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
        if let Some(budget) = stable_budget
            && count_stable_candidates(&releases) >= budget
        {
            break;
        }
        next_url = following_url;
    }

    Ok(releases)
}

/// A release the version selectors may pick: published (not a draft, not a
/// prerelease) and carrying a parseable `X.Y.Z` tag ([`parse_tag_version`]).
fn is_stable_candidate(release: &Release) -> bool {
    !release.prerelease && !release.draft && parse_tag_version(&release.tag_name).is_some()
}

fn count_stable_candidates(releases: &[Release]) -> usize {
    releases.iter().filter(|r| is_stable_candidate(r)).count()
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
    if !is_trusted_releases_page_url(url, endpoint) {
        bail!("GitHub returned an unexpected releases pagination URL");
    }
    Ok(Some(url.to_string()))
}

fn is_trusted_releases_page_url(url: &str, endpoint: &str) -> bool {
    if url.starts_with(endpoint) {
        return true;
    }

    let Some(repository_path) = url.strip_prefix("https://api.github.com/repositories/") else {
        return false;
    };
    let Some((repository_id, query)) = repository_path.split_once("/releases?") else {
        return false;
    };
    !repository_id.is_empty()
        && repository_id.bytes().all(|byte| byte.is_ascii_digit())
        && !query.is_empty()
}

/// The release the repository owner marked as "Latest".
///
/// GitHub computes nothing for this label: the owner points it anywhere, so
/// it may reference a prerelease or lag behind the newest tag. Callers treat
/// it as a hint ([`fetch_upgrade_release`], [`fetch_install_release`]),
/// not as an authoritative newest release.
///
/// `Ok(None)` when the repository has no release marked as latest (GitHub
/// answers 404 — for example when every release is a prerelease).
fn fetch_owner_latest(agent: &Agent, repo: &str) -> Result<Option<Release>> {
    validate_repo(repo)?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    match agent.get(&url).call() {
        Ok(mut response) => {
            let release = response
                .body_mut()
                .read_json()
                .context("Failed to parse release info")?;
            Ok(Some(release))
        }
        Err(UreqError::StatusCode(404)) => Ok(None),
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
            bail!("Failed to fetch the latest release of {repo}: HTTP {status_code}");
        }
        Err(e) => Err(e).context("Failed to fetch the latest release"),
    }
}

pub fn fetch_release_info(agent: &Agent, repo: &str, tag: Option<&str>) -> Result<Release> {
    let Some(tag) = tag else {
        return fetch_owner_latest(agent, repo)?
            .ok_or_else(|| anyhow::anyhow!("no release marked as latest for {repo}"));
    };
    validate_repo(repo)?;
    let url = format!(
        "https://api.github.com/repos/{repo}/releases/tags/{}",
        urlencoding::encode(tag)
    );

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
            bail!("Failed to fetch release info for {repo}/{tag}: HTTP {status_code}");
        }
        Err(e) => {
            return Err(e)
                .with_context(|| format!("Failed to fetch release info for {repo}/{tag}"));
        }
    };

    response
        .body_mut()
        .read_json()
        .context("Failed to parse release info")
}

/// Fetch the newest stable release for a fresh install (nothing cached).
///
/// The owner-marked "Latest" release joins the candidate pool, but so do the
/// `scan_limit` newest stable releases from the release list
/// ([`scan_recent_releases`]), and the overall maximum wins
/// ([`select_latest_release`]): the label can lag behind the real newest
/// tag, and GitHub offers no endpoint that computes a version maximum.
pub fn fetch_install_release(agent: &Agent, repo: &str, scan_limit: usize) -> Result<Release> {
    let mut releases = scan_recent_releases(agent, repo, scan_limit)?;
    if let Some(hint) = fetch_owner_latest(agent, repo)?
        && !hint.prerelease
        && !hint.draft
    {
        releases.push(hint);
    }
    select_latest_release(releases)
        .ok_or_else(|| anyhow::anyhow!("No stable releases found for {repo}"))
}

/// Fetch the release to upgrade to when `--tag` is not given and a version
/// is already installed.
///
/// Resolution is cheapest-first:
///
/// 1. **Latest-label hint**: [`fetch_owner_latest`] in one request. The
///    owner-controlled label is trusted only when [`check_hint`] approves
///    the tag (stable, same tag family, not older than the install) and,
///    when it claims to be newer, [`compare_is_ahead`] confirms the tag
///    is really ahead of the installed one in git history. A tag the
///    compare API contradicts is excluded from the fallback scan too: as
///    the family maximum it would otherwise be re-selected there.
/// 2. **Bounded scan**: [`scan_recent_releases`] walks the release list
///    until `scan_limit` stable candidates have been seen, and
///    [`select_upgrade_release`] picks the newest in the installed tag
///    family. A family with no stable release inside that window is an
///    error, never a silent jump to an unrelated release stream.
pub fn fetch_upgrade_release(
    agent: &Agent,
    repo: &str,
    cached_tag: Option<&str>,
    scan_limit: usize,
) -> Result<Release> {
    let Some(cached_tag) = cached_tag else {
        return fetch_install_release(agent, repo, scan_limit);
    };

    let Some(cached_version) = parse_tag_version(cached_tag) else {
        return fetch_install_release(agent, repo, scan_limit);
    };

    let mut vetoed_tag: Option<String> = None;
    if let Some(hint) = fetch_owner_latest(agent, repo)? {
        match check_hint(&hint, &cached_version) {
            HintCheck::Accept => return Ok(hint),
            HintCheck::VerifyWithCompare => {
                let ahead = compare_is_ahead(agent, repo, cached_tag, &hint.tag_name)?;
                if ahead != Some(false) {
                    return Ok(hint);
                }
                eprintln!(
                    "Note: '{}' is marked latest but is not ahead of '{cached_tag}' in git history; ignoring it",
                    hint.tag_name
                );
                vetoed_tag = Some(hint.tag_name);
            }
            HintCheck::Reject => {
                eprintln!(
                    "Note: latest-release hint '{}' is not a usable upgrade; scanning releases instead",
                    hint.tag_name
                );
            }
        }
    }

    let releases = scan_recent_releases(agent, repo, scan_limit)?;
    let selected = select_upgrade_from_scan(releases, &cached_version, vetoed_tag.as_deref());
    match selected {
        Some(release) => Ok(release),
        None => bail!(
            "No stable release in the installed tag family '{}' within the latest {scan_limit} stable releases of {repo}; refusing to select an unrelated release. Raise stable_scan_limit in the settings file to scan further",
            cached_version.family
        ),
    }
}

/// The compare-API fields used to order two refs.
#[derive(Deserialize)]
struct CompareInfo {
    ahead_by: u64,
}

/// Whether `head` contains commits beyond `base` — strictly newer in git
/// history. `Ok(None)` when GitHub offers no comparison at all (unknown
/// ref or unrelated histories): the absence of a verdict is not a "no".
fn compare_is_ahead(agent: &Agent, repo: &str, base: &str, head: &str) -> Result<Option<bool>> {
    let url = format!(
        "https://api.github.com/repos/{repo}/compare/{}...{}",
        urlencoding::encode(base),
        urlencoding::encode(head)
    );
    let mut response = match agent.get(&url).call() {
        Ok(r) => r,
        Err(UreqError::StatusCode(404 | 409)) => return Ok(None),
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
            bail!("Failed to compare {base}...{head} for {repo}: HTTP {status_code}");
        }
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to compare {base}...{head} for {repo}"));
        }
    };
    let info: CompareInfo = response
        .body_mut()
        .read_json()
        .context("Failed to parse compare info")?;
    Ok(Some(info.ahead_by > 0))
}

/// Verdict on using the owner-marked "Latest" release as an upgrade target.
#[derive(Debug, PartialEq, Eq)]
enum HintCheck {
    /// Stable, in the installed tag family, at the same version: usable
    /// as-is (the install already sits at that version).
    Accept,
    /// Newer than the install by version: usable only once the compare API
    /// confirms the tag is ahead in git history, guarding against an owner
    /// mislabeling an old build with a bumped version.
    VerifyWithCompare,
    /// Not usable: not stable, unparseable, from another tag family, or
    /// older than the installed version.
    Reject,
}

fn check_hint(hint: &Release, installed: &TagVersion) -> HintCheck {
    if hint.prerelease || hint.draft {
        return HintCheck::Reject;
    }
    let Some(version) = parse_tag_version(&hint.tag_name) else {
        return HintCheck::Reject;
    };
    if version.family != installed.family {
        return HintCheck::Reject;
    }
    if version.parts < installed.parts {
        return HintCheck::Reject;
    }
    if version.parts == installed.parts {
        HintCheck::Accept
    } else {
        HintCheck::VerifyWithCompare
    }
}

/// The newest stable release across all tag families. Prereleases, drafts and
/// tags without a parseable version are ignored. Ties on
/// `[major, minor, patch]` (e.g. `2.2.9` vs `v2.2.9`) are broken
/// deterministically: the shortest version prefix wins, then the
/// lexicographically smallest.
fn select_latest_release(releases: Vec<Release>) -> Option<Release> {
    releases
        .into_iter()
        .filter_map(|release| {
            if release.prerelease || release.draft {
                return None;
            }
            let version = parse_tag_version(&release.tag_name)?;
            Some((version, release))
        })
        .max_by(|(a, _), (b, _)| {
            a.parts
                .cmp(&b.parts)
                .then_with(|| b.stream.len().cmp(&a.stream.len()))
                .then_with(|| b.stream.cmp(&a.stream))
        })
        .map(|(_, release)| release)
}

/// Pick the upgrade target from a scanned pool, skipping a compare-vetoed
/// tag. The vetoed hint is typically the family maximum, so excluding it
/// lets the scan fall back to the next-newest release of the family
/// instead of re-selecting the rejected one.
fn select_upgrade_from_scan(
    releases: Vec<Release>,
    cached_version: &TagVersion,
    vetoed_tag: Option<&str>,
) -> Option<Release> {
    let candidates = match vetoed_tag {
        Some(tag) => releases
            .into_iter()
            .filter(|release| release.tag_name != tag)
            .collect(),
        None => releases,
    };
    select_upgrade_release(candidates, cached_version)
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
        let token = get_auth_token(None);
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
        let token = get_auth_token(None);
        let agent = configure_agent(&ua, token.as_deref());

        let result = list_releases(&agent, "lucidfrontier45/grd");
        assert!(result.is_ok());
    }

    #[test]
    fn test_fetch_owner_latest_on_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let token = get_auth_token(None);
        let agent = configure_agent(&ua, token.as_deref());

        // fff publishes mostly prereleases, but its stable line carries the
        // owner's "Latest" marker; the hint must be a stable full release.
        let latest = fetch_owner_latest(&agent, "dmtrKovalenko/fff")
            .unwrap()
            .expect("fff marks a release as latest");
        assert!(!latest.prerelease);
        assert!(!latest.draft);
        assert!(!latest.assets.is_empty());
    }

    #[test]
    fn test_compare_is_ahead_on_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let token = get_auth_token(None);
        let agent = configure_agent(&ua, token.as_deref());

        assert_eq!(
            compare_is_ahead(&agent, "dmtrKovalenko/fff", "v0.10.6", "v0.11.0").unwrap(),
            Some(true)
        );
        assert_eq!(
            compare_is_ahead(&agent, "dmtrKovalenko/fff", "v0.11.0", "v0.10.6").unwrap(),
            Some(false)
        );
        // A ref GitHub cannot resolve yields no verdict, not "behind".
        assert_eq!(
            compare_is_ahead(
                &agent,
                "dmtrKovalenko/fff",
                "v0.11.0",
                "no-such-ref-grd-test"
            )
            .unwrap(),
            None
        );
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
        let link = "<https://api.github.com/repositories/212613049/releases?per_page=100&page=2>; rel=\"next\", <https://api.github.com/repositories/212613049/releases?per_page=100&page=4>; rel=\"last\"";
        assert_eq!(
            next_page_url(link, endpoint).unwrap().as_deref(),
            Some("https://api.github.com/repositories/212613049/releases?per_page=100&page=2")
        );
        assert_eq!(next_page_url("", endpoint).unwrap(), None);
        assert!(next_page_url("<https://example.com/evil>; rel=\"next\"", endpoint).is_err());
        assert!(
            next_page_url(
                "<https://api.github.com/repositories/not-an-id/releases?page=2>; rel=\"next\"",
                endpoint
            )
            .is_err()
        );
    }

    #[test]
    fn test_upgrade_selection_uses_later_page_and_skips_prereleases_and_drafts() {
        let cached_version = parse_tag_version("v2.2.7").unwrap();
        let page_one_url = "https://api.github.com/repos/owner/repo/releases?page=1";
        let page_two_url = "https://api.github.com/repos/owner/repo/releases?page=2";
        let releases = collect_release_pages(Some(page_one_url.to_string()), None, |url| {
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

    #[test]
    fn test_collect_release_pages_stops_once_stable_budget_is_reached() {
        let page_one_url = "https://api.github.com/repos/owner/repo/releases?page=1";
        let page_two_url = "https://api.github.com/repos/owner/repo/releases?page=2";
        let page_three_url = "https://api.github.com/repos/owner/repo/releases?page=3";
        let mut fetched = Vec::new();
        let releases = collect_release_pages(Some(page_one_url.to_string()), Some(2), |url| {
            fetched.push(url.to_string());
            if url == page_one_url {
                // Only one stable candidate on this page: prerelease and
                // unparseable tags do not count toward the budget, so the
                // walk must continue.
                Ok((
                    vec![
                        test_release("1.2.0-nightly.1", true, false),
                        test_release("nightly", false, false),
                        test_release("v1.2.0", false, false),
                    ],
                    Some(page_two_url.to_string()),
                ))
            } else if url == page_two_url {
                // Budget met after this page; page three must not be fetched
                // even though GitHub advertises a link to it.
                Ok((
                    vec![test_release("v1.2.1", false, false)],
                    Some(page_three_url.to_string()),
                ))
            } else {
                panic!("unexpected page fetched: {url}");
            }
        })
        .unwrap();
        assert_eq!(fetched, vec![page_one_url, page_two_url]);
        assert_eq!(releases.len(), 4);
    }

    #[test]
    fn test_collect_release_pages_without_budget_walks_every_page() {
        let page_one_url = "https://api.github.com/repos/owner/repo/releases?page=1";
        let page_two_url = "https://api.github.com/repos/owner/repo/releases?page=2";
        let mut fetched = Vec::new();
        let releases = collect_release_pages(Some(page_one_url.to_string()), None, |url| {
            fetched.push(url.to_string());
            Ok((
                vec![
                    test_release("v1.0.0", false, false),
                    test_release("v1.0.1-nightly.1", true, false),
                ],
                (url == page_one_url).then(|| page_two_url.to_string()),
            ))
        })
        .unwrap();
        assert_eq!(fetched, vec![page_one_url, page_two_url]);
        assert_eq!(releases.len(), 4);
    }

    #[test]
    fn test_vetoed_hint_is_not_reselected_by_the_scan() {
        let cached = parse_tag_version("v2.2.7").unwrap();
        let pool = || {
            vec![
                test_release("v2.2.8", false, false), // vetoed hint: family maximum
                test_release("v2.2.7", false, false),
                test_release("v2.2.6", false, false),
            ]
        };
        // Without a veto the scan picks the family maximum.
        assert_eq!(
            select_upgrade_from_scan(pool(), &cached, None)
                .unwrap()
                .tag_name,
            "v2.2.8"
        );
        // A compare-vetoed tag must not come back through the scan: the
        // next-newest release of the family wins instead.
        assert_eq!(
            select_upgrade_from_scan(pool(), &cached, Some("v2.2.8"))
                .unwrap()
                .tag_name,
            "v2.2.7"
        );
    }

    #[test]
    fn test_check_hint_gatekeeps_owner_latest() {
        let installed = parse_tag_version("v2.2.7").unwrap();
        // Strictly newer same-family stable: needs the compare cross-check.
        assert_eq!(
            check_hint(&test_release("v2.3.0", false, false), &installed),
            HintCheck::VerifyWithCompare
        );
        // Same version: the install is already there, nothing to verify.
        assert_eq!(
            check_hint(&test_release("v2.2.7", false, false), &installed),
            HintCheck::Accept
        );
        // Older same-family stable.
        assert_eq!(
            check_hint(&test_release("v2.2.6", false, false), &installed),
            HintCheck::Reject
        );
        // Prerelease or draft marker.
        assert_eq!(
            check_hint(&test_release("v2.2.8", true, false), &installed),
            HintCheck::Reject
        );
        assert_eq!(
            check_hint(&test_release("v2.2.8", false, true), &installed),
            HintCheck::Reject
        );
        // Another tag family, even when newer.
        assert_eq!(
            check_hint(&test_release("2.2.8", false, false), &installed),
            HintCheck::Reject
        );
        assert_eq!(
            check_hint(&test_release("daemon-v2.2.8", false, false), &installed),
            HintCheck::Reject
        );
        // Unparseable tag.
        assert_eq!(
            check_hint(&test_release("changelog", false, false), &installed),
            HintCheck::Reject
        );
    }

    #[test]
    fn test_select_latest_release_ignores_owner_latest_marker_and_other_families() {
        // anomalyco/opencode-style list: the owner marked v1.19.5 as
        // `/releases/latest`, but v2.2.7 exists unmarked alongside a
        // `process-daemon-v0.5.2` sub-project stream.
        let releases = vec![
            test_release("process-daemon-v0.5.2", false, false),
            test_release("v2.2.7", false, false),
            test_release("v1.19.5", false, false),
        ];
        let selected = select_latest_release(releases).unwrap();
        assert_eq!(selected.tag_name, "v2.2.7");
    }

    #[test]
    fn test_select_latest_release_skips_prereleases_drafts_and_unparseable_tags() {
        let releases = vec![
            test_release("v2.2.8-rc.1", true, false),
            test_release("v2.2.9", false, true),
            test_release("changelog", false, false),
        ];
        assert!(select_latest_release(releases).is_none());
    }

    #[test]
    fn test_select_latest_release_breaks_part_ties_deterministically() {
        // Same parts in different tag families: the shortest version prefix
        // wins, then the lexicographically smallest — independent of the
        // order the releases arrive in.
        let releases = vec![
            test_release("app-v2.2.9", false, false),
            test_release("2.2.9", false, false),
            test_release("v2.2.9", false, false),
        ];
        let selected = select_latest_release(releases).unwrap();
        assert_eq!(selected.tag_name, "2.2.9");

        let reversed = vec![
            test_release("v2.2.9", false, false),
            test_release("2.2.9", false, false),
            test_release("app-v2.2.9", false, false),
        ];
        let selected = select_latest_release(reversed).unwrap();
        assert_eq!(selected.tag_name, "2.2.9");
    }

    #[test]
    fn test_select_latest_release_empty_list_is_none() {
        assert!(select_latest_release(vec![]).is_none());
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
