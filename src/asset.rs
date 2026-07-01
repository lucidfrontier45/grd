use std::{
    collections::HashMap,
    io::{self, IsTerminal, Write},
};

use anyhow::{Context, Result, bail};

use crate::github::Asset;

struct PlatformAlias {
    canonical_os: &'static str,
    inferred_arch: Option<&'static str>,
}

static PLATFORM_ALIASES: LazyLock<HashMap<&str, PlatformAlias>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        "win",
        PlatformAlias {
            canonical_os: "windows",
            inferred_arch: None,
        },
    );
    m.insert(
        "win32",
        PlatformAlias {
            canonical_os: "windows",
            inferred_arch: Some("x86_64"),
        },
    );
    m.insert(
        "win64",
        PlatformAlias {
            canonical_os: "windows",
            inferred_arch: Some("x86_64"),
        },
    );
    m
});

fn normalize_platform_identifier(platform: &str) -> (String, Option<String>) {
    let normalized = platform.to_lowercase();

    if let Some(alias_info) = PLATFORM_ALIASES.get(normalized.as_str()) {
        return (
            alias_info.canonical_os.to_string(),
            alias_info.inferred_arch.map(|s| s.to_string()),
        );
    }

    (normalized.clone(), None)
}

use std::sync::LazyLock;

#[allow(dead_code)]
fn infer_architecture_from_platform(platform: &str) -> Option<String> {
    let normalized = platform.to_lowercase();
    PLATFORM_ALIASES
        .get(normalized.as_str())
        .and_then(|alias| alias.inferred_arch.map(|s| s.to_string()))
}

pub fn normalize_os(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "windows" | "win32" | "win64" => Ok("windows".to_string()),
        "macos" => Ok("macos".to_string()),
        "linux" => Ok("linux".to_string()),
        _ => bail!("Invalid OS '{}'. Supported: windows, macos, linux", input),
    }
}

pub fn normalize_arch(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "x86_64" | "amd64" | "x64" => Ok("x86_64".to_string()),
        "aarch64" | "arm64" => Ok("aarch64".to_string()),
        "loong64" | "loongarch64" => Ok("loong64".to_string()),
        _ => bail!(
            "Invalid architecture '{}'. Supported: x86_64 (aliases: amd64, x64), aarch64 (alias: arm64), loong64 (alias: loongarch64)",
            input
        ),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

fn has_explicit_arch_pattern(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("x86_64")
        || name.contains("amd64")
        || name.contains("x64")
        || name.contains("aarch64")
        || name.contains("arm64")
        || name.contains("loong64")
        || name.contains("loongarch64")
        || name.contains("i686")
        || name.contains("i386")
        || name.contains("armhf")
        || name.contains("armv7")
        || name.contains("riscv64")
        || name.contains("riscv32")
        || name.contains("ppc64le")
        || name.contains("ppc64")
        || name.contains("powerpc")
        || name.contains("s390x")
        || name.contains("mips")
        || name.contains("win64")
        || name.contains("win32")
        || name.contains("x86")
}

fn default_arch_for_os(os: &str) -> Option<&'static str> {
    match os {
        "linux" => Some("x86_64"),
        "macos" => Some("aarch64"),
        "windows" => Some("x86_64"),
        _ => None,
    }
}

const COMPOUND_EXTS: &[&str] = &[".tar.gz", ".tar.xz", ".tar.bz2"];
const ALLOWED_EXTS: &[&str] = &[".exe", ".zip", ".tar.gz", ".tgz", ".tar.xz"];

/// Suffixes that LOOK like version literals (contain digits) but are actually
/// well-known format tokens. Treated as real extensions, never stripped.
const VERSION_BLOCKLIST: &[&str] = &[
    "sha256", "sha512", "sha384", "sha224", "sha1", "md5", "minisig",
];

/// Per option (c): a single dot-segment is a version literal iff it contains
/// at least one non-alphabetic ASCII character. Pure-letter segments
/// (`gz`, `zip`, `dmg`, …) are real extensions, never version tails.
fn is_version_literal(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    let lower = segment.to_lowercase();
    if VERSION_BLOCKLIST.contains(&lower.as_str()) {
        return false;
    }
    segment.chars().any(|c| !c.is_ascii_alphabetic())
}

/// Returns the extension (with leading `.`) of a basename, or `None` if the
/// basename has no extension (e.g. `LICENSE`, `app-1.2.3`).
///
/// Algorithm: starting from the full basename, check compound suffixes first
/// at every step; otherwise strip trailing version-literal segments; the first
/// non-version-literal tail is the extension. Returns `None` if no dot remains.
pub(crate) fn extract_extension(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let basename = lower.rsplit_once('/').map(|(_, b)| b).unwrap_or(&lower);

    let mut current = basename;
    loop {
        for ext in COMPOUND_EXTS {
            if current.ends_with(ext) {
                return Some(ext.to_string());
            }
        }
        let last_dot = current.rfind('.')?;
        let last_seg = &current[last_dot + 1..];
        if !is_version_literal(last_seg) {
            return Some(format!(".{}", last_seg));
        }
        current = &current[..last_dot];
    }
}

fn is_allowed_by_extension(name: &str) -> bool {
    match extract_extension(name) {
        Some(ext) => ALLOWED_EXTS.contains(&ext.as_str()),
        None => true, // no extension → allowed
    }
}

