use std::env;

use anyhow::Result;
use clap::Parser;

mod asset;
mod cli;
mod download;
mod extract;
mod github;

use crate::cli::Args;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.list_platforms {
        println!("Supported platforms:");
        println!("  - windows-x86_64");
        println!("  - windows-aarch64");
        println!("  - macos-x86_64");
        println!("  - macos-aarch64");
        println!("  - linux-x86_64");
        println!("  - linux-aarch64");
        return Ok(());
    }

    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let agent: ureq::Agent = ureq::Agent::config_builder().user_agent(&ua).build().into();

    if args.list {
        return github::list_releases(&agent, &args.repo);
    }

    let release = github::fetch_release_info(&agent, &args.repo, args.tag.as_deref())?;
    println!("Selected version: {}", release.tag_name);

    let os = match &args.os {
        Some(s) => asset::normalize_os(s)?,
        None => env::consts::OS.to_string(),
    };
    let arch = match &args.arch {
        Some(s) => asset::normalize_arch(s)?,
        None => env::consts::ARCH.to_string(),
    };

    if args.os.is_none() && args.arch.is_none() {
        println!("Detected platform: {}-{}", os, arch);
    } else {
        println!("Using platform: {}-{}", os, arch);
    }

    let asset = asset::select_asset(
        &release.assets,
        &os,
        &arch,
        args.first,
        args.exclude.as_deref(),
    )?;
    println!("Selected asset: {}", asset.name);

    let bin_name = args.bin_name.unwrap_or_else(|| {
        args.repo
            .split('/')
            .next_back()
            .unwrap_or("app")
            .to_string()
    });

    let source = download::download_asset(&agent, &asset, args.memory_limit)?;

    extract::extract_and_save(
        source,
        &asset.name,
        &bin_name,
        &args.destination,
        args.no_decompress,
    )?;

    println!(
        "Successfully installed '{}' to {:?}",
        bin_name, args.destination
    );
    Ok(())
}
