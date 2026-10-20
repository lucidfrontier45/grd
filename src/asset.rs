use std::{
    cmp::Ordering,
    collections::HashMap,
    io::{self, IsTerminal, Write},
    sync::LazyLock,
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

#[cfg(test)]
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

// --------------- OS/arch vocabulary ---------------
//
// One set of token tables shared by the candidate filter (`collect_matches`)
// and the scorer (`calculate_match_score`). Only the vocabulary is shared:
// the filter is inclusive/existential while the scorer is weighted with
// cross-OS exclusions and a musl bonus, so each consumer keeps its own rules
// on top of these lists.

/// Canonical arch → tokens naming it in a release asset name. The scorer uses
/// these as-is; the candidate filter applies [`WINDOWS_X86_64_TOKENS`] on top.
static ARCH_ALIASES: &[(&str, &[&str])] = &[
    ("x86_64", &["x86_64", "amd64", "x64", "win64"]),
    ("aarch64", &["aarch64", "arm64"]),
    ("loong64", &["loong64", "loongarch64"]),
];

/// Word-size tokens naming `x86_64`, which appear only in windows asset
/// names (`win64`/`win32` describe the Windows API). The candidate filter
/// counts them for windows targets only; the scorer historically counts
/// `win64` on every OS and `win32` on none — kept so scores stay identical.
static WINDOWS_X86_64_TOKENS: &[&str] = &["win64", "win32"];

/// Arch tokens of archs `grd` does not target but that appear in release
/// names. They still count as an explicit arch, blocking the default-arch
/// fallback.
static OTHER_ARCH_TOKENS: &[&str] = &[
    "i686", "i386", "x86", "armhf", "armv7", "riscv64", "riscv32", "ppc64le", "ppc64", "powerpc",
    "s390x", "mips",
];

/// EABI triples (`gnueabi`/`musleabi`, with or without `hf`) name 32-bit ARM.
static ARM_EABI_TOKENS: &[&str] = &["gnueabi", "musleabi"];

/// Canonical os → tokens naming it in a release asset name.
static OS_ALIASES: &[(&str, &[&str])] = &[
    (
        "windows",
        &["windows", "pc-windows", "win64", "win32", "win"],
    ),
    ("macos", &["apple-darwin", "darwin", "macos"]),
    ("linux", &["linux", "unknown-linux"]),
];

/// Competing-OS tokens that disqualify a name for the keyed os when scoring:
/// `darwin` never counts for windows (and `win` would otherwise match inside
/// it), `windows` never counts for macos.
static OS_EXCLUSIONS: &[(&str, &[&str])] = &[("windows", &["darwin"]), ("macos", &["windows"])];

/// Tokens recorded for `key` in a vocabulary table, or `&[]` when unknown.
fn vocab(
    table: &'static [(&'static str, &'static [&'static str])],
    key: &str,
) -> &'static [&'static str] {
    table
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(&[], |(_, tokens)| *tokens)
}

/// Every arch token in the vocabulary, targeted or not.
fn arch_tokens() -> impl Iterator<Item = &'static str> {
    ARCH_ALIASES
        .iter()
        .flat_map(|(_, tokens)| tokens.iter().copied())
        .chain(WINDOWS_X86_64_TOKENS.iter().copied())
        .chain(OTHER_ARCH_TOKENS.iter().copied())
        .chain(ARM_EABI_TOKENS.iter().copied())
}

/// True when `name` names any architecture explicitly — targeted or not —
/// via a token, in which case the default-arch fallback must not apply.
/// 32-bit ARM comes as a bare `arm` token or an ARM EABI triple:
/// sharkdp/fd ships `arm-unknown-linux-gnueabihf` /
/// `arm-unknown-linux-musleabihf`; without recognizing these as explicit
/// archs, the linux default-arch fallback (x86_64) would match them for
/// linux/x86_64 targets.
fn has_explicit_arch_pattern(name: &str) -> bool {
    let name = name.to_lowercase();
    arch_tokens().any(|t| name.contains(t)) || has_bare_arm_token(&name)
}

/// True when `name` contains `arm` as a standalone token (delimited by
/// non-alphanumerics), e.g. `tool-1.2.3-arm-linux.tar.gz` or
/// `tool-arm.unknown.linux.gnueabihf.tar.gz`. A bare `arm` token in a release
/// filename means the ARM architecture; it is not part of another word.
fn has_bare_arm_token(name: &str) -> bool {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| tok == "arm")
}

/// True when `name` mentions the musl libc (`musl`, `musleabi`, `musleabihf`).
/// Substring matching is deliberate: it covers the EABI naming for free, since
/// `musleabi` contains `musl`.
fn has_musl_token(name: &str) -> bool {
    name.contains("musl")
}

/// True when `name` carries a `.zip` extension (case-insensitive). Used only as
/// a tie-break among assets that already share the top primary score, so a
/// `.zip` never outranks a better OS/arch/musl match.
fn is_zip(name: &str) -> bool {
    matches!(extract_extension(name).as_deref(), Some(".zip"))
}