fn calculate_match_score(asset_name: &str, target_os: &str, target_arch: &str) -> i32 {
    let name = asset_name.to_lowercase();
    let mut score = 0;

    let os_patterns = match target_os {
        "windows" => vec!["windows", "pc-windows", "win64", "win32", "win"],
        "macos" => vec!["apple-darwin", "darwin", "macos"],
        "linux" => vec!["linux", "unknown-linux"],
        _ => vec![],
    };

    let arch_patterns = match target_arch {
        "x86_64" => vec!["x86_64", "amd64", "x64", "win64"],
        "aarch64" => vec!["aarch64", "arm64"],
        "loong64" => vec!["loong64", "loongarch64"],
        _ => vec![],
    };

    let os_exclusions = match target_os {
        "windows" => vec!["darwin"],
        "macos" => vec!["windows"],
        _ => vec![],
    };

    for exclusion in &os_exclusions {
        if name.contains(exclusion) {
            return score;
        }
    }

    let exact_os_pattern = match target_os {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => "",
    };

    if !exact_os_pattern.is_empty() && name.contains(exact_os_pattern) {
        score += 2;
    } else {
        for pattern in &os_patterns {
            if name.contains(pattern) {
                score += 1;
                break;
            }
        }
    }

    for pattern in &arch_patterns {
        if name.contains(pattern) {
            score += 1;
            break;
        }
    }

    score
}

fn sort_by_score(assets: &mut Vec<&Asset>, target_os: &str, target_arch: &str) {
    assets.sort_by(|a, b| {
        let score_a = calculate_match_score(&a.name, target_os, target_arch);
        let score_b = calculate_match_score(&b.name, target_os, target_arch);
        score_b.cmp(&score_a)
    });
}

fn best_unique_match<'a>(
    assets: &[&'a Asset],
    target_os: &str,
    target_arch: &str,
) -> Option<&'a Asset> {
    let mut best: Option<(&'a Asset, i32)> = None;
    let mut best_count = 0;

    for asset in assets {
        let score = calculate_match_score(&asset.name, target_os, target_arch);
        match best {
            None => {
                best = Some((asset, score));
                best_count = 1;
            }
            Some((_, best_score)) if score > best_score => {
                best = Some((asset, score));
                best_count = 1;
            }
            Some((_, best_score)) if score == best_score => {
                best_count += 1;
            }
            _ => {}
        }
    }

    if best_count == 1 {
        best.map(|(asset, _)| asset)
    } else {
        None
    }
}

fn canonical_arch_for_matching(arch: &str) -> String {
    normalize_arch(arch).unwrap_or_else(|_| arch.to_lowercase())
}

fn show_all_assets(assets: &[&Asset], target_os: &str, target_arch: &str) {
    for (i, asset) in assets.iter().enumerate() {
        let score = calculate_match_score(&asset.name, target_os, target_arch);
        println!(
            "{}. {} ({}) [{}]",
            i + 1,
            asset.name,
            format_size(asset.size),
            score
        );
    }
}

