use std::{
    env, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use grd::{
    asset,
    cli::{Args, Command},
    config, confirm_upgrade, download, extract, github, naming, state,
};
use ureq::Agent;

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
        Some(command) => run_command(command),
        None => run_install(&args),
    }
}

/// Run the requested subcommand. The install pipeline ([`run_install`]) is the
/// no-subcommand path.
fn run_command(command: &Command) -> Result<()> {
    match command {
        Command::Register { path } => cmd_register(path),
        Command::Remove { repo } => cmd_remove(repo),
        Command::Info { repo } => cmd_info(repo),
        Command::ListInstalled => cmd_list_installed(),
        Command::ListPlatform => cmd_list_platform(),
    }
}

/// `grd register`: remember the default install directory.
fn cmd_register(path: &Path) -> Result<()> {
    let mut cache = state::State::load();
    let path_str = path.display().to_string();
    cache.set_default_install_dir(&path_str);
    cache.save();
    println!("Default install directory set to: {}", path.display());
    Ok(())
}

/// `grd remove`: delete the installed binary and its cache entry.
fn cmd_remove(repo: &str) -> Result<()> {
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
    Ok(())
}

/// `grd info`: print the cached install record for `repo`.
fn cmd_info(repo: &str) -> Result<()> {
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
    Ok(())
}

/// `grd list-installed`: list cached packages.
fn cmd_list_installed() -> Result<()> {
    let cache = state::State::load();
    if cache.versions.is_empty() {
        println!("No installed packages found.");
    } else {
        for (repo, release) in &cache.versions {
            println!("{} (tag: {}, asset: {})", repo, release.tag, release.asset);
        }
    }
    Ok(())
}

/// `grd list-platform`: print supported platform pairs.
fn cmd_list_platform() -> Result<()> {
    println!("Supported platforms:");
    println!("  - windows-x86_64");
    println!("  - windows-aarch64");
    println!("  - macos-x86_64");
    println!("  - macos-aarch64");
    println!("  - linux-x86_64");
    println!("  - linux-aarch64");
    Ok(())
}

/// The install pipeline for the no-subcommand path.
fn run_install(args: &Args) -> Result<()> {
    let dest = resolve_dest(args)?;
    let (agent, repo) = prepare_request(args)?;

    if args.list {
        return list_available_releases(&agent, repo);
    }

    let release = github::fetch_release_info(&agent, repo, args.tag.as_deref())?;
    println!("Selected version: {}", release.tag_name);

    let (os, arch) = detect_platform(args)?;
    let asset = pick_asset(args, &release, &os, &arch)?;
    println!("Selected asset: {}", asset.name);

    install_release(args, &agent, repo, &dest, &release, &asset, &os)
}

/// Destination directory: `--destination`, else the registered default, else
/// the platform default. Created unless `--dry-run`.
fn resolve_dest(args: &Args) -> Result<PathBuf> {
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
    Ok(dest)
}

/// Build the API client and pass the remaining local validations before any
/// network work: a rejected value must not cost the user a release lookup or
/// an asset download.
fn prepare_request(args: &Args) -> Result<(Agent, &str)> {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = config::get_auth_token();
    let agent = config::configure_agent(&ua, token.as_deref());

    let repo = args
        .repo
        .as_deref()
        .context("a repo argument is required")?;
    if let Some(rename) = &args.rename {
        validate_rename(rename)?;
    }
    Ok((agent, repo))
}

/// `--list`: print the available release tags and stop.
fn list_available_releases(agent: &Agent, repo: &str) -> Result<()> {
    let releases = github::list_releases(agent, repo)?;
    println!("Available releases for {}:", repo);
    for rel in releases {
        println!("  - {}", rel.tag_name);
    }
    Ok(())
}

/// Target platform: `--os`/`--arch` when given, else the host's.
fn detect_platform(args: &Args) -> Result<(String, String)> {
    let os = detect_os(args)?;
    let arch = detect_arch(args)?;
    println!("{} platform: {}-{}", platform_label(args), os, arch);
    Ok((os, arch))
}

/// `--os` when given (normalized), else the host OS.
fn detect_os(args: &Args) -> Result<String> {
    match &args.os {
        Some(s) => asset::normalize_os(s),
        None => Ok(env::consts::OS.to_string()),
    }
}

/// `--arch` when given (normalized), else the host arch.
fn detect_arch(args: &Args) -> Result<String> {
    match &args.arch {
        Some(s) => asset::normalize_arch(s),
        None => Ok(env::consts::ARCH.to_string()),
    }
}

/// "Detected" when neither `--os` nor `--arch` was given, else "Using".
fn platform_label(args: &Args) -> &'static str {
    if args.os.is_none() && args.arch.is_none() {
        "Detected"
    } else {
        "Using"
    }
}

/// The asset to install: interactive `--select`/`--select-all` modes, else the
/// auto-picked best match. Multiple candidates abort with a listing.
fn pick_asset(
    args: &Args,
    release: &github::Release,
    os: &str,
    arch: &str,
) -> Result<github::Asset> {
    let selection = asset::select_asset(
        &release.assets,
        os,
        arch,
        select_mode(args),
        args.exclude.as_deref(),
        args.no_ext_filter,
    )
    .inspect_err(|_| note_selection_failure(args))?;
    match selection {
        asset::AssetSelection::Single(asset) => Ok(asset),
        asset::AssetSelection::Multiple(matches) => bail!(
            "Multiple assets found for {os}-{arch}. Refine filters or pass --select to choose interactively:\n{}",
            listing(&matches)
        ),
    }
}

