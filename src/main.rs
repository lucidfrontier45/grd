use std::{env, fs, io::IsTerminal, path::PathBuf};

use anyhow::{Result, bail};
use clap::Parser;
use grd::{asset, cli::Args, config, confirm_upgrade, download, extract, github, state};

fn main() -> Result<()> {
    let args = Args::parse();

    if args.list_installed {
        if let Some(repo) = &args.repo {
            bail!("--list-installed does not accept a repo argument: {repo}");
        }
        let cache = state::State::load();
        if cache.versions.is_empty() {
            println!("No installed packages found.");
        } else {
            for (repo, release) in &cache.versions {
                println!("{} (tag: {}, asset: {})", repo, release.tag, release.asset);
            }
        }
        return Ok(());
    }

    if args.remove {
        let Some(repo) = &args.repo else {
            bail!("a repo argument is required for --remove");
        };
        let mut cache = state::State::load();
        if let Some(entry) = cache.remove_cached(repo) {
            let bin_name = repo.split('/').next_back().unwrap_or("app");
            let filename = if cfg!(windows) {
                format!("{}.exe", bin_name)
            } else {
                bin_name.to_string()
            };

            let dest = entry.destination.as_deref().unwrap_or(".");
            let target_path = PathBuf::from(dest).join(&filename);

            if target_path.exists() {
                fs::remove_file(&target_path)?;
                println!("Removed '{}'", bin_name);
            } else {
                eprintln!("Warning: binary not found at {:?}", target_path);
            }

            cache.save();
        } else {
            eprintln!(
                "Warning: no cached entry found for '{}' — nothing to remove.",
                repo
            );
        }
        return Ok(());
    }

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
    let token = config::get_auth_token();
    let agent = config::configure_agent(&ua, token.as_deref());

    let Some(repo) = &args.repo else {
        bail!("a repo argument is required");
    };

    if args.list {
        let releases = github::list_releases(&agent, repo)?;
        println!("Available releases for {}:", repo);
        for rel in releases {
            println!("  - {}", rel.tag_name);
        }
        return Ok(());
    }

    let release = github::fetch_release_info(&agent, repo, args.tag.as_deref())?;
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
        args.select,
        args.exclude.as_deref(),
    )
    .inspect_err(|_| {
        if args.select {
            eprintln!("Note: --select flag was used but manual selection failed");
        }
    })?;
    println!("Selected asset: {}", asset.name);

    let bin_name = args
        .bin_name
        .clone()
        .unwrap_or_else(|| repo.split('/').next_back().unwrap_or("app").to_string());

    if args.tag.is_none() && !args.force {
        let target_path = if args.no_decompress {
            args.destination.join(&asset.name)
        } else if cfg!(windows) {
            args.destination.join(format!("{}.exe", bin_name))
        } else {
            args.destination.join(&bin_name)
        };
        if target_path.exists() {
            let cache = state::State::load();
            let cached = cache.get_cached(repo);
            let cache_hit =
                cached.is_some_and(|c| c.tag == release.tag_name && c.asset == asset.name);
            if cache_hit {
                println!("Already at {} version {}", asset.name, release.tag_name);
                return Ok(());
            }
            if let Some(cached) = cached
                && !args.yes
            {
                if !std::io::stdin().is_terminal() {
                    bail!(
                        "refusing to prompt for upgrade in non-interactive mode; pass -y to proceed"
                    );
                }
                if !confirm_upgrade(&cached.tag, &release.tag_name) {
                    println!("Upgrade cancelled.");
                    return Ok(());
                }
            }
        }
    }

    let source = download::download_asset(&agent, &asset, args.memory_limit)?;

    extract::extract_and_save(
        source,
        &asset.name,
        &bin_name,
        &args.destination,
        args.no_decompress,
    )?;

    let mut cache = state::State::load();
    cache.set_cached(
        repo,
        &asset.name,
        &release.tag_name,
        Some(args.destination.display().to_string()),
    );
    cache.save();

    println!(
        "Successfully installed '{}' to {:?}",
        bin_name, args.destination
    );
    Ok(())
}