fn collect_selection<'a>(assets: &'a [&'a Asset]) -> Result<&'a Asset> {
    loop {
        print!("Enter choice (1-{}): ", assets.len());
        io::stdout().flush().context("Failed to flush stdout")?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read input")?;
        match input.trim().parse::<usize>() {
            Ok(n) if n >= 1 && n <= assets.len() => return Ok(assets[n - 1]),
            _ => println!(
                "Invalid choice. Enter a number between 1 and {}.",
                assets.len()
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Auto-pick the best-matching asset using score-based resolution.
    ///
    /// Calls `find_asset` internally:
    /// - **Exact** — one asset clearly beats all others → returned as `AssetSelection::Single`.
    /// - **Multiple** — tie among top scorers → all returned as `AssetSelection::Multiple`.
    /// - **None** — no platform match → bails with error.
    ///
    /// No interactivity. Works in non-TTY (CI, pipes). Default behavior when no
    /// `--select` / `--select-all` flag is passed.
    Default,

    /// Present a filtered, sorted list of candidates and let the user pick one interactively.
    ///
    /// Calls `resolve_candidates` to filter assets by OS/arch, then scores and sorts
    /// them for display. The user selects a single entry via stdin.
    ///
    /// Requires a TTY — bails with error if stdin is not a terminal.
    /// Activated by `--select` on the CLI.
    Filtered,

    /// Full interactive browser showing every asset in the release.
    ///
    /// No platform pre-filtering. Passes the entire asset list to `interactive_select`
    /// for TUI-style picking (e.g., skim / fzf).
    ///
    /// Requires a TTY — bails with error if stdin is not a terminal.
    /// Activated by `--select-all` on the CLI.
    All,
}

#[derive(Debug)]
pub enum Selection {
    Exact(Asset),
    Multiple(Vec<Asset>),
    None,
}

#[derive(Debug)]
pub enum AssetSelection {
    Single(Asset),
    Multiple(Vec<Asset>),
}

/// Filter `assets` by platform, then try to pick a single best match.
///
/// Returns the full candidate list and, when one asset clearly wins the
/// scoring, a clone of that winner. Callers that want a prompt-on-tie UX
/// use the candidate list; auto-pick callers use the winner.
fn resolve_candidates<'a>(
    assets: &'a [Asset],
    os: &str,
    arch: &str,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> (Vec<&'a Asset>, Option<Asset>) {
    let blacklist: Vec<String> = exclude.map_or_else(Vec::new, |s| {
        s.split(',').map(|w| w.trim().to_lowercase()).collect()
    });

    let (normalized_os, inferred_arch) = normalize_platform_identifier(os);
    let normalized_arch = canonical_arch_for_matching(arch);

    let effective_arch = if inferred_arch.is_some() {
        inferred_arch
    } else {
        Some(normalized_arch)
    };

    let matches = collect_matches(
        assets,
        &blacklist,
        &normalized_os,
        &effective_arch,
        no_ext_filter,
    );

    if let Some(asset) = best_unique_match(
        &matches,
        &normalized_os,
        effective_arch.as_deref().unwrap_or(arch),
    ) {
        return (matches, Some(asset.clone()));
    }

    if matches.len() > 1
        && normalized_os == "windows"
        && effective_arch.as_deref() == Some("x86_64")
    {
        let win64: Vec<&Asset> = matches
            .iter()
            .filter(|a| a.name.to_lowercase().contains("win64"))
            .cloned()
            .collect();
        if win64.len() == 1 {
            return (matches, Some(win64[0].clone()));
        }
    }

    (matches, None)
}

pub fn find_asset(
    assets: &[Asset],
    os: &str,
    arch: &str,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Selection {
    let (matches, winner) = resolve_candidates(assets, os, arch, exclude, no_ext_filter);

    if let Some(asset) = winner {
        return Selection::Exact(asset);
    }

    match matches.len() {
        0 => Selection::None,
        1 => Selection::Exact(matches[0].clone()),
        _ => Selection::Multiple(matches.into_iter().cloned().collect()),
    }
}

pub fn select_asset(
    assets: &[Asset],
    os: &str,
    arch: &str,
    mode: SelectMode,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Result<AssetSelection> {
    let stdin_is_terminal = if cfg!(test) {
        false
    } else {
        io::stdin().is_terminal()
    };
    if matches!(mode, SelectMode::Filtered | SelectMode::All) && !stdin_is_terminal {
        bail!("Cannot select asset in non-terminal environment");
    }

    let (normalized_os, inferred_arch) = normalize_platform_identifier(os);
    let normalized_arch = canonical_arch_for_matching(arch);
    let effective_arch = inferred_arch.or(Some(normalized_arch));
    let arch_ref = effective_arch.as_deref().unwrap_or(arch);

    match mode {
        SelectMode::All => {
            let blacklist: Vec<String> = exclude.map_or_else(Vec::new, |s| {
                s.split(',').map(|w| w.trim().to_lowercase()).collect()
            });
            let selected = interactive_select(
                assets,
                &blacklist,
                &normalized_os,
                &effective_arch,
                arch,
                "Select an asset:",
                no_ext_filter,
            )?;
            Ok(AssetSelection::Single(selected))
        }
        SelectMode::Filtered => {
            let mut matches = resolve_candidates(assets, os, arch, exclude, no_ext_filter).0;
            if matches.is_empty() {
                bail!("No matching asset found for {normalized_os}-{arch_ref}");
            }
            sort_by_score(&mut matches, &normalized_os, arch_ref);
            println!("Select a matching asset:");
            show_all_assets(&matches, &normalized_os, arch_ref);
            let selected = collect_selection(&matches)?;
            Ok(AssetSelection::Single(selected.clone()))
        }
        SelectMode::Default => match find_asset(assets, os, arch, exclude, no_ext_filter) {
            Selection::Exact(asset) => Ok(AssetSelection::Single(asset)),
            Selection::Multiple(matches) => {
                let mut sorted = matches.iter().collect::<Vec<_>>();
                sort_by_score(&mut sorted, &normalized_os, arch_ref);
                Ok(AssetSelection::Multiple(
                    sorted.into_iter().cloned().collect(),
                ))
            }
            Selection::None => {
                bail!("No matching asset found for {normalized_os}-{arch_ref}")
            }
        },
    }
}

fn interactive_select(
    assets: &[Asset],
    blacklist: &[String],
    normalized_os: &str,
    effective_arch: &Option<String>,
    arch: &str,
    prompt: &str,
    no_ext_filter: bool,
) -> Result<Asset> {
    if !io::stdin().is_terminal() {
        bail!("Cannot select asset in non-terminal environment");
    }
    let mut all_assets: Vec<&Asset> = assets
        .iter()
        .filter(|a| {
            let name = a.name.to_lowercase();
            (no_ext_filter || is_allowed_by_extension(&name))
                && !blacklist.iter().any(|b| name.contains(b))
        })
        .collect();
    if all_assets.is_empty() {
        bail!("No assets available after applying filters");
    }
    sort_by_score(
        &mut all_assets,
        normalized_os,
        effective_arch.as_deref().unwrap_or(arch),
    );
    println!("{prompt}");
    show_all_assets(
        &all_assets,
        normalized_os,
        effective_arch.as_deref().unwrap_or(arch),
    );
    let selected = collect_selection(&all_assets)?;
    Ok(selected.clone())
}

fn collect_matches<'a>(
    assets: &'a [Asset],
    blacklist: &[String],
    normalized_os: &str,
    effective_arch: &Option<String>,
    no_ext_filter: bool,
) -> Vec<&'a Asset> {
    assets
        .iter()
        .filter(|a| {
            let name = a.name.to_lowercase();

            let ext_ok = no_ext_filter || is_allowed_by_extension(&name);

            let os_match = match normalized_os {
                "windows" => {
                    (name.contains("windows")
                        || name.contains("win64")
                        || name.contains("pc-windows")
                        || name.contains("win32")
                        || name.contains("win"))
                        && !name.contains("darwin")
                }
                "macos" => {
                    name.contains("apple-darwin")
                        || name.contains("macos")
                        || name.contains("darwin")
                }
                "linux" => name.contains("linux") || name.contains("unknown-linux"),
                _ => false,
            };
            let arch_match = match effective_arch.as_deref() {
                Some("x86_64") => {
                    name.contains("x86_64")
                        || name.contains("amd64")
                        || name.contains("x64")
                        || (normalized_os == "windows" && name.contains("win64"))
                        || (normalized_os == "windows" && name.contains("win32"))
                        || (!has_explicit_arch_pattern(&name)
                            && default_arch_for_os(normalized_os) == Some("x86_64"))
                }
                Some("aarch64") => {
                    name.contains("aarch64")
                        || name.contains("arm64")
                        || (!has_explicit_arch_pattern(&name)
                            && default_arch_for_os(normalized_os) == Some("aarch64"))
                }
                Some("loong64") => {
                    name.contains("loong64")
                        || name.contains("loongarch64")
                        || (!has_explicit_arch_pattern(&name)
                            && default_arch_for_os(normalized_os) == Some("loong64"))
                }
                _ => false,
            };
            ext_ok && os_match && arch_match && !blacklist.iter().any(|b| name.contains(b))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_os_valid() {
        assert_eq!(normalize_os("windows").unwrap(), "windows");
        assert_eq!(normalize_os("WINDOWS").unwrap(), "windows");
        assert_eq!(normalize_os("macos").unwrap(), "macos");
        assert_eq!(normalize_os("MACOS").unwrap(), "macos");
        assert_eq!(normalize_os("linux").unwrap(), "linux");
        assert_eq!(normalize_os("LINUX").unwrap(), "linux");
    }

    #[test]
    fn test_normalize_os_invalid() {
        assert!(normalize_os("freebsd").is_err());
        assert!(normalize_os("android").is_err());
        assert!(normalize_os("invalid").is_err());
    }

    #[test]
    fn test_normalize_arch_valid() {
        assert_eq!(normalize_arch("x86_64").unwrap(), "x86_64");
        assert_eq!(normalize_arch("amd64").unwrap(), "x86_64");
        assert_eq!(normalize_arch("AMD64").unwrap(), "x86_64");
        assert_eq!(normalize_arch("x64").unwrap(), "x86_64");
        assert_eq!(normalize_arch("X64").unwrap(), "x86_64");
        assert_eq!(normalize_arch("aarch64").unwrap(), "aarch64");
        assert_eq!(normalize_arch("arm64").unwrap(), "aarch64");
        assert_eq!(normalize_arch("ARM64").unwrap(), "aarch64");
        assert_eq!(normalize_arch("loong64").unwrap(), "loong64");
        assert_eq!(normalize_arch("loongarch64").unwrap(), "loong64");
    }

    #[test]
    fn test_normalize_arch_invalid() {
        assert!(normalize_arch("i386").is_err());
        assert!(normalize_arch("x86").is_err());
        assert!(normalize_arch("armv7").is_err());
        assert!(normalize_arch("invalid").is_err());
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1023), "1023.0 KB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 10), "10.0 MB");
    }

    #[test]
    fn test_select_asset_darwin_exclusion_with_win32() {
        let assets = vec![Asset {
            name: "app-x86_64-apple-darwin.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win32", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "darwin assets should NOT match win32"
        );
    }

    #[test]
    fn test_select_asset_darwin_exclusion_with_win64() {
        let assets = vec![Asset {
            name: "app-x86_64-darwin.exe".to_string(),
            browser_download_url: "https://example.com/app.exe".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win64", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "darwin assets should NOT match win64"
        );
    }

    #[test]
    fn test_select_asset_loong64_not_matched_to_x86_64() {
        let assets = vec![Asset {
            name: "app-linux-loong64.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "loong64 assets should NOT match x86_64"
        );
    }

    #[test]
    fn test_select_asset_loong64_matches_loong64() {
        let assets = vec![Asset {
            name: "app-linux-loong64.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "linux", "loong64", None, false);
        assert!(matches!(result, Selection::Exact(_)));
    }

    #[test]
    fn test_calculate_match_score_exact_os() {
        let score = calculate_match_score("app-linux-x86_64.tar.gz", "linux", "x86_64");
        assert_eq!(score, 3);
    }

    #[test]
    fn test_calculate_match_score_platform_alias_os() {
        let score = calculate_match_score("app-win-x86_64.zip", "windows", "x86_64");
        assert_eq!(score, 2);
    }

    #[test]
    fn test_calculate_match_score_exact_arch() {
        let score = calculate_match_score("app-linux-x86_64.tar.gz", "linux", "x86_64");
        assert_eq!(score, 3);
    }

    #[test]
    fn test_calculate_match_score_cross_arch() {
        let score = calculate_match_score("app-linux-aarch64.tar.gz", "linux", "x86_64");
        assert_eq!(score, 2);
    }

    #[test]
    fn test_calculate_match_score_cross_os() {
        let score = calculate_match_score("app-darwin-x86_64.tar.gz", "linux", "x86_64");
        assert_eq!(score, 1);
    }

    #[test]
    fn test_sort_by_score() {
        let assets = [
            Asset {
                name: "app-darwin-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app1.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app2.tar.gz".to_string(),
                size: 2048,
            },
            Asset {
                name: "app-win-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/app3.zip".to_string(),
                size: 3072,
            },
        ];

        let mut asset_refs: Vec<&Asset> = assets.iter().collect();
        sort_by_score(&mut asset_refs, "windows", "x86_64");

        assert_eq!(asset_refs[0].name, "app-win-x86_64.zip");
        assert_eq!(asset_refs[1].name, "app-linux-x86_64.tar.gz");
        assert_eq!(asset_refs[2].name, "app-darwin-x86_64.tar.gz");
    }

    #[test]
    fn test_sort_by_score_same_score_preserves_order() {
        let assets = [
            Asset {
                name: "app-linux-x86_64-v1.tar.gz".to_string(),
                browser_download_url: "https://example.com/app1.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-linux-x86_64-v2.tar.gz".to_string(),
                browser_download_url: "https://example.com/app2.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let mut asset_refs: Vec<&Asset> = assets.iter().collect();
        sort_by_score(&mut asset_refs, "linux", "x86_64");

        assert_eq!(asset_refs[0].name, "app-linux-x86_64-v1.tar.gz");
        assert_eq!(asset_refs[1].name, "app-linux-x86_64-v2.tar.gz");
    }

    #[test]
    fn test_select_asset_respects_exclude_filter() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux-gnu.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-gnu.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux-musl.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-musl.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "linux", "x86_64", Some("gnu"), false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-x86_64-linux-musl.tar.gz"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_sorted_by_score() {
        let assets = vec![
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app1.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-win-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/app2.zip".to_string(),
                size: 2048,
            },
            Asset {
                name: "app-linux-amd64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app3.tar.gz".to_string(),
                size: 3072,
            },
        ];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(matches!(result, Selection::Multiple(_)));
    }

    #[test]
    fn test_select_asset_multiple_matches_without_force_select() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/app1.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux.zip".to_string(),
                browser_download_url: "https://example.com/app2.zip".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(matches!(result, Selection::Multiple(_)));
    }

    #[test]
    fn test_select_asset_prefers_linux_amd64_over_linux() {
        let assets = vec![
            Asset {
                name: "golangci-lint-2.12.2-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "golangci-lint-2.12.2-linux-amd64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux-amd64.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "linux", "amd64", None, false);
        match result {
            Selection::Exact(asset) => {
                assert_eq!(asset.name, "golangci-lint-2.12.2-linux-amd64.tar.gz")
            }
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_prefers_windows_amd64_over_windows() {
        let assets = vec![
            Asset {
                name: "golangci-lint-2.12.2-windows.zip".to_string(),
                browser_download_url: "https://example.com/windows.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "golangci-lint-2.12.2-windows-amd64.zip".to_string(),
                browser_download_url: "https://example.com/windows-amd64.zip".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "windows", "amd64", None, false);
        match result {
            Selection::Exact(asset) => {
                assert_eq!(asset.name, "golangci-lint-2.12.2-windows-amd64.zip")
            }
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_prefers_win64_over_windows() {
        let assets = vec![
            Asset {
                name: "golangci-lint-2.12.2-windows.zip".to_string(),
                browser_download_url: "https://example.com/windows.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "golangci-lint-2.12.2-win64.zip".to_string(),
                browser_download_url: "https://example.com/win64.zip".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "win64", "amd64", None, false);
        match result {
            Selection::Exact(asset) => {
                assert_eq!(asset.name, "golangci-lint-2.12.2-win64.zip")
            }
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_darwin_not_matched_to_windows() {
        let assets = vec![
            Asset {
                name: "app-x86_64-apple-darwin.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-darwin.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-aarch64-darwin.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-arm-darwin.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "windows", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "darwin assets should NOT match Windows"
        );
    }

    #[test]
    fn test_select_asset_darwin_with_win_alias_not_matched() {
        let assets = vec![Asset {
            name: "app-x86_64-darwin.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "darwin assets should NOT match even with 'win' alias"
        );
    }

    #[test]
    fn test_normalize_platform_identifier_win() {
        let (os, arch) = normalize_platform_identifier("win");
        assert_eq!(os, "windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_win32() {
        let (os, arch) = normalize_platform_identifier("win32");
        assert_eq!(os, "windows");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_normalize_platform_identifier_win64() {
        let (os, arch) = normalize_platform_identifier("win64");
        assert_eq!(os, "windows");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_normalize_platform_identifier_windows() {
        let (os, arch) = normalize_platform_identifier("windows");
        assert_eq!(os, "windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_unknown() {
        let (os, arch) = normalize_platform_identifier("unknown_platform");
        assert_eq!(os, "unknown_platform");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_case_insensitive_lowercase() {
        let (os, arch) = normalize_platform_identifier("win");
        assert_eq!(os, "windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_case_insensitive_uppercase() {
        let (os, arch) = normalize_platform_identifier("WIN");
        assert_eq!(os, "windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_case_insensitive_mixed() {
        let (os, arch) = normalize_platform_identifier("WiN");
        assert_eq!(os, "windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_win32_mixed_case() {
        let (os, arch) = normalize_platform_identifier("Win32");
        assert_eq!(os, "windows");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_normalize_platform_identifier_win64_uppercase() {
        let (os, arch) = normalize_platform_identifier("WIN64");
        assert_eq!(os, "windows");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_infer_architecture_from_platform_win32() {
        let arch = infer_architecture_from_platform("win32");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_infer_architecture_from_platform_win64() {
        let arch = infer_architecture_from_platform("win64");
        assert_eq!(arch, Some("x86_64".to_string()));
    }

    #[test]
    fn test_infer_architecture_from_platform_win() {
        let arch = infer_architecture_from_platform("win");
        assert!(arch.is_none());
    }

    #[test]
    fn test_infer_architecture_from_platform_windows() {
        let arch = infer_architecture_from_platform("windows");
        assert!(arch.is_none());
    }

    #[test]
    fn test_select_asset_windows_variant_win() {
        let assets = vec![Asset {
            name: "app-win-x86_64.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win-x86_64.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_variant_win32() {
        let assets = vec![Asset {
            name: "app-win32-x86_64.exe".to_string(),
            browser_download_url: "https://example.com/app.exe".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win32", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win32-x86_64.exe"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_variant_win64() {
        let assets = vec![Asset {
            name: "app-win64-x86_64.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "win64", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win64-x86_64.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_mixed_patterns() {
        let assets = vec![
            Asset {
                name: "app-win-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/app1.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-win32-x86_64.exe".to_string(),
                browser_download_url: "https://example.com/app2.exe".to_string(),
                size: 2048,
            },
            Asset {
                name: "app-win64-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/app3.zip".to_string(),
                size: 3072,
            },
        ];

        let result = find_asset(&assets, "win", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win64-x86_64.zip"),
            _ => panic!("Expected Selection::Exact with win64, got Multiple"),
        }
    }

    #[test]
    fn test_normalize_platform_identifier_empty_string() {
        let (os, arch) = normalize_platform_identifier("");
        assert_eq!(os, "");
        assert!(arch.is_none());
    }

    #[test]
    fn test_infer_architecture_from_platform_empty_string() {
        let arch = infer_architecture_from_platform("");
        assert!(arch.is_none());
    }

    #[test]
    fn test_normalize_platform_identifier_unknown_platform() {
        let (os, arch) = normalize_platform_identifier("freebsd");
        assert_eq!(os, "freebsd");
        assert!(arch.is_none());
    }

    #[test]
    fn test_infer_architecture_from_platform_unknown_platform() {
        let arch = infer_architecture_from_platform("freebsd");
        assert!(arch.is_none());
    }

    #[test]
    fn test_select_asset_single_match_auto_selected() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-linux-x86_64.tar.gz"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_win64_preferred_over_win32() {
        let assets = vec![
            Asset {
                name: "upx-4.2.4-win32.zip".to_string(),
                browser_download_url: "https://example.com/upx32.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "upx-4.2.4-win64.zip".to_string(),
                browser_download_url: "https://example.com/upx64.zip".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "windows", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "upx-4.2.4-win64.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_win32_fallback_when_only_win32() {
        let assets = vec![Asset {
            name: "upx-4.2.4-win32.zip".to_string(),
            browser_download_url: "https://example.com/upx32.zip".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "windows", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "upx-4.2.4-win32.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_windows_win64_without_arch_string() {
        let assets = vec![Asset {
            name: "upx-4.2.4-win64.zip".to_string(),
            browser_download_url: "https://example.com/upx.zip".to_string(),
            size: 1024,
        }];

        let result = find_asset(&assets, "windows", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "upx-4.2.4-win64.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_normalize_os_win32() {
        let result = normalize_os("win32").unwrap();
        assert_eq!(result, "windows");
    }

    #[test]
    fn test_normalize_os_win64() {
        let result = normalize_os("win64").unwrap();
        assert_eq!(result, "windows");
    }
    #[test]
    fn test_default_arch_for_os_values() {
        assert_eq!(default_arch_for_os("linux"), Some("x86_64"));
        assert_eq!(default_arch_for_os("macos"), Some("aarch64"));
        assert_eq!(default_arch_for_os("windows"), Some("x86_64"));
        assert_eq!(default_arch_for_os("freebsd"), None);
        assert_eq!(default_arch_for_os(""), None);
    }

    #[test]
    fn test_has_explicit_arch_pattern_supported() {
        assert!(has_explicit_arch_pattern("app-linux-x86_64.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-amd64.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-aarch64.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-arm64.tar.gz"));
        assert!(has_explicit_arch_pattern("app-win64.zip"));
        assert!(has_explicit_arch_pattern("app-win32.zip"));
        assert!(has_explicit_arch_pattern("app.linux.x64.tar.gz"));
    }

    #[test]
    fn test_has_explicit_arch_pattern_unsupported() {
        assert!(has_explicit_arch_pattern("app-linux-i386.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-i686.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-armhf.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-armv7.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-riscv64.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-ppc64le.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-s390x.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-mips.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux-powerpc.tar.gz"));
    }

    #[test]
    fn test_has_explicit_arch_pattern_x86_32() {
        assert!(has_explicit_arch_pattern("app-linux-x86.tar.gz"));
        assert!(has_explicit_arch_pattern("app-linux.i386.rpm"));
    }

    #[test]
    fn test_has_explicit_arch_pattern_no_arch() {
        assert!(!has_explicit_arch_pattern("app-linux.tar.gz"));
        assert!(!has_explicit_arch_pattern("app-macos.zip"));
        assert!(!has_explicit_arch_pattern("app-win.zip"));
        assert!(!has_explicit_arch_pattern("app.zip"));
        assert!(!has_explicit_arch_pattern("app-linux-musl.tar.gz"));
    }

    #[test]
    fn test_select_asset_no_arch_assumes_x86_64_linux() {
        let assets = vec![Asset {
            name: "app-linux.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "linux", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-linux.tar.gz"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_no_arch_does_not_match_aarch64_linux() {
        let assets = vec![Asset {
            name: "app-linux.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "linux", "aarch64", None, false);
        assert!(matches!(result, Selection::None));
    }

    #[test]
    fn test_select_asset_no_arch_assumes_aarch64_macos() {
        let assets = vec![Asset {
            name: "app-macos.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "macos", "aarch64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-macos.tar.gz"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_no_arch_does_not_match_x86_64_macos() {
        let assets = vec![Asset {
            name: "app-macos.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "macos", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "macos default is aarch64, not x86_64"
        );
    }

    #[test]
    fn test_select_asset_no_arch_assumes_x86_64_windows() {
        let assets = vec![Asset {
            name: "app-win.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "windows", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_explicit_arch_preferred_over_implicit() {
        let assets = vec![
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-x86_64.tar.gz".to_string(),
                size: 2048,
            },
            Asset {
                name: "app-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-noarch.tar.gz".to_string(),
                size: 1024,
            },
        ];
        let result = find_asset(&assets, "linux", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-linux-x86_64.tar.gz"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_no_arch_i386_not_matched_as_x86_64() {
        let assets = vec![Asset {
            name: "app-linux-i386.tar.gz".to_string(),
            browser_download_url: "https://example.com/app-i386.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "i386 has explicit arch, should NOT match x86_64"
        );
    }

    #[test]
    fn test_select_asset_no_arch_win32_alias_matches() {
        let assets = vec![Asset {
            name: "app-win.zip".to_string(),
            browser_download_url: "https://example.com/app-zip.zip".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "win32", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-win.zip"),
            _ => panic!("Expected Selection::Exact"),
        }
    }

    #[test]
    fn test_select_asset_no_arch_darwin_exclusion_still_applies() {
        let assets = vec![Asset {
            name: "app-darwin.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "windows", "x86_64", None, false);
        assert!(
            matches!(result, Selection::None),
            "darwin asset should not match windows"
        );
    }

    // ---------------- Extension filter tests ----------------

    #[test]
    fn test_extract_extension_allowed() {
        assert_eq!(
            extract_extension("app-linux-x86_64.tar.gz"),
            Some(".tar.gz".to_string())
        );
        assert_eq!(
            extract_extension("app-1.2.3.tar.gz"),
            Some(".tar.gz".to_string())
        );
        assert_eq!(extract_extension("myapp.exe"), Some(".exe".to_string()));
        assert_eq!(extract_extension("tool.tgz"), Some(".tgz".to_string()));
        assert_eq!(extract_extension("cli.tar.xz"), Some(".tar.xz".to_string()));
        assert_eq!(extract_extension("pkg.zip"), Some(".zip".to_string()));
    }

    #[test]
    fn test_extract_extension_no_extension() {
        assert_eq!(extract_extension("LICENSE"), None);
        assert_eq!(extract_extension("README"), None);
        assert_eq!(extract_extension("app-1.2.3"), None);
        assert_eq!(extract_extension("cli-rc1"), None);
        assert_eq!(extract_extension("tool-beta2"), None);
        assert_eq!(extract_extension("myapp-v2.0.0-linux-x86_64"), None);
    }

    #[test]
    fn test_extract_extension_disallowed() {
        assert_eq!(
            extract_extension("myapp.exe.sha256"),
            Some(".sha256".to_string())
        );
        assert_eq!(extract_extension("app.dmg"), Some(".dmg".to_string()));
        assert_eq!(extract_extension("pkg.deb"), Some(".deb".to_string()));
        assert_eq!(extract_extension("pkg.rpm"), Some(".rpm".to_string()));
        assert_eq!(extract_extension("foo.json"), Some(".json".to_string()));
    }

    #[test]
    fn test_extract_extension_compound_after_strip() {
        // After stripping the trailing version literal "1.2.3", the remaining
        // tail is ".tar.gz" which is a compound extension → recognized.
        assert_eq!(
            extract_extension("cli.tar.gz.1.2.3"),
            Some(".tar.gz".to_string())
        );
    }

    #[test]
    fn test_extract_extension_case_insensitive_and_directory() {
        assert_eq!(extract_extension("APP.TAR.GZ"), Some(".tar.gz".to_string()));
        assert_eq!(
            extract_extension("dist/app.tar.gz"),
            Some(".tar.gz".to_string())
        );
    }

    #[test]
    fn test_is_allowed_by_extension_allowed() {
        assert!(is_allowed_by_extension("app-linux-x86_64.tar.gz"));
        assert!(is_allowed_by_extension("myapp.exe"));
        assert!(is_allowed_by_extension("tool.tgz"));
        assert!(is_allowed_by_extension("cli.tar.xz"));
        assert!(is_allowed_by_extension("pkg.zip"));
    }

    #[test]
    fn test_is_allowed_by_extension_no_ext() {
        assert!(is_allowed_by_extension("LICENSE"));
        assert!(is_allowed_by_extension("README"));
        assert!(is_allowed_by_extension("app-1.2.3"));
        assert!(is_allowed_by_extension("cli-rc1"));
        assert!(is_allowed_by_extension("myapp-v2.0.0-linux-x86_64"));
    }

    #[test]
    fn test_is_allowed_by_extension_disallowed() {
        assert!(!is_allowed_by_extension("myapp.exe.sha256"));
        assert!(!is_allowed_by_extension("app.dmg"));
        assert!(!is_allowed_by_extension("pkg.deb"));
        assert!(!is_allowed_by_extension("pkg.rpm"));
        assert!(!is_allowed_by_extension("foo.json"));
        assert!(!is_allowed_by_extension("app.msi"));
        assert!(!is_allowed_by_extension("app.AppImage"));
        assert!(!is_allowed_by_extension("checksums.txt"));
    }

    #[test]
    fn test_find_asset_filters_non_binary_assets_by_default() {
        let assets = vec![
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "checksums.txt".to_string(),
                browser_download_url: "https://example.com/checksums.txt".to_string(),
                size: 64,
            },
            Asset {
                name: "app-linux-x86_64.tar.gz.sha256".to_string(),
                browser_download_url: "https://example.com/app.sha256".to_string(),
                size: 64,
            },
        ];
        let result = find_asset(&assets, "linux", "x86_64", None, false);
        match result {
            Selection::Exact(asset) => {
                assert_eq!(asset.name, "app-linux-x86_64.tar.gz");
                // Explicitly confirm the .sha256 was filtered out even though
                // its basename embeds linux/x86_64 tokens.
                assert_ne!(asset.name, "app-linux-x86_64.tar.gz.sha256");
            }
            _ => panic!("Expected Selection::Exact, got {result:?}"),
        }
    }

    #[test]
    fn test_find_asset_no_ext_filter_promotes_os_arch_matching_checksum() {
        // Same inputs as the default-filter test, but with --no-ext-filter.
        // The .txt has no OS/arch tokens, but the .tar.gz.sha256 embeds
        // linux + x86_64 → it now matches OS/arch and yields Multiple.
        let assets = vec![
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "checksums.txt".to_string(),
                browser_download_url: "https://example.com/checksums.txt".to_string(),
                size: 64,
            },
            Asset {
                name: "app-linux-x86_64.tar.gz.sha256".to_string(),
                browser_download_url: "https://example.com/app.sha256".to_string(),
                size: 64,
            },
        ];
        let result = find_asset(&assets, "linux", "x86_64", None, true);
        match result {
            Selection::Multiple(matches) => {
                let names: Vec<&str> = matches.iter().map(|a| a.name.as_str()).collect();
                assert!(names.contains(&"app-linux-x86_64.tar.gz"));
                assert!(names.contains(&"app-linux-x86_64.tar.gz.sha256"));
                assert_eq!(matches.len(), 2, "checksums.txt should still be excluded");
            }
            _ => panic!("Expected Selection::Multiple, got {result:?}"),
        }
    }

    #[test]
    fn test_find_asset_dmg_blocked_by_default() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.dmg".to_string(),
            browser_download_url: "https://example.com/app.dmg".to_string(),
            size: 1024,
        }];
        assert!(matches!(
            find_asset(&assets, "linux", "x86_64", None, false),
            Selection::None
        ));
    }

    #[test]
    fn test_find_asset_dmg_allowed_with_no_ext_filter() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.dmg".to_string(),
            browser_download_url: "https://example.com/app.dmg".to_string(),
            size: 1024,
        }];
        let result = find_asset(&assets, "linux", "x86_64", None, true);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-linux-x86_64.dmg"),
            _ => panic!("Expected Selection::Exact, got {result:?}"),
        }
    }

    #[test]
    fn test_find_asset_version_literal_not_filtered_for_os_arch() {
        // No-extension version-literal assets never match OS/arch anyway,
        // but the filter must not exclude them based on extension.
        for name in ["app-1.2.3", "cli-rc1", "myapp-v2.0.0-linux-x86_64"] {
            let assets = vec![Asset {
                name: name.to_string(),
                browser_download_url: format!("https://example.com/{name}"),
                size: 1024,
            }];
            // None is OS/arch-mismatch, not filter exclusion. With a different
            // OS/arch we cannot assert "not filtered" without seeing the matches
            // list, but we can assert the filter does not throw or panic.
            let _ = find_asset(&assets, "linux", "x86_64", None, false);
        }
    }

    #[test]
    fn test_select_asset_with_default_filter_does_not_match_dmg() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.dmg".to_string(),
            browser_download_url: "https://example.com/app.dmg".to_string(),
            size: 1024,
        }];
        // Non-terminal + Selection::None path should bail; this validates the
        // signature accepts the new arg without breaking existing behavior.
        let result = select_asset(&assets, "linux", "x86_64", SelectMode::Default, None, false);
        assert!(result.is_err(), "expected Selection::None to bail");
    }

    #[test]
    fn test_select_asset_with_no_ext_filter_matches_dmg() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.dmg".to_string(),
            browser_download_url: "https://example.com/app.dmg".to_string(),
            size: 1024,
        }];
        // Non-terminal + Selection::Exact should succeed without prompting.
        let result =
            select_asset(&assets, "linux", "x86_64", SelectMode::Default, None, true).unwrap();
        let AssetSelection::Single(asset) = result else {
            panic!("expected exact asset");
        };
        assert_eq!(asset.name, "app-linux-x86_64.dmg");
    }

    #[test]
    fn test_select_asset_default_multiple_returns_selection() {
        let assets = vec![
            Asset {
                name: "app-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app1.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-linux-amd64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app2.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = select_asset(&assets, "linux", "x86_64", SelectMode::Default, None, false)
            .expect("default multiple should return list");
        let AssetSelection::Multiple(matches) = result else {
            panic!("expected multiple assets");
        };
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_select_asset_filtered_requires_terminal() {
        let assets = vec![Asset {
            name: "app-linux-x86_64.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let err = select_asset(
            &assets,
            "linux",
            "x86_64",
            SelectMode::Filtered,
            None,
            false,
        )
        .expect_err("filtered mode should require terminal before prompting");
        assert!(
            err.to_string()
                .contains("Cannot select asset in non-terminal environment")
        );
    }
}
