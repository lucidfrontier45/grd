use std::{env, fs, io::IsTerminal, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use grd::{
    asset,
    cli::{Args, Command},
    config, confirm_upgrade, download, extract, github, state,
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

/// Resolve the filename written to disk.
///
/// `--rename` always wins. Under `--no-decompress` nothing is extracted, so
/// without `--rename` the asset keeps its own filename (today's behavior);
/// otherwise the lookup name doubles as the output name.
fn resolve_out_name(
    rename: Option<&str>,
    lookup_name: &str,
    asset_name: &str,
    no_decompress: bool,
) -> String {
    match rename {
        Some(rename) => rename.to_string(),
        None if no_decompress => asset_name.to_string(),
        None => lookup_name.to_string(),
    }
}

/// Drop a trailing `.exe` so `--rename fd.exe` and `--rename fd` agree.
fn strip_exe_suffix(name: &str) -> &str {
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// Apply the platform naming rule once, so the same string is used for the
/// target path, the extraction output, and the recorded state entry.
fn out_file_name(out_name: &str, no_decompress: bool) -> String {
    if no_decompress {
        out_name.to_string()
    } else if cfg!(windows) {
        format!("{}.exe", strip_exe_suffix(out_name))
    } else {
        out_name.to_string()
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
    let out_name = resolve_out_name(
        args.rename.as_deref(),
        &lookup_name,
        &asset.name,
        args.no_decompress,
    );
    // One normalization pass: the same string feeds the target path, the
    // extractor, and the state entry, so they cannot drift apart.
    let out_file = out_file_name(&out_name, args.no_decompress);
    let target_path = dest.join(&out_file);

    if args.dry_run {
        let cache = state::State::load();
        let cached = cache.get_cached(repo);
        let is_same_version =
            cached.is_some_and(|c| c.tag == release.tag_name && c.asset == asset.name);

        if !args.force && target_path.exists() && is_same_version {
            println!(
                "[dry-run] already at {} {}; no action",
                asset.name, release.tag_name
            );
            return Ok(());
        }
        println!(
            "[dry-run] would install '{}' to {}",
            out_file,
            dest.display()
        );
        println!(
            "[dry-run] source: {} {} → {}",
            repo, release.tag_name, asset.name
        );
        return Ok(());
    }

    if !args.force && target_path.exists() {
        let cache = state::State::load();
        let cached = cache.get_cached(repo);
        let is_same_version =
            cached.is_some_and(|c| c.tag == release.tag_name && c.asset == asset.name);

        if is_same_version {
            println!("Already at {} version {}", asset.name, release.tag_name);
            return Ok(());
        }

        // Prompt for upgrade only when tag is not explicitly pinned
        if args.tag.is_none()
            && let Some(cached) = cached
            && !args.yes
        {
            if !std::io::stdin().is_terminal() {
                bail!("refusing to prompt for upgrade in non-interactive mode; pass -y to proceed");
            }
            if !confirm_upgrade(&cached.tag, &release.tag_name) {
                println!("Upgrade cancelled.");
                return Ok(());
            }
        }
    }

    let source = download::download_asset(&agent, &asset, args.memory_limit)?;

    extract::extract_and_save(
        source,
        &asset.name,
        &lookup_name,
        &out_file,
        &dest,
        args.no_decompress,
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
    fn test_resolve_out_name_rename_wins_over_lookup() {
        assert_eq!(
            resolve_out_name(Some("exe2"), "exe1", "asset.tar.gz", false),
            "exe2"
        );
    }

    #[test]
    fn test_resolve_out_name_defaults_to_lookup_name() {
        assert_eq!(resolve_out_name(None, "prj", "asset.tar.gz", false), "prj");
    }

    #[test]
    fn test_resolve_out_name_no_decompress_defaults_to_asset() {
        assert_eq!(
            resolve_out_name(None, "prj", "asset.tar.gz", true),
            "asset.tar.gz"
        );
    }

    #[test]
    fn test_resolve_out_name_no_decompress_honors_rename() {
        assert_eq!(
            resolve_out_name(Some("exe2"), "prj", "asset.tar.gz", true),
            "exe2"
        );
    }

    #[test]
    fn test_strip_exe_suffix_is_case_insensitive() {
        assert_eq!(strip_exe_suffix("fd.exe"), "fd");
        assert_eq!(strip_exe_suffix("fd.EXE"), "fd");
        assert_eq!(strip_exe_suffix("fd.Exe"), "fd");
    }

    #[test]
    fn test_strip_exe_suffix_leaves_other_names_alone() {
        assert_eq!(strip_exe_suffix("fd"), "fd");
        assert_eq!(strip_exe_suffix("fd.tar.gz"), "fd.tar.gz");
        assert_eq!(strip_exe_suffix("fd.exe.bak"), "fd.exe.bak");
        // A bare ".exe" has no stem left to keep.
        assert_eq!(strip_exe_suffix(".exe"), ".exe");
    }

    #[test]
    fn test_out_file_name_no_decompress_keeps_name_verbatim() {
        // A raw asset keeps its extension; no .exe rewriting happens.
        assert_eq!(
            out_file_name("asset.tar.gz", true),
            "asset.tar.gz".to_string()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_out_file_name_unix_is_verbatim() {
        assert_eq!(out_file_name("fd", false), "fd");
        assert_eq!(out_file_name("fd.exe", false), "fd.exe");
    }

    #[cfg(windows)]
    #[test]
    fn test_out_file_name_windows_appends_single_exe() {
        assert_eq!(out_file_name("fd", false), "fd.exe");
        assert_eq!(out_file_name("fd.exe", false), "fd.exe");
        assert_eq!(out_file_name("fd.EXE", false), "fd.exe");
    }
}
