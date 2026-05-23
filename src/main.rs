use std::env;

use anyhow::Result;
use clap::Parser;
use grd::{asset, cli::Args, config, download, extract, github, state};

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
    let token = config::get_auth_token();
    let agent = config::configure_agent(&ua, token.as_deref());

    if args.list {
        let releases = github::list_releases(&agent, &args.repo)?;
        println!("Available releases for {}:", &args.repo);
        for rel in releases {
            println!("  - {}", rel.tag_name);
        }
        return Ok(());
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
        args.select,
        args.exclude.as_deref(),
    )
    .inspect_err(|_| {
        if args.select {
            eprintln!("Note: --select flag was used but manual selection failed");
        }
    })?;
    println!("Selected asset: {}", asset.name);

    let bin_name = args.bin_name.clone().unwrap_or_else(|| {
        args.repo
            .split('/')
            .next_back()
            .unwrap_or("app")
            .to_string()
    });

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
            let cache_hit = cache
                .get_cached(&args.repo)
                .is_some_and(|cached| cached.tag == release.tag_name && cached.asset == asset.name);
            if cache_hit {
                println!("Already at {} version {}", asset.name, release.tag_name);
                return Ok(());
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
    cache.set_cached(&args.repo, &asset.name, &release.tag_name);
    cache.save();

    println!(
        "Successfully installed '{}' to {:?}",
        bin_name, args.destination
    );
    Ok(())
}
