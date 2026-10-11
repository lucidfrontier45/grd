use std::{env, fs, io::IsTerminal, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use grd::{
    asset,
    cli::{Args, Command},
    config, confirm_upgrade, download, extract, github, naming, state,
};

/// Filename looked up inside the release archive: `--bin-name` when given,
/// otherwise the repository basename.
fn resolve_lookup_name(args: &Args, repo: &str) -> String {
    args.bin_name
        .clone()
        .unwrap_or_else(|| repo.split('/').next_back().unwrap_or("app").to_string())
}

/// Reject `--rename` values that could escape the destination directory.
/// `dest.join(rename)` honors any separator inside `rename`, so a value such
/// as `../../etc/cron.d/pwn` would write outside `dest`.
fn validate_rename(rename: &str) -> Result<()> {
    if rename.is_empty() {
        bail!("--rename must not be empty");
    }
    if rename.contains('/') || rename.contains('\\') || rename.contains("..") {
        bail!("--rename must be a plain filename, not a path: '{rename}'");
    }
    Ok(())
}

/// Output filename that is known before extraction, if any.
///
/// An explicit `--rename` (with the Windows `.exe` rule) or the asset's own
/// name under `--no-decompress` is known up front; otherwise the name comes
/// from the matched archive entry and is only known after extraction.
fn resolve_known_out_name(args: &Args, asset_name: &str, os: &str) -> Option<String> {
    if let Some(rename) = &args.rename {
        Some(naming::rename_output(rename, os))
    } else if args.no_decompress {
        Some(asset_name.to_string())
    } else {
        None
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    match &args.command {
        Some(Command::Register { path }) => {
            let mut cache = state::State::load();
            let path_str = path.display().to_string();
            cache.set_default_install_dir(&path_str);
            cache.save();
            println!("Default install directory set to: {}", path.display());
            return Ok(());
        }
        Some(Command::Remove { repo }) => {
            let mut cache = state::State::load();
            if let Some(entry) = cache.remove_cached(repo) {
                let filename = entry.installed_filename(repo);

                let dest = &entry.destination;
                let target_path = PathBuf::from(dest).join(&filename);

                if target_path.exists() {
                    fs::remove_file(&target_path)?;
                    println!("Removed '{}'", filename);
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
        Some(Command::Info { repo }) => {
            let cache = state::State::load();
            match cache.get_cached(repo) {
                Some(entry) => {
                    let filename = entry.installed_filename(repo);
                    let dest = &entry.destination;
                    let binary_path = PathBuf::from(dest).join(&filename);

                    println!(
                        "repo={};tag={};asset={};destination={};binary={};binary_exists={}",
                        repo,
                        entry.tag,
                        entry.asset,
                        dest,
                        binary_path.display(),
                        binary_path.exists()
                    );
                }
                None => {
                    eprintln!("No cached entry found for '{}'", repo);
                }
            }
            return Ok(());
        }
        Some(Command::ListInstalled) => {
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
        Some(Command::ListPlatform) => {
            println!("Supported platforms:");
            println!("  - windows-x86_64");
            println!("  - windows-aarch64");
            println!("  - macos-x86_64");
            println!("  - macos-aarch64");
            println!("  - linux-x86_64");
            println!("  - linux-aarch64");
            return Ok(());
        }
        None => {}
    }

    let dest = match &args.destination {
        Some(d) => d.clone(),
        None => state::State::load()
            .get_default_install_dir()
            .map(PathBuf::from)
            .unwrap_or_else(state::State::default_install_path),
    };
    if !args.dry_run {
        fs::create_dir_all(&dest).context("Failed to create destination directory")?;
    }

    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = config::get_auth_token();
    let agent = config::configure_agent(&ua, token.as_deref());

    let Some(repo) = &args.repo else {
        bail!("a repo argument is required");
    };

    // Validate before any network work: a rejected value must not cost the user
    // a release lookup or an asset download.
    if let Some(rename) = &args.rename {
        validate_rename(rename)?;
    }

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

    let select_mode = if args.select_all {
        asset::SelectMode::All
    } else if args.select {
        asset::SelectMode::Filtered
    } else {
        asset::SelectMode::Default
    };

    let asset = match asset::select_asset(
        &release.assets,
        &os,
        &arch,
        select_mode,
        args.exclude.as_deref(),
        args.no_ext_filter,
    )
    .inspect_err(|_| {
        if args.select {
            eprintln!("Note: --select flag was used but manual selection failed");
        } else if args.select_all {
            eprintln!("Note: --select-all flag was used but manual selection failed");
        }
    })? {
        asset::AssetSelection::Single(asset) => asset,
        asset::AssetSelection::Multiple(matches) => {
            let listing = matches
                .iter()
                .map(|a| format!("  - {}", a.name))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "Multiple assets found for {os}-{arch}. Refine filters or pass --select to choose interactively:\n{listing}"
            );
        }
    };
    println!("Selected asset: {}", asset.name);

    let lookup_name = resolve_lookup_name(&args, repo);

    // The output filename is known before extraction only for an explicit
    // `--rename` or `--no-decompress`; otherwise it is the matched archive
    // entry's own name, which only the extractor can report.
    let known_out = resolve_known_out_name(&args, &asset.name, &os);

    let cache = state::State::load();
    let cached = cache.get_cached(repo);
    let cached_tag = cached.map(|c| c.tag.clone());
    let predicted_out = known_out.or_else(|| cached.map(|c| c.installed_filename(repo)));
    let is_same_version =
        cached.is_some_and(|c| c.tag == release.tag_name && c.asset == asset.name);
    let installed_path = predicted_out.as_ref().map(|name| dest.join(name));

    if args.dry_run {
        if !args.force && is_same_version && installed_path.as_ref().is_some_and(|p| p.exists()) {
            println!(
                "[dry-run] already at {} {}; no action",
                asset.name, release.tag_name
            );
            return Ok(());
        }
        println!(
            "[dry-run] would install '{}' to {}",
            predicted_out.as_deref().unwrap_or(&lookup_name),
            dest.display()
        );
        println!(
            "[dry-run] source: {} {} → {}",
            repo, release.tag_name, asset.name
        );
        return Ok(());
    }

    if !args.force && is_same_version && installed_path.as_ref().is_some_and(|p| p.exists()) {
        println!("Already at {} version {}", asset.name, release.tag_name);
        return Ok(());
    }

    // Prompt for upgrade only when an install exists and the tag is not pinned.
    if !args.force
        && args.tag.is_none()
        && installed_path.as_ref().is_some_and(|p| p.exists())
        && let Some(cached_tag) = &cached_tag
        && !args.yes
    {
        if !std::io::stdin().is_terminal() {
            bail!("refusing to prompt for upgrade in non-interactive mode; pass -y to proceed");
        }
        if !confirm_upgrade(cached_tag, &release.tag_name) {
            println!("Upgrade cancelled.");
            return Ok(());
        }
    }

    let source = download::download_asset(&agent, &asset, args.memory_limit)?;

    let out_file = extract::extract_and_save(
        source,
        &asset.name,
        &lookup_name,
        args.rename.as_deref(),
        &dest,
        args.no_decompress,
        &os,
    )?;

    let mut cache = state::State::load();
    cache.set_cached(
        repo,
        &asset.name,
        &release.tag_name,
        dest.display().to_string(),
        &out_file,
    );
    cache.save();

    println!("Successfully installed '{}' to {:?}", out_file, dest);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rename_accepts_plain_filenames() {
        assert!(validate_rename("fd").is_ok());
        assert!(validate_rename("my-tool").is_ok());
        assert!(validate_rename("my_tool.exe").is_ok());
    }

    #[test]
    fn test_validate_rename_rejects_empty() {
        let err = validate_rename("").unwrap_err().to_string();
        assert!(
            err.contains("--rename"),
            "error should name the flag: {err}"
        );
    }

    #[test]
    fn test_validate_rename_rejects_path_separators() {
        for bad in [
            "../evil", "sub/dir", "..\\evil", "sub\\dir", "..", "a/../b", "..hidden",
        ] {
            let err = validate_rename(bad).unwrap_err().to_string();
            assert!(
                err.contains("--rename"),
                "error for '{bad}' should name the flag: {err}"
            );
        }
    }

    #[test]
    fn test_resolve_lookup_name_prefers_bin_name() {
        let args = Args::parse_from(["grd", "owner/prj", "--bin-name", "exe1"]);
        assert_eq!(resolve_lookup_name(&args, "owner/prj"), "exe1");
    }

    #[test]
    fn test_resolve_lookup_name_defaults_to_repo_basename() {
        let args = Args::parse_from(["grd", "owner/prj"]);
        assert_eq!(resolve_lookup_name(&args, "owner/prj"), "prj");
    }

    #[test]
    fn test_resolve_known_out_name_rename_appends_exe_on_windows() {
        let args = Args::parse_from(["grd", "owner/prj", "--rename", "tool"]);
        assert_eq!(
            resolve_known_out_name(&args, "asset.tar.gz", "windows"),
            Some("tool.exe".to_string())
        );
    }

    #[test]
    fn test_resolve_known_out_name_rename_verbatim_off_windows() {
        let args = Args::parse_from(["grd", "owner/prj", "--rename", "tool"]);
        assert_eq!(
            resolve_known_out_name(&args, "asset.tar.gz", "linux"),
            Some("tool".to_string())
        );
    }

    #[test]
    fn test_resolve_known_out_name_no_decompress_uses_asset_name() {
        let args = Args::parse_from(["grd", "owner/prj", "--no-decompress"]);
        assert_eq!(
            resolve_known_out_name(&args, "asset.tar.gz", "linux"),
            Some("asset.tar.gz".to_string())
        );
    }

    #[test]
    fn test_resolve_known_out_name_defaults_to_none() {
        let args = Args::parse_from(["grd", "owner/prj"]);
        assert_eq!(resolve_known_out_name(&args, "asset.tar.gz", "linux"), None);
    }
}
