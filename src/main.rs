use std::{
    env, fs,
    io::{BufRead, IsTerminal},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use grd::{
    asset,
    cli::{Args, Command, ConfigCommand, GithubPatAction, InstallDirAction},
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

/// Reject `--bin-name` values that cannot become a plain on-disk filename.
///
/// `--bin-name` names the installed file, so a value like `..` or `/` would
/// escape the destination directory. A path (`pkg/bin/app`) is fine: only its
/// last component becomes the file name.
fn validate_bin_name(bin_name: &str) -> Result<()> {
    let base = naming::base_name(bin_name);
    if base.is_empty() || base == "." || base == ".." {
        bail!("--bin-name must name a file, not a path: '{bin_name}'");
    }
    Ok(())
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

/// Name the installed binary will have.
///
/// `--rename` wins, then `--bin-name`, then the repository basename; a
/// Windows target always carries a single `.exe` suffix. Under
/// `--no-decompress` nothing is extracted, so the asset's own name is kept.
/// This is the single authority for the name: the extractor writes exactly
/// what it returns instead of re-deriving one.
fn resolve_out_name(args: &Args, repo: &str, asset_name: &str, os: &str) -> String {
    if args.no_decompress {
        return args
            .rename
            .as_deref()
            .map_or_else(|| asset_name.to_string(), |r| naming::rename_output(r, os));
    }
    naming::installed_name(args.rename.as_deref(), &resolve_lookup_name(args, repo), os)
}

fn main() -> Result<()> {
    let args = Args::parse();

    // No env file is loaded implicitly; only the explicitly requested one.
    if let Some(env_file) = args.env_file.as_deref() {
        // SAFETY: this is the first thing `main` does, so no other thread
        // exists yet that could observe the environment being mutated.
        unsafe { config::load_env_file(env_file)? };
    }

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
        Command::Config { command } => cmd_config(command),
    }
}

/// `grd register`: remember the default install directory.
/// A thin alias over `grd config install_dir set`.
fn cmd_register(path: &Path) -> Result<()> {
    cmd_install_dir(&InstallDirAction::Set {
        path: path.to_path_buf(),
    })
}

/// `grd config`: manage settings stored in the state file.
fn cmd_config(command: &ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::GithubPat { action } => cmd_github_pat(action),
        ConfigCommand::InstallDir { action } => cmd_install_dir(action),
    }
}

/// `grd config install_dir set <path>` / `grd config install_dir unset`:
/// store or remove the default install directory — the same setting that
/// `grd register <path>` writes.
fn cmd_install_dir(action: &InstallDirAction) -> Result<()> {
    let mut cache = state::State::load();
    match action {
        InstallDirAction::Set { path } => {
            store_install_dir(&mut cache, path)?;
            cache.save()?;
            println!("Default install directory set to: {}", path.display());
        }
        InstallDirAction::Unset => {
            if cache.get_default_install_dir().is_some() {
                cache.clear_default_install_dir();
                cache.save()?;
                println!("Default install directory removed from the settings file");
            } else {
                eprintln!("Warning: no default install directory stored — nothing to unset.");
            }
        }
    }
    Ok(())
}

/// Record `path` as the default install directory.
///
/// The path must be valid UTF-8: the state file holds UTF-8 only, and a
/// mangled path would send later installs to a different directory than the
/// one that was registered. The path is rejected before anything is stored,
/// so a failed call leaves the state untouched.
fn store_install_dir(cache: &mut state::State, path: &Path) -> Result<()> {
    cache
        .set_default_install_dir(path)
        .context("invalid install directory")
}

/// `grd config github_pat set <token>` / `grd config github_pat unset`:
/// store or remove the GitHub PAT. Environment variables still take
/// precedence over the stored value; the token is never echoed back.
fn cmd_github_pat(action: &GithubPatAction) -> Result<()> {
    let mut cache = state::State::load();
    match action {
        GithubPatAction::Set { token } => {
            let token = match token {
                Some(t) => t.clone(),
                // No positional: read the token from stdin so it can be
                // piped without landing in shell history or argv.
                None => read_token_from_stdin()?,
            };
            config::validate_pat_format(&token)?;
            cache.set_github_pat(&token);
            cache.save()?;
            println!("GitHub PAT saved to the settings file");
        }
        GithubPatAction::Unset => {
            if cache.get_github_pat().is_some() {
                cache.clear_github_pat();
                cache.save()?;
                println!("GitHub PAT removed from the settings file");
            } else {
                eprintln!("Warning: no GitHub PAT stored — nothing to unset.");
            }
        }
    }
    Ok(())
}

/// Read a PAT from stdin (a single line, trimmed) when `set` is called
/// without a positional token.
fn read_token_from_stdin() -> Result<String> {
    if std::io::stdin().is_terminal() {
        // Prompt on stderr so piped invocations stay clean while
        // interactive users see that input is expected.
        eprint!("Paste token: ");
    }
    read_token_from_reader(&mut std::io::stdin().lock())
}

fn read_token_from_reader(input: &mut impl BufRead) -> Result<String> {
    let mut line = String::new();
    input
        .read_line(&mut line)
        .context("failed to read token from stdin")?;
    let token = line.trim().to_string();
    if token.is_empty() {
        bail!("no token provided; pass it as an argument or pipe it to stdin");
    }
    Ok(token)
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

        // The binary is already gone, so a failed state update must surface:
        // the cache would otherwise keep claiming an install that is gone.
        cache
            .save()
            .context("binary removed but failed to update the state file")?;
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
    // One load feeds the destination, the auth token and the cached tag;
    // `install_release` reloads on its own after the download completes.
    let state = state::State::load();
    let dest = resolve_dest(args, &state)?;
    let (agent, repo) = prepare_request(args, state.get_github_pat())?;

    if args.list {
        return list_available_releases(&agent, repo);
    }

    let cached_tag = state.get_cached(repo).map(|cached| cached.tag.clone());
    let release = match args.tag.as_deref() {
        Some(tag) => github::fetch_release_info(&agent, repo, Some(tag))?,
        None => github::fetch_upgrade_release(
            &agent,
            repo,
            cached_tag.as_deref(),
            state.get_stable_scan_limit(),
        )?,
    };
    if args.tag.is_none()
        && let Some(installed_tag) = cached_tag.as_deref()
        && github::is_downgrade(&release.tag_name, installed_tag)
    {
        bail!(
            "refusing to downgrade {repo} from {installed_tag} to {}; use --tag to install a specific version",
            release.tag_name
        );
    }
    println!("Selected version: {}", release.tag_name);

    let (os, arch) = detect_platform(args)?;
    let asset = pick_asset(args, &release, &os, &arch)?;
    println!("Selected asset: {}", asset.name);

    install_release(
        args,
        &agent,
        repo,
        &dest,
        &release,
        &asset,
        naming::Platform::new(&os, &arch),
    )
}

/// Destination directory: `--destination`, else the registered default, else
/// the platform default. Created unless `--dry-run`.
fn resolve_dest(args: &Args, state: &state::State) -> Result<PathBuf> {
    let dest = match &args.destination {
        Some(d) => d.clone(),
        None => state
            .get_default_install_dir()
            .map(PathBuf::from)
            .unwrap_or_else(state::State::default_install_path),
    };
    // The destination ends up in the state file, which holds UTF-8 only:
    // reject a path with non-UTF-8 bytes before touching the filesystem.
    state::storable_path(&dest).context("invalid destination")?;
    if !args.dry_run {
        fs::create_dir_all(&dest).context("Failed to create destination directory")?;
    }
    Ok(dest)
}

/// Build the API client and pass the remaining local validations before any
/// network work: a rejected value must not cost the user a release lookup or
/// an asset download.
fn prepare_request<'a>(args: &'a Args, file_pat: Option<&str>) -> Result<(Agent, &'a str)> {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = config::get_auth_token(file_pat);
    let agent = config::configure_agent(&ua, token.as_deref());

    let repo = args
        .repo
        .as_deref()
        .context("a repo argument is required")?;
    if let Some(rename) = &args.rename {
        validate_rename(rename)?;
    }
    if let Some(bin_name) = &args.bin_name {
        validate_bin_name(bin_name)?;
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
    platform: naming::Platform<'_>,
) -> Result<()> {
    let cache = state::State::load();
    let cached = cache.get_cached(repo);

    let lookup_name = resolve_lookup_name(args, repo);
    // The output name follows the lookup name (`--rename` > `--bin-name` >
    // repository basename), so it is known before anything is downloaded —
    // and it is the single source of truth: the extractor writes exactly
    // this name instead of re-deriving one.
    let out_name = resolve_out_name(args, repo, &asset.name, platform.os);
    let installed_path = dest.join(&out_name);
    let up_to_date = is_up_to_date(args, cached, release, asset, &installed_path);

    if args.dry_run {
        return dry_run_report(dest, repo, release, asset, &out_name, up_to_date);
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

    let out_file =
        download_and_extract(args, agent, dest, asset, platform, &lookup_name, &out_name)?;

    let mut cache = state::State::load();
    cache
        .set_cached(repo, &asset.name, &release.tag_name, dest, &out_file)
        .context("failed to record the install in the state file")?;
    // The binary is already in place; still fail loudly so the user knows
    // this install will not be tracked (and so will re-download next time).
    cache
        .save()
        .context("binary installed but failed to record it in the state file")?;

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
    platform: naming::Platform<'_>,
    lookup_name: &str,
    out_name: &str,
) -> Result<String> {
    let source = download::download_asset(agent, asset, args.memory_limit)?;
    extract::extract_and_save(
        source,
        &asset.name,
        lookup_name,
        out_name,
        dest,
        args.no_decompress,
        platform,
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
fn install_exists(installed_path: &Path) -> bool {
    installed_path.exists()
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
    installed_path: &Path,
) -> bool {
    !is_forced(args) && is_same_version(cached, release, asset) && install_exists(installed_path)
}

/// True when an existing install should be confirmed interactively before
/// being upgraded.
fn needs_upgrade_confirmation(args: &Args, installed_path: &Path) -> bool {
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
    fn test_read_token_from_reader_trims_input() {
        let mut input = std::io::Cursor::new("ghp_token_value\n");
        assert_eq!(
            read_token_from_reader(&mut input).unwrap(),
            "ghp_token_value"
        );
    }

    #[test]
    fn test_read_token_from_reader_rejects_empty_line() {
        let mut input = std::io::Cursor::new("\n");
        let err = read_token_from_reader(&mut input).unwrap_err().to_string();
        assert!(err.contains("no token provided"), "got: {err}");
    }

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
    fn test_resolve_out_name_rename_appends_exe_on_windows() {
        let args = Args::parse_from(["grd", "owner/prj", "--rename", "tool"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "windows"),
            "tool.exe"
        );
    }

    #[test]
    fn test_resolve_out_name_rename_verbatim_off_windows() {
        let args = Args::parse_from(["grd", "owner/prj", "--rename", "tool"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "tool"
        );
    }

    #[test]
    fn test_resolve_out_name_no_decompress_uses_asset_name() {
        let args = Args::parse_from(["grd", "owner/prj", "--no-decompress"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "asset.tar.gz"
        );
    }

    #[test]
    fn test_resolve_out_name_defaults_to_repo_basename() {
        let args = Args::parse_from(["grd", "owner/prj"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "prj"
        );
    }

    #[test]
    fn test_resolve_out_name_uses_bin_name() {
        let args = Args::parse_from(["grd", "owner/prj", "--bin-name", "app-bin"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "app-bin"
        );
    }

    #[test]
    fn test_resolve_out_name_bin_name_gets_exe_on_windows() {
        let args = Args::parse_from(["grd", "owner/prj", "--bin-name", "app-bin"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "windows"),
            "app-bin.exe"
        );
    }

    #[test]
    fn test_resolve_out_name_rename_wins_over_bin_name() {
        let args = Args::parse_from([
            "grd",
            "owner/prj",
            "--bin-name",
            "app-bin",
            "--rename",
            "other",
        ]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "other"
        );
    }

    #[test]
    fn test_resolve_out_name_bin_name_path_uses_last_component() {
        let args = Args::parse_from(["grd", "owner/prj", "--bin-name", "pkg/bin/app"]);
        assert_eq!(
            resolve_out_name(&args, "owner/prj", "asset.tar.gz", "linux"),
            "app"
        );
    }

    #[test]
    fn test_store_install_dir_stores_utf8_path() {
        let mut cache = state::State::default();
        store_install_dir(&mut cache, Path::new("/opt/bin")).unwrap();
        assert_eq!(cache.get_default_install_dir(), Some("/opt/bin"));
    }

    #[cfg(unix)]
    #[test]
    fn test_store_install_dir_rejects_non_utf8_path() {
        use std::os::unix::ffi::OsStrExt;

        let mut cache = state::State::default();
        let bad = PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/d/b\xFFin"));
        let err = store_install_dir(&mut cache, &bad).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("UTF-8"), "got: {msg}");
        assert!(
            cache.get_default_install_dir().is_none(),
            "a rejected path must not be stored"
        );
    }

    #[test]
    fn test_resolve_dest_creates_the_destination() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("bin");
        let args = Args::parse_from(["grd", "owner/repo", "-d", dest.to_str().unwrap()]);

        let resolved = resolve_dest(&args, &state::State::default()).unwrap();
        assert_eq!(resolved, dest);
        assert!(dest.is_dir(), "the destination must be created");
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_dest_rejects_non_utf8_destination() {
        use std::{ffi::OsString, os::unix::ffi::OsStrExt};

        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp
            .path()
            .join(OsString::from(std::ffi::OsStr::from_bytes(b"b\xFFin")));
        let args = Args::parse_from([
            OsString::from("grd"),
            OsString::from("owner/repo"),
            OsString::from("-d"),
            dest.clone().into_os_string(),
        ]);

        let err = resolve_dest(&args, &state::State::default()).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("UTF-8"), "got: {msg}");
        assert!(
            !dest.exists(),
            "a rejected destination must not be created on disk"
        );
    }

    #[test]
    fn test_validate_bin_name_accepts_plain_names_and_paths() {
        assert!(validate_bin_name("app-bin").is_ok());
        assert!(validate_bin_name("pkg/bin/app").is_ok());
    }

    #[test]
    fn test_validate_bin_name_rejects_paths_that_escape() {
        for bad in ["..", ".", "/", "\\", "a/../.."] {
            let err = validate_bin_name(bad).unwrap_err().to_string();
            assert!(
                err.contains("--bin-name"),
                "error for '{bad}' should name the flag: {err}"
            );
        }
    }
}