/// Interactive selection mode implied by the CLI flags.
fn select_mode(args: &Args) -> asset::SelectMode {
    if args.select_all {
        asset::SelectMode::All
    } else if args.select {
        asset::SelectMode::Filtered
    } else {
        asset::SelectMode::Default
    }
}

/// Note which manual mode was active when selection errors out.
fn note_selection_failure(args: &Args) {
    if args.select {
        eprintln!("Note: --select flag was used but manual selection failed");
    } else if args.select_all {
        eprintln!("Note: --select-all flag was used but manual selection failed");
    }
}

/// The candidate listing shown when selection finds several matches.
fn listing(matches: &[github::Asset]) -> String {
    matches
        .iter()
        .map(|a| format!("  - {}", a.name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The tail of the install pipeline: dry-run report, up-to-date short-circuit,
/// upgrade confirmation, then download/extract/record.
fn install_release(
    args: &Args,
    agent: &Agent,
    repo: &str,
    dest: &Path,
    release: &github::Release,
    asset: &github::Asset,
    os: &str,
) -> Result<()> {
    let cache = state::State::load();
    let cached = cache.get_cached(repo);

    let lookup_name = resolve_lookup_name(args, repo);
    // The output filename is known before extraction only for an explicit
    // `--rename` or `--no-decompress`; otherwise it is the matched archive
    // entry's own name, which only the extractor can report.
    let out_name = resolve_known_out_name(args, &asset.name, os)
        .or_else(|| cached.map(|c| c.installed_filename(repo)));
    let installed_path = out_name.as_ref().map(|name| dest.join(name));
    let up_to_date = is_up_to_date(args, cached, release, asset, &installed_path);

    if args.dry_run {
        return dry_run_report(
            dest,
            repo,
            release,
            asset,
            out_name.as_deref().unwrap_or(&lookup_name),
            up_to_date,
        );
    }

    if up_to_date {
        println!("Already at {} version {}", asset.name, release.tag_name);
        return Ok(());
    }

    // Prompt for upgrade only when an install exists and the tag is not pinned.
    if let Some(cached_tag) = cached.map(|c| c.tag.as_str())
        && needs_upgrade_confirmation(args, &installed_path)
        && !confirm_upgrade_to(cached_tag, &release.tag_name)?
    {
        return Ok(());
    }

    let out_file = download_and_extract(args, agent, dest, asset, os, &lookup_name)?;

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

/// Download the asset and extract it into `dest`, returning the output
/// filename.
fn download_and_extract(
    args: &Args,
    agent: &Agent,
    dest: &Path,
    asset: &github::Asset,
    os: &str,
    lookup_name: &str,
) -> Result<String> {
    let source = download::download_asset(agent, asset, args.memory_limit)?;
    extract::extract_and_save(
        source,
        &asset.name,
        lookup_name,
        args.rename.as_deref(),
        dest,
        args.no_decompress,
        os,
    )
}

/// True when `--force` overrides the up-to-date short-circuit and the upgrade
/// confirmation.
fn is_forced(args: &Args) -> bool {
    args.force
}

/// True when `--tag` pins the release: a pinned install is deliberate and is
/// never confirmed interactively.
fn is_tag_pinned(args: &Args) -> bool {
    args.tag.is_some()
}

/// True when `--yes` pre-accepts the upgrade.
fn is_confirmed(args: &Args) -> bool {
    args.yes
}

/// True when a previously installed binary exists at `installed_path`.
fn install_exists(installed_path: &Option<PathBuf>) -> bool {
    installed_path.as_ref().is_some_and(|p| p.exists())
}

/// True when the cached record already matches this release and asset.
fn is_same_version(
    cached: Option<&state::CachedRelease>,
    release: &github::Release,
    asset: &github::Asset,
) -> bool {
    cached.is_some_and(|c| c.tag == release.tag_name && c.asset == asset.name)
}

/// True when the install on disk is already at this exact release and asset
/// and `--force` does not override — there is nothing to install.
fn is_up_to_date(
    args: &Args,
    cached: Option<&state::CachedRelease>,
    release: &github::Release,
    asset: &github::Asset,
    installed_path: &Option<PathBuf>,
) -> bool {
    !is_forced(args) && is_same_version(cached, release, asset) && install_exists(installed_path)
}

/// True when an existing install should be confirmed interactively before
/// being upgraded.
fn needs_upgrade_confirmation(args: &Args, installed_path: &Option<PathBuf>) -> bool {
    !is_forced(args)
        && !is_tag_pinned(args)
        && install_exists(installed_path)
        && !is_confirmed(args)
}

/// Ask the user to confirm upgrading from `cached_tag`; returns `false` when
/// the upgrade is cancelled.
fn confirm_upgrade_to(cached_tag: &str, release_tag: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("refusing to prompt for upgrade in non-interactive mode; pass -y to proceed");
    }
    if !confirm_upgrade(cached_tag, release_tag) {
        println!("Upgrade cancelled.");
        return Ok(false);
    }
    Ok(true)
}

/// `--dry-run`: report what would happen and stop before any change.
fn dry_run_report(
    dest: &Path,
    repo: &str,
    release: &github::Release,
    asset: &github::Asset,
    out_name: &str,
    up_to_date: bool,
) -> Result<()> {
    if up_to_date {
        println!(
            "[dry-run] already at {} {}; no action",
            asset.name, release.tag_name
        );
        return Ok(());
    }
    println!(
        "[dry-run] would install '{}' to {}",
        out_name,
        dest.display()
    );
    println!(
        "[dry-run] source: {} {} → {}",
        repo, release.tag_name, asset.name
    );
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
