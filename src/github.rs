use anyhow::{Context, Result, bail};
use serde::Deserialize;
use ureq::Agent;

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
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let mut response = agent.get(&url).call().context("Failed to list releases")?;
    response
        .body_mut()
        .read_json()
        .context("Failed to parse releases")
}

pub fn fetch_release_info(agent: &Agent, repo: &str, tag: Option<&str>) -> Result<Release> {
    let url = match tag {
        Some(t) => format!("https://api.github.com/repos/{}/releases/tags/{}", repo, t),
        None => format!("https://api.github.com/repos/{}/releases/latest", repo),
    };

    let mut response = agent.get(&url).call().with_context(|| {
        format!(
            "Failed to fetch release info for {}/{}",
            repo,
            tag.unwrap_or("latest")
        )
    })?;

    if !response.status().is_success() {
        bail!("Failed to fetch release info: HTTP {}", response.status())
    }

    response
        .body_mut()
        .read_json()
        .context("Failed to parse release info")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[ignore = "Requires GitHub API access (may be rate limited)"]
    fn test_fetch_release_from_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let agent = Agent::config_builder().user_agent(&ua).build().into();

        let result = fetch_release_info(&agent, "lucidfrontier45/grd", None);
        assert!(result.is_ok());

        let release = result.unwrap();
        assert!(!release.tag_name.is_empty());
        assert!(!release.assets.is_empty());
    }

    #[test]
    #[ignore = "Requires GitHub API access (may be rate limited)"]
    fn test_list_releases_from_real_repo() {
        let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
        let agent = Agent::config_builder().user_agent(&ua).build().into();

        let result = list_releases(&agent, "lucidfrontier45/grd");
        assert!(result.is_ok());
    }
}