/// True when `name` mentions the glibc libc (`gnu`, `gnueabi`, `gnueabihf`).
/// Same substring rationale as [`has_musl_token`]. Test-only: the scoring rules
/// never demote glibc assets, so this exists to document the other half of the
/// libc pair and to assert the glibc side in tests.
#[cfg(test)]
fn has_gnu_token(name: &str) -> bool {
    name.contains("gnu")
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

/// Platform match score of `name` for the target platform.
///
/// 2 points for the exact OS token, 1 for an OS alias, 1 for the target arch
/// and 1 for a musl build on linux. Zero means the name names no platform at
/// all. Shared with the archive-entry lookup in `src/extract.rs`.
pub fn platform_score(name: &str, target_os: &str, target_arch: &str) -> i32 {
    calculate_match_score(name, target_os, target_arch)
}

/// True when `name` names the target OS (see `OS_ALIASES`).
pub fn names_os(name: &str, target_os: &str) -> bool {
    os_score(&name.to_lowercase(), target_os) > 0
}

/// The canonical OS names `grd` knows about.
const CANONICAL_OSES: &[&str] = &["windows", "macos", "linux"];

/// The OS that `name` names when it is not the target one, if any.
///
/// Used to warn when a selected archive entry names a platform other than
/// the target (a `windows` build for a `linux` target, say). A name with
/// no OS token at all (`app-bin-v1.2.3`) names none. The competing-OS
/// exclusions apply (see `OS_EXCLUSIONS`), so `darwin` is not mistaken
/// for `win`.
pub fn other_named_os(name: &str, target_os: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    CANONICAL_OSES
        .iter()
        .copied()
        .find(|os| *os != target_os && !os_excludes(&lower, os) && os_score(&lower, os) > 0)
}

fn calculate_match_score(asset_name: &str, target_os: &str, target_arch: &str) -> i32 {
    let name = asset_name.to_lowercase();

    if os_excludes(&name, target_os) {
        return 0;
    }

    os_score(&name, target_os) + arch_score(&name, target_arch) + musl_bonus(&name, target_os)
}

/// True when `name` carries a competing OS's token (see [`OS_EXCLUSIONS`]).
fn os_excludes(name: &str, os: &str) -> bool {
    vocab(OS_EXCLUSIONS, os).iter().any(|t| name.contains(t))
}

/// 2 points for the exact os token, 1 for an os alias (see [`OS_ALIASES`]).
fn os_score(name: &str, target_os: &str) -> i32 {
    let aliases = vocab(OS_ALIASES, target_os);
    if aliases.is_empty() {
        return 0;
    }
    if name.contains(target_os) {
        return 2;
    }
    i32::from(aliases.iter().any(|t| name.contains(t)))
}

/// 1 point when the name names the target arch (see [`ARCH_ALIASES`]). The
/// scorer has no default-arch fallback and no windows gating: `win64` counts
/// as `x86_64` on every OS, `win32` never does.
fn arch_score(name: &str, target_arch: &str) -> i32 {
    i32::from(
        vocab(ARCH_ALIASES, target_arch)
            .iter()
            .any(|t| name.contains(t)),
    )
}

/// On linux, prefer statically linked musl builds when a release ships both
/// a glibc (`gnu`) and a musl asset for the same arch: the bonus breaks the
/// tie so `best_unique_match` resolves it instead of yielding
/// `Selection::Multiple`. No gnu penalty — a libc-neutral asset must not be
/// demoted below glibc, which was not asked for.
fn musl_bonus(name: &str, target_os: &str) -> i32 {
    i32::from(target_os == "linux" && has_musl_token(name))
}

fn sort_by_score(assets: &mut Vec<&Asset>, target_os: &str, target_arch: &str) {
    assets.sort_by(|a, b| {
        let score_a = calculate_match_score(&a.name, target_os, target_arch);
        let score_b = calculate_match_score(&b.name, target_os, target_arch);
        score_b
            .cmp(&score_a)
            .then_with(|| is_zip(&b.name).cmp(&is_zip(&a.name)))
    });
}

/// Tracks the highest score seen and the first asset reaching it.
struct TopScored<'a> {
    leader: Option<&'a Asset>,
    score: i32,
    ties: usize,
}

impl<'a> TopScored<'a> {
    fn new() -> Self {
        Self {
            leader: None,
            score: i32::MIN,
            ties: 0,
        }
    }

    /// Record one asset's score; the first leader wins ties.
    fn push(&mut self, asset: &'a Asset, score: i32) {
        match score.cmp(&self.score) {
            Ordering::Greater => {
                self.leader = Some(asset);
                self.score = score;
                self.ties = 1;
            }
            Ordering::Equal => self.ties += 1,
            Ordering::Less => {}
        }
    }
}

fn best_unique_match<'a>(
    assets: &[&'a Asset],
    target_os: &str,
    target_arch: &str,
) -> Option<&'a Asset> {
    let mut top = TopScored::new();
    for asset in assets {
        top.push(
            asset,
            calculate_match_score(&asset.name, target_os, target_arch),
        );
    }

    if top.ties == 1 {
        return top.leader;
    }

    // Tie among top scorers: a lone `.zip` in that tied set wins. This is a
    // secondary key only — it runs after the primary OS/arch/musl score, so a
    // precisely-matched `.tar.gz` still beats a vague `.zip`, and a musl
    // `.tar.gz` (primary 4) still beats a glibc `.zip` (primary 3). No zip, or
    // more than one, leaves the set genuinely ambiguous → `None`.
    lone_zip_at_score(assets, top.score, target_os, target_arch)
}

/// The lone `.zip` among assets tied at `score`, if exactly one exists.
fn lone_zip_at_score<'a>(
    assets: &[&'a Asset],
    score: i32,
    target_os: &str,
    target_arch: &str,
) -> Option<&'a Asset> {
    let mut zips = assets
        .iter()
        .copied()
        .filter(|asset| is_zip_at_score(asset, score, target_os, target_arch));
    let candidate = zips.next()?;
    zips.next().is_none().then_some(candidate)
}

