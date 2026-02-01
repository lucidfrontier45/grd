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

pub fn list_releases(agent: &Agent, repo: &str) -> Result<()> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let mut response = agent.get(&url).call().context("Failed to list releases")?;
    let releases: Vec<Release> = response
        .body_mut()
        .read_json()
        .context("Failed to parse releases")?;

    println!("Available releases for {}:", repo);
    for rel in releases {
        println!("  - {}", rel.tag_name);
    }
    Ok(())
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