/// True when `asset` is a `.zip` tied at `score`.
fn is_zip_at_score(asset: &Asset, score: i32, target_os: &str, target_arch: &str) -> bool {
    calculate_match_score(&asset.name, target_os, target_arch) == score && is_zip(&asset.name)
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
    let blacklist = parse_blacklist(exclude);
    let (normalized_os, effective_arch) = matching_platform(os, arch);

    let matches = collect_matches(
        assets,
        &blacklist,
        &normalized_os,
        &Some(effective_arch.clone()),
        no_ext_filter,
    );

    if let Some(asset) = best_unique_match(&matches, &normalized_os, &effective_arch) {
        return (matches, Some(asset.clone()));
    }

    if matches.len() > 1 && normalized_os == "windows" && effective_arch == "x86_64" {
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

/// The normalized OS and the arch to match against: the alias-inferred arch
/// (`win32`/`win64` names imply `x86_64`) or the canonicalized `arch`.
fn matching_platform(os: &str, arch: &str) -> (String, String) {
    let (normalized_os, inferred_arch) = normalize_platform_identifier(os);
    let effective_arch = inferred_arch.unwrap_or_else(|| canonical_arch_for_matching(arch));
    (normalized_os, effective_arch)
}

/// `--exclude` words, trimmed and lowercased.
fn parse_blacklist(exclude: Option<&str>) -> Vec<String> {
    exclude.map_or_else(Vec::new, |s| {
        s.split(',').map(|w| w.trim().to_lowercase()).collect()
    })
}

pub fn select_asset(
    assets: &[Asset],
    os: &str,
    arch: &str,
    mode: SelectMode,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Result<AssetSelection> {
    ensure_interactive_input(mode)?;
    match mode {
        SelectMode::All => select_interactive_all(assets, os, arch, exclude, no_ext_filter),
        SelectMode::Filtered => select_filtered(assets, os, arch, exclude, no_ext_filter),
        SelectMode::Default => select_default(assets, os, arch, exclude, no_ext_filter),
    }
}

/// Interactive modes require a terminal on stdin.
fn ensure_interactive_input(mode: SelectMode) -> Result<()> {
    if matches!(mode, SelectMode::Filtered | SelectMode::All) && !stdin_is_terminal() {
        bail!("Cannot select asset in non-terminal environment");
    }
    Ok(())
}

/// Reports a terminal on stdin; always `false` under `cfg!(test)` so the
/// interactive modes bail in unit tests.
fn stdin_is_terminal() -> bool {
    if cfg!(test) {
        false
    } else {
        io::stdin().is_terminal()
    }
}

/// `SelectMode::All`: browse every asset in the release interactively.
fn select_interactive_all(
    assets: &[Asset],
    os: &str,
    arch: &str,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Result<AssetSelection> {
    let (normalized_os, effective_arch) = matching_platform(os, arch);
    let blacklist = parse_blacklist(exclude);
    let selected = interactive_select(
        assets,
        &blacklist,
        &normalized_os,
        &effective_arch,
        "Select an asset:",
        no_ext_filter,
    )?;
    Ok(AssetSelection::Single(selected))
}

/// `SelectMode::Filtered`: pick interactively among platform matches.
fn select_filtered(
    assets: &[Asset],
    os: &str,
    arch: &str,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Result<AssetSelection> {
    let (normalized_os, effective_arch) = matching_platform(os, arch);
    let mut matches = resolve_candidates(assets, os, arch, exclude, no_ext_filter).0;
    if matches.is_empty() {
        bail!("No matching asset found for {normalized_os}-{effective_arch}");
    }
    sort_by_score(&mut matches, &normalized_os, &effective_arch);
    println!("Select a matching asset:");
    show_all_assets(&matches, &normalized_os, &effective_arch);
    let selected = collect_selection(&matches)?;
    Ok(AssetSelection::Single(selected.clone()))
}

/// `SelectMode::Default`: auto-pick the best-matching asset.
fn select_default(
    assets: &[Asset],
    os: &str,
    arch: &str,
    exclude: Option<&str>,
    no_ext_filter: bool,
) -> Result<AssetSelection> {
    let (normalized_os, effective_arch) = matching_platform(os, arch);
    match find_asset(assets, os, arch, exclude, no_ext_filter) {
        Selection::Exact(asset) => Ok(AssetSelection::Single(asset)),
        Selection::Multiple(matches) => Ok(AssetSelection::Multiple(sorted_clones(
            &matches,
            &normalized_os,
            &effective_arch,
        ))),
        Selection::None => bail!("No matching asset found for {normalized_os}-{effective_arch}"),
    }
}

/// `matches` cloned and ordered by score (best first).
fn sorted_clones(matches: &[Asset], os: &str, arch: &str) -> Vec<Asset> {
    let mut sorted: Vec<&Asset> = matches.iter().collect();
    sort_by_score(&mut sorted, os, arch);
    sorted.into_iter().cloned().collect()
}

fn interactive_select(
    assets: &[Asset],
    blacklist: &[String],
    normalized_os: &str,
    effective_arch: &str,
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
            passes_static_filters(&name, blacklist, no_ext_filter)
        })
        .collect();
    if all_assets.is_empty() {
        bail!("No assets available after applying filters");
    }
    sort_by_score(&mut all_assets, normalized_os, effective_arch);
    println!("{prompt}");
    show_all_assets(&all_assets, normalized_os, effective_arch);
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
            is_candidate(
                &name,
                blacklist,
                no_ext_filter,
                normalized_os,
                effective_arch.as_deref(),
            )
        })
        .collect()
}

/// True when `name` survives the platform-independent filters.
fn passes_static_filters(name: &str, blacklist: &[String], no_ext_filter: bool) -> bool {
    (no_ext_filter || is_allowed_by_extension(name)) && !is_blacklisted(name, blacklist)
}

/// True when `name` contains any blacklist entry. Entries are matched
/// verbatim against the lowercased name (callers lowercase entries).
fn is_blacklisted(name: &str, blacklist: &[String]) -> bool {
    blacklist.iter().any(|b| name.contains(b))
}

/// True when `name` survives all filters and names the target platform.
fn is_candidate(
    name: &str,
    blacklist: &[String],
    no_ext_filter: bool,
    os: &str,
    arch: Option<&str>,
) -> bool {
    passes_static_filters(name, blacklist, no_ext_filter)
        && os_matches(name, os)
        && arch_matches(name, os, arch)
}

/// True when `name` names `os` for the candidate filter (see [`OS_ALIASES`]).
/// Windows names must not mention `darwin` — the windows aliases include
/// `win`, a substring of `darwin`. Otherwise the filter stays existential: it
/// keeps e.g. a name mentioning both `macos` and `windows`, which
/// [`os_excludes`] zeroes when scoring.
fn os_matches(name: &str, os: &str) -> bool {
    vocab(OS_ALIASES, os).iter().any(|t| name.contains(t))
        && (os != "windows" || !os_excludes(name, os))
}

/// True when `name` matches the requested arch for the candidate filter:
/// explicit arch tokens, else the target OS's default arch when the name
/// declares no arch at all (e.g. `app-linux.tar.gz` is linux/x86_64).
fn arch_matches(name: &str, os: &str, arch: Option<&str>) -> bool {
    let Some(arch) = arch else {
        return false;
    };
    has_arch_token(name, arch, os)
        || (!has_explicit_arch_pattern(name) && default_arch_for_os(os) == Some(arch))
}

/// True when `name` names `arch` with any arch token.
fn has_arch_token(name: &str, arch: &str, os: &str) -> bool {
    vocab(ARCH_ALIASES, arch)
        .iter()
        .any(|t| token_names_arch(name, t, os))
        || has_windows_word_token(name, arch, os)
}

/// True when `token` names `arch` in `name` for target `os`. Windows
/// word-size tokens name `x86_64` but appear only in windows names, so they
/// count for windows targets only — `app-darwin-win64` must not read as
/// x86_64 elsewhere (the scorer differs; see [`WINDOWS_X86_64_TOKENS`]).
fn token_names_arch(name: &str, token: &str, os: &str) -> bool {
    name.contains(token) && (os == "windows" || !WINDOWS_X86_64_TOKENS.contains(&token))
}

/// True when `name` names `arch` with a windows word-size token (`win64`,
/// `win32`) — `x86_64` targets on windows only.
fn has_windows_word_token(name: &str, arch: &str, os: &str) -> bool {
    arch == "x86_64" && os == "windows" && WINDOWS_X86_64_TOKENS.iter().any(|t| name.contains(t))
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
    fn test_other_named_os_flags_foreign_builds() {
        assert_eq!(
            other_named_os("app-windows-x64.exe", "linux"),
            Some("windows")
        );
        assert_eq!(other_named_os("app-darwin-aarch64", "linux"), Some("macos"));
        assert_eq!(other_named_os("app-linux-x64", "windows"), Some("linux"));
        // The target OS itself, and names without any OS token, name none.
        assert_eq!(other_named_os("app-linux-x64", "linux"), None);
        assert_eq!(other_named_os("app-bin-v1.2.3", "linux"), None);
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
    fn test_exclude_musl_selects_gnu_asset() {
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

        let result = find_asset(&assets, "linux", "x86_64", Some("musl"), false);
        match result {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-x86_64-linux-gnu.tar.gz"),
            other => panic!("Expected Selection::Exact for the gnu asset, got {other:?}"),
        }
    }

    #[test]
    fn test_libc_token_helpers() {
        assert!(has_musl_token("app-x86_64-linux-musl.tar.gz"));
        assert!(has_musl_token("fd-arm-unknown-linux-musleabihf.tar.gz"));
        assert!(!has_musl_token("app-x86_64-linux-gnu.tar.gz"));

        assert!(has_gnu_token("app-x86_64-linux-gnu.tar.gz"));
        assert!(has_gnu_token("fd-arm-unknown-linux-gnueabihf.tar.gz"));
        assert!(!has_gnu_token("app-x86_64-linux-musl.tar.gz"));
    }

    #[test]
    fn test_score_prefers_musl_over_gnu_on_linux() {
        let musl = calculate_match_score("app-x86_64-unknown-linux-musl.tar.gz", "linux", "x86_64");
        let gnu = calculate_match_score("app-x86_64-unknown-linux-gnu.tar.gz", "linux", "x86_64");

        assert_eq!(gnu, 3, "gnu = 2 (exact os token) + 1 (arch token)");
        assert_eq!(musl, 4, "musl = gnu + 1 musl bonus");
        assert!(musl > gnu);
    }

    #[test]
    fn test_musl_bonus_is_linux_only() {
        let plain = calculate_match_score("x86_64-pc-windows-msvc.zip", "windows", "x86_64");
        let musl_named = calculate_match_score("x86_64-pc-windows-musl.zip", "windows", "x86_64");

        assert_eq!(
            plain, musl_named,
            "a `musl`-containing name must not gain a bonus on non-linux targets"
        );
        assert_eq!(musl_named, 3, "2 (exact os token) + 1 (arch token)");

        let darwin =
            calculate_match_score("tool-aarch64-apple-darwin-musl.tar.gz", "macos", "aarch64");
        assert_eq!(
            darwin, 2,
            "macOS scores are unchanged: 1 (darwin os) + 1 (arch)"
        );
    }

    #[test]
    fn test_musleabi_outscores_gnueabi_on_linux() {
        let musleabi =
            calculate_match_score("fd-arm-unknown-linux-musleabihf.tar.gz", "linux", "aarch64");
        let gnueabi =
            calculate_match_score("fd-arm-unknown-linux-gnueabihf.tar.gz", "linux", "aarch64");

        assert!(
            musleabi > gnueabi,
            "musleabi {musleabi} vs gnueabi {gnueabi}"
        );
        assert!(has_musl_token("fd-arm-unknown-linux-musleabihf.tar.gz"));
    }

    #[test]
    fn test_find_asset_prefers_musl_over_gnu() {
        for arch in ["x86_64", "aarch64"] {
            let assets = vec![
                Asset {
                    name: format!("app-{arch}-unknown-linux-gnu.tar.gz"),
                    browser_download_url: "https://example.com/app-gnu.tar.gz".to_string(),
                    size: 1024,
                },
                Asset {
                    name: format!("app-{arch}-unknown-linux-musl.tar.gz"),
                    browser_download_url: "https://example.com/app-musl.tar.gz".to_string(),
                    size: 2048,
                },
            ];

            match find_asset(&assets, "linux", arch, None, false) {
                Selection::Exact(asset) => {
                    assert_eq!(asset.name, format!("app-{arch}-unknown-linux-musl.tar.gz"));
                }
                other => panic!("{arch}: expected Selection::Exact(musl), got {other:?}"),
            }
        }
    }

    #[test]
    fn test_sort_by_score_places_musl_first() {
        let assets = vec![
            Asset {
                name: "app-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-gnu.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-unknown-linux-musl.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-musl.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let mut asset_refs: Vec<&Asset> = assets.iter().collect();
        sort_by_score(&mut asset_refs, "linux", "x86_64");

        assert_eq!(asset_refs[0].name, "app-x86_64-unknown-linux-musl.tar.gz");
        assert_eq!(asset_refs[1].name, "app-x86_64-unknown-linux-gnu.tar.gz");
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
        // Same primary score (3): linux exact +2, x86_64 +1. The `.zip` wins as
        // the format tie-break, so this is no longer an ambiguous match.
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

        match find_asset(&assets, "linux", "x86_64", None, false) {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-x86_64-linux.zip"),
            other => panic!("expected the zip asset, got {other:?}"),
        }
    }

    #[test]
    fn test_is_zip_helper() {
        assert!(is_zip("app-x86_64-linux.zip"));
        assert!(is_zip("APP-X86_64-LINUX.ZIP"));
        assert!(is_zip("dist/app-1.2.3.zip"));
        assert!(!is_zip("app-x86_64-linux.tar.gz"));
        assert!(!is_zip("app-x86_64-linux.tgz"));
        assert!(!is_zip("app-x86_64-linux.tar.xz"));
        assert!(!is_zip("app-x86_64-linux.exe"));
        assert!(!is_zip("app-x86_64-linux.tar.gz.sha256"));
    }

    #[test]
    fn test_select_asset_prefers_zip_over_non_zip_formats_on_linux() {
        for loser in [
            "app-x86_64-linux.tar.gz",
            "app-x86_64-linux.tgz",
            "app-x86_64-linux.tar.xz",
            "app-x86_64-linux.exe",
        ] {
            let assets = vec![
                Asset {
                    name: loser.to_string(),
                    browser_download_url: format!("https://example.com/{loser}"),
                    size: 1024,
                },
                Asset {
                    name: "app-x86_64-linux.zip".to_string(),
                    browser_download_url: "https://example.com/app.zip".to_string(),
                    size: 2048,
                },
            ];

            match find_asset(&assets, "linux", "x86_64", None, false) {
                Selection::Exact(asset) => assert_eq!(
                    asset.name, "app-x86_64-linux.zip",
                    "{loser} should lose to the zip"
                ),
                other => panic!("{loser}: expected the zip asset, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_select_asset_prefers_zip_over_exe_on_windows() {
        let assets = vec![
            Asset {
                name: "app-windows-x86_64.exe".to_string(),
                browser_download_url: "https://example.com/app.exe".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-windows-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/app.zip".to_string(),
                size: 2048,
            },
        ];

        match find_asset(&assets, "windows", "x86_64", None, false) {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-windows-x86_64.zip"),
            other => panic!("expected the zip asset, got {other:?}"),
        }
    }

    #[test]
    fn test_select_asset_musl_targz_beats_glibc_zip() {
        // Primary score decides: musl tar.gz = 4 (2 os + 1 arch + 1 musl),
        // glibc zip = 3. The format tie-break must not promote the zip.
        let assets = vec![
            Asset {
                name: "app-x86_64-linux-gnu.zip".to_string(),
                browser_download_url: "https://example.com/app-gnu.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux-musl.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-musl.tar.gz".to_string(),
                size: 2048,
            },
        ];

        match find_asset(&assets, "linux", "x86_64", None, false) {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-x86_64-linux-musl.tar.gz"),
            other => panic!("expected the musl tar.gz, got {other:?}"),
        }
    }

    #[test]
    fn test_select_asset_precise_targz_beats_vague_zip() {
        // Precise tar.gz = 3 (2 os + 1 arch); vague zip = 2 (os only, default
        // arch). The richer primary score wins despite the zip preference.
        let assets = vec![
            Asset {
                name: "app-linux.zip".to_string(),
                browser_download_url: "https://example.com/app.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 2048,
            },
        ];

        match find_asset(&assets, "linux", "x86_64", None, false) {
            Selection::Exact(asset) => assert_eq!(asset.name, "app-x86_64-linux.tar.gz"),
            other => panic!("expected the precise tar.gz, got {other:?}"),
        }
    }

    #[test]
    fn test_select_asset_non_zip_tie_stays_multiple() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux.tar.xz".to_string(),
                browser_download_url: "https://example.com/app.tar.xz".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(
            matches!(result, Selection::Multiple(_)),
            "without a zip the tie must stay ambiguous, got {result:?}"
        );
    }

    #[test]
    fn test_select_asset_two_zips_stay_multiple() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux.zip".to_string(),
                browser_download_url: "https://example.com/app1.zip".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-amd64-linux.zip".to_string(),
                browser_download_url: "https://example.com/app2.zip".to_string(),
                size: 2048,
            },
        ];

        let result = find_asset(&assets, "linux", "x86_64", None, false);
        assert!(
            matches!(result, Selection::Multiple(_)),
            "more than one zip is still ambiguous, got {result:?}"
        );
    }

    #[test]
    fn test_sort_by_score_places_zip_first_on_tie() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux.zip".to_string(),
                browser_download_url: "https://example.com/app.zip".to_string(),
                size: 2048,
            },
        ];

        let mut asset_refs: Vec<&Asset> = assets.iter().collect();
        sort_by_score(&mut asset_refs, "linux", "x86_64");

        assert_eq!(asset_refs[0].name, "app-x86_64-linux.zip");
        assert_eq!(asset_refs[1].name, "app-x86_64-linux.tar.gz");
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

    // --------------- sharkdp/fd ARM regression tests ---------------
    //
    // fd (sharkdp/fd v10.4.2) ships 32-bit ARM builds named
    // `arm-unknown-linux-gnueabihf` / `arm-unknown-linux-musleabihf`. These
    // must never be selected for a linux/x86_64 target: the bare `arm` token
    // (and the EABI triples) are explicit archs, so the default-arch fallback
    // (x86_64 for linux) must not apply to them.

    /// Exact asset list of sharkdp/fd v10.4.2 (from the GitHub release API).
    fn fd_v10_4_2_assets() -> Vec<Asset> {
        [
            "fd-musl_10.4.2_amd64.deb",
            "fd-musl_10.4.2_arm64.deb",
            "fd-musl_10.4.2_armhf.deb",
            "fd-musl_10.4.2_i686.deb",
            "fd-v10.4.2-aarch64-apple-darwin.tar.gz",
            "fd-v10.4.2-aarch64-pc-windows-msvc.zip",
            "fd-v10.4.2-aarch64-unknown-linux-gnu.tar.gz",
            "fd-v10.4.2-aarch64-unknown-linux-musl.tar.gz",
            "fd-v10.4.2-arm-unknown-linux-gnueabihf.tar.gz",
            "fd-v10.4.2-arm-unknown-linux-musleabihf.tar.gz",
            "fd-v10.4.2-i686-pc-windows-msvc.zip",
            "fd-v10.4.2-i686-unknown-linux-gnu.tar.gz",
            "fd-v10.4.2-i686-unknown-linux-musl.tar.gz",
            "fd-v10.4.2-x86_64-pc-windows-gnu.zip",
            "fd-v10.4.2-x86_64-pc-windows-msvc.zip",
            "fd-v10.4.2-x86_64-unknown-linux-gnu.tar.gz",
            "fd-v10.4.2-x86_64-unknown-linux-musl.tar.gz",
            "fd_10.4.2_amd64.deb",
            "fd_10.4.2_arm64.deb",
            "fd_10.4.2_armhf.deb",
            "fd_10.4.2_i686.deb",
        ]
        .into_iter()
        .map(|name| Asset {
            name: name.to_string(),
            browser_download_url: format!(
                "https://github.com/sharkdp/fd/releases/download/v10.4.2/{name}"
            ),
            size: 1024,
        })
        .collect()
    }

    fn assert_not_arm(asset: &Asset, context: &str) {
        let n = asset.name.to_lowercase();
        assert!(
            !(n.contains("aarch64")
                || n.contains("arm64")
                || n.contains("-arm-")
                || n.contains("gnueabi")
                || n.contains("musleabi")
                || n.contains("armhf")
                || n.contains("armv7")),
            "{context}: ARM artifact '{}' must never be selected for linux/x86_64",
            asset.name
        );
    }

    #[test]
    fn test_fd_release_linux_x86_64_never_selects_arm() {
        let assets = fd_v10_4_2_assets();
        let result = find_asset(&assets, "linux", "x86_64", None, false);
        match result {
            // The gnu and musl x86_64 linux builds used to tie; the musl bonus
            // now makes musl the unique winner, so this is an Exact match and
            // never an ambiguous one.
            Selection::Exact(asset) => {
                assert_not_arm(&asset, "Exact");
                assert_eq!(asset.name, "fd-v10.4.2-x86_64-unknown-linux-musl.tar.gz");
            }
            Selection::Multiple(matches) => {
                panic!("gnu/musl must no longer tie, got: {matches:?}");
            }
            Selection::None => panic!("expected an x86_64 linux build to match"),
        }
    }

    #[test]
    fn test_fd_arm_gnueabihf_alone_not_selected_for_x86_64() {
        let assets = vec![Asset {
            name: "fd-v10.4.2-arm-unknown-linux-gnueabihf.tar.gz".to_string(),
            browser_download_url: "https://example.com/fd-arm.tar.gz".to_string(),
            size: 1024,
        }];
        assert!(
            matches!(
                find_asset(&assets, "linux", "x86_64", None, false),
                Selection::None
            ),
            "arm-unknown-linux-gnueabihf is 32-bit ARM and must not satisfy linux/x86_64"
        );
    }

    #[test]
    fn test_fd_arm_musleabihf_alone_not_selected_for_x86_64() {
        let assets = vec![Asset {
            name: "fd-v10.4.2-arm-unknown-linux-musleabihf.tar.gz".to_string(),
            browser_download_url: "https://example.com/fd-arm-musl.tar.gz".to_string(),
            size: 1024,
        }];
        assert!(
            matches!(
                find_asset(&assets, "linux", "x86_64", None, false),
                Selection::None
            ),
            "arm-unknown-linux-musleabihf is 32-bit ARM and must not satisfy linux/x86_64"
        );
    }

    #[test]
    fn test_fd_arm_asset_not_selected_for_aarch64_either() {
        // 32-bit ARM (arm-*) is a different architecture than 64-bit aarch64.
        let assets = vec![Asset {
            name: "fd-v10.4.2-arm-unknown-linux-gnueabihf.tar.gz".to_string(),
            browser_download_url: "https://example.com/fd-arm.tar.gz".to_string(),
            size: 1024,
        }];
        assert!(
            matches!(
                find_asset(&assets, "linux", "aarch64", None, false),
                Selection::None
            ),
            "32-bit arm must not satisfy aarch64 either"
        );
    }

    #[test]
    fn test_bare_arm_token_is_explicit_arch() {
        assert!(has_explicit_arch_pattern("tool-1.2.3-arm-linux.tar.gz"));
        assert!(has_explicit_arch_pattern(
            "tool-arm.unknown.linux.gnueabihf.tar.gz"
        ));
        assert!(has_explicit_arch_pattern("tool_v1_arm_linux.zip"));
        assert!(has_explicit_arch_pattern(
            "fd-v10.4.2-arm-unknown-linux-gnueabihf.tar.gz"
        ));
        // ...but never as a substring of another word
        assert!(!has_explicit_arch_pattern("tool-v1-harmony-linux.tar.gz"));
        assert!(!has_explicit_arch_pattern("tool-v1-warm-linux.tar.gz"));
        assert!(!has_explicit_arch_pattern("app-linux-musl.tar.gz"));
    }

    // --------------- Characterization tests ---------------
    //
    // Lock in the current behavior of `has_explicit_arch_pattern`,
    // `calculate_match_score`, and `collect_matches` — including their known
    // asymmetries — before the complexity refactor. The refactor must keep
    // these green without edits.

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://example.com/{name}"),
            size: 1024,
        }
    }

    fn collect_names_with(
        names: &[&str],
        blacklist: &[String],
        os: &str,
        arch: Option<&str>,
        no_ext_filter: bool,
    ) -> Vec<String> {
        let assets: Vec<Asset> = names.iter().map(|n| asset(n)).collect();
        collect_matches(
            &assets,
            blacklist,
            os,
            &arch.map(|a| a.to_string()),
            no_ext_filter,
        )
        .into_iter()
        .map(|a| a.name.clone())
        .collect()
    }

    fn collect_names(names: &[&str], os: &str, arch: &str) -> Vec<String> {
        collect_names_with(names, &[], os, Some(arch), false)
    }

    #[test]
    fn characterize_has_explicit_arch_pattern_tokens() {
        for name in [
            "app-x86_64.tar.gz",
            "app-amd64.tar.gz",
            "app-x64.tar.gz",
            "app-aarch64.tar.gz",
            "app-arm64.tar.gz",
            "app-loong64.tar.gz",
            "app-loongarch64.tar.gz",
            "app-i686.tar.gz",
            "app-i386.tar.gz",
            "app-x86.tar.gz",
            "app-armhf.tar.gz",
            "app-armv7.tar.gz",
            "app-riscv64.tar.gz",
            "app-riscv32.tar.gz",
            "app-ppc64le.tar.gz",
            "app-ppc64.tar.gz",
            "app-powerpc.tar.gz",
            "app-s390x.tar.gz",
            "app-mips.tar.gz",
            "app-win64.zip",
            "app-win32.zip",
            "app-gnueabihf.tar.gz",
            "app-musleabihf.tar.gz",
            "APP-X86_64.TAR.GZ",
        ] {
            assert!(has_explicit_arch_pattern(name), "{name}");
        }
    }

    #[test]
    fn characterize_has_explicit_arch_pattern_absent() {
        for name in [
            "app-windows.zip",
            "app-darwin.tar.gz",
            "app-linux.tar.gz",
            "app-musl.tar.gz",
            "tool-v1-harmony-linux.tar.gz",
        ] {
            assert!(!has_explicit_arch_pattern(name), "{name}");
        }
    }

    #[test]
    fn characterize_match_score_os_and_arch() {
        // exact os token beats an os alias
        assert_eq!(
            calculate_match_score("app-windows.zip", "windows", "x86_64"),
            2
        );
        assert_eq!(calculate_match_score("app-win.zip", "windows", "x86_64"), 1);
        // cross-OS exclusion short-circuits to 0 before any scoring
        assert_eq!(
            calculate_match_score("app-x86_64-apple-darwin.tar.gz", "windows", "x86_64"),
            0
        );
        // ... but the same name still collects its arch point for other targets
        assert_eq!(
            calculate_match_score("app-x86_64-apple-darwin.tar.gz", "linux", "x86_64"),
            1
        );
    }

    #[test]
    fn characterize_match_score_musl_bonus_and_gnu_neutrality() {
        // musl gets +1 on linux; glibc is never penalized (gnu == neutral)
        assert_eq!(
            calculate_match_score("app-x86_64-linux-musl.tar.gz", "linux", "x86_64"),
            4
        );
        assert_eq!(
            calculate_match_score("app-x86_64-linux-gnu.tar.gz", "linux", "x86_64"),
            3
        );
        assert_eq!(
            calculate_match_score("app-x86_64-linux.tar.gz", "linux", "x86_64"),
            3
        );
        // the bonus is linux-only and unconditional (no os/arch match needed)
        assert_eq!(calculate_match_score("app-musl.zip", "linux", "x86_64"), 1);
        assert_eq!(
            calculate_match_score("app-x86_64-musl.zip", "macos", "x86_64"),
            1
        );
    }

    #[test]
    fn characterize_match_score_win64_win32_tokens() {
        // `win64` scores as both os and arch on windows, `win32` only as os
        assert_eq!(
            calculate_match_score("app-win64.zip", "windows", "x86_64"),
            2
        );
        assert_eq!(
            calculate_match_score("app-win32.zip", "windows", "x86_64"),
            1
        );
        // `win64` still scores as x86_64 even off windows targets
        // (collect_matches gates it behind windows — documented drift)
        assert_eq!(
            calculate_match_score("app-darwin-win64.tar.gz", "macos", "x86_64"),
            2
        );
    }

    #[test]
    fn characterize_match_score_macos_windows_exclusion() {
        // the scorer zeroes names mentioning windows for macos targets
        assert_eq!(
            calculate_match_score("app-macos-windows-x86_64.tar.gz", "macos", "x86_64"),
            0
        );
    }

    #[test]
    fn characterize_collect_matches_default_arch_fallback() {
        // no explicit arch in the name → the OS default arch applies
        assert_eq!(
            collect_names(&["app-linux.tar.gz"], "linux", "x86_64"),
            ["app-linux.tar.gz"]
        );
        assert_eq!(
            collect_names(&["app-windows.zip"], "windows", "x86_64"),
            ["app-windows.zip"]
        );
        // macos defaults to aarch64, so an arch-less name is not an x86_64 match
        assert!(collect_names(&["app-darwin.tar.gz"], "macos", "x86_64").is_empty());
        assert_eq!(
            collect_names(&["app-darwin.tar.gz"], "macos", "aarch64"),
            ["app-darwin.tar.gz"]
        );
        // ... and linux never falls back to loong64
        assert!(collect_names(&["app-linux.tar.gz"], "linux", "loong64").is_empty());
        assert_eq!(
            collect_names(&["app-loongarch64-linux.tar.gz"], "linux", "loong64"),
            ["app-loongarch64-linux.tar.gz"]
        );
        // no requested arch → nothing matches
        assert!(collect_names_with(&["app-linux.tar.gz"], &[], "linux", None, false).is_empty());
    }

    #[test]
    fn characterize_collect_matches_bare_arm_and_eabi() {
        // 32-bit ARM (bare `arm` token / EABI triples) must not fall into the
        // default-arch fallback, for any target arch
        let arm = [
            "fd-v10.4.2-arm-unknown-linux-gnueabihf.tar.gz",
            "fd-v10.4.2-arm-unknown-linux-musleabihf.tar.gz",
            "tool-1.2.3-arm-linux.tar.gz",
        ];
        assert!(collect_names(&arm, "linux", "x86_64").is_empty());
        assert!(collect_names(&arm, "linux", "aarch64").is_empty());
    }

    #[test]
    fn characterize_collect_matches_win64_win32_gating() {
        // win64/win32 count as x86_64 only for windows targets
        assert_eq!(
            collect_names(&["app-win64.zip"], "windows", "x86_64"),
            ["app-win64.zip"]
        );
        assert_eq!(
            collect_names(&["app-win32.zip"], "windows", "x86_64"),
            ["app-win32.zip"]
        );
        // off windows the same tokens are dropped even though the scorer
        // would still read `win64` as x86_64 (documented drift)
        assert!(collect_names(&["app-darwin-win64.tar.gz"], "macos", "x86_64").is_empty());
    }

    #[test]
    fn characterize_collect_matches_windows_darwin_exclusion() {
        // `darwin` contains `win`, so the windows os aliases match it — the
        // darwin exclusion is what keeps such names out of windows results
        assert!(collect_names(&["app-x86_64-apple-darwin.tar.gz"], "windows", "x86_64").is_empty());
    }

    #[test]
    fn characterize_collect_matches_macos_has_no_windows_exclusion() {
        // drift vs calculate_match_score (which zeroes this name): the macos
        // filter keeps any darwin/macos name regardless of a windows mention
        assert_eq!(
            collect_names(&["app-macos-windows-x86_64.tar.gz"], "macos", "x86_64"),
            ["app-macos-windows-x86_64.tar.gz"]
        );
    }

    #[test]
    fn characterize_collect_matches_blacklist_and_extension() {
        let names = ["app-linux-x86_64-musl.tar.gz", "app-linux-x86_64.dmg"];
        // .dmg is not an allowed extension by default
        assert_eq!(
            collect_names(&names, "linux", "x86_64"),
            ["app-linux-x86_64-musl.tar.gz"]
        );
        // ... but is kept with the extension filter off
        assert_eq!(
            collect_names_with(&names, &[], "linux", Some("x86_64"), true),
            names
        );
        // blacklist entries are matched verbatim against the lowercased name
        // (callers lowercase entries upstream), so only the lowercase form drops
        let blacklist = vec!["musl".to_string()];
        assert_eq!(
            collect_names_with(&names, &blacklist, "linux", Some("x86_64"), true),
            ["app-linux-x86_64.dmg"]
        );
        let blacklist = vec!["MUSL".to_string()];
        assert_eq!(
            collect_names_with(&names, &blacklist, "linux", Some("x86_64"), true),
            names
        );
    }

    #[test]
    fn characterize_collect_matches_unsupported_os_or_arch() {
        // unknown os matches nothing
        assert!(collect_names(&["app-freebsd-x86_64.tar.gz"], "freebsd", "x86_64").is_empty());
        // neither does an arch outside x86_64/aarch64/loong64, even when the
        // name carries that exact token
        assert!(collect_names(&["app-riscv64-linux.tar.gz"], "linux", "riscv64").is_empty());
    }
}
