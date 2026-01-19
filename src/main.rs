use std::{
    env,
    fs::{self, File},
    io::{self, Cursor, Read, Seek, Write},
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};
use clap::Parser;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use tempfile::NamedTempFile;
use ureq::Agent;
use zip::ZipArchive;

#[derive(Parser, Debug)]
#[command(author, version, about = "GitHub Release Downloader")]
struct Args {
    /// GitHub repository (e.g., owner/repo)
    repo: Option<String>,

    /// Version to download (e.g., v1.2.3). If omitted, uses latest
    #[arg(short, long)]
    tag: Option<String>,

    /// List available release versions
    #[arg(short, long)]
    list: bool,

    /// Destination directory
    #[arg(short, long, default_value = ".")]
    destination: PathBuf,

    /// Executable file name (defaults to repository name if not specified)
    #[arg(short, long)]
    bin_name: Option<String>,

    /// Always select the first matching asset without prompting
    #[arg(long)]
    first: bool,

    /// Comma-separated list of words to exclude from asset matching
    #[arg(long)]
    exclude: Option<String>,

    /// Save downloaded file without decompressing/extracting it
    #[arg(long = "no-decompress")]
    no_decompress: bool,

    /// Memory limit in bytes; downloads larger than this use temp files
    #[arg(short = 'm', long = "memory-limit", default_value = "104857600")]
    memory_limit: u64,

    /// Target OS (windows, macos, linux, auto-detect if omitted)
    #[arg(long)]
    os: Option<String>,

    /// Target architecture (x86_64, aarch64, auto-detect if omitted)
    #[arg(long)]
    arch: Option<String>,

    /// List supported platform combinations
    #[arg(long)]
    list_platforms: bool,
}

#[derive(Deserialize, Debug)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize, Debug, Clone)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

enum DownloadSource {
    Memory(Vec<u8>),
    Disk(NamedTempFile),
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

fn main() -> Result<()> {
    let args = Args::parse();

    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let agent: Agent = Agent::config_builder().user_agent(&ua).build().into();

    // If the --list flag is present
    if args.list {
        let repo = args
            .repo
            .as_ref()
            .ok_or_else(|| anyhow!("--list requires a repository"))?;
        return list_releases(&agent, repo);
    }

    let repo = args.repo.ok_or_else(|| anyhow!("Repository is required"))?;

    // 1. Fetch release info (specific tag or latest)
    let release = fetch_release_info(&agent, &repo, args.tag.as_deref())?;
    println!("Selected version: {}", release.tag_name);

    // 2. Select the asset best matching the host or explicit platform
    let os = args
        .os
        .as_ref()
        .map(|s| normalize_os(s))
        .transpose()?
        .unwrap_or_else(|| env::consts::OS.to_string());
    let arch = args
        .arch
        .as_ref()
        .map(|s| normalize_arch(s))
        .transpose()?
        .unwrap_or_else(|| env::consts::ARCH.to_string());

    if args.os.is_none() && args.arch.is_none() {
        println!("Detected platform: {}-{}", os, arch);
    } else {
        println!("Using platform: {}-{}", os, arch);
    }

    let asset = select_asset(
        &release.assets,
        &os,
        &arch,
        args.first,
        args.exclude.as_deref(),
    )?;
    println!("Selected asset: {}", asset.name);

    // 3. Download and place the binary
    let bin_name = args
        .bin_name
        .unwrap_or_else(|| repo.split('/').next_back().unwrap_or("app").to_string());

    let source = download_asset(&agent, &asset, args.memory_limit)?;

    extract_and_save(
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

/// List releases
fn list_releases(agent: &Agent, repo: &str) -> Result<()> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let mut response = agent.get(&url).call()?;
    let releases: Vec<Release> = response.body_mut().read_json()?;

    println!("Available releases for {}:", repo);
    for rel in releases {
        println!("  - {}", rel.tag_name);
    }
    Ok(())
}

/// Fetch release information for a given tag or the latest release
fn fetch_release_info(agent: &Agent, repo: &str, tag: Option<&str>) -> Result<Release> {
    let url = match tag {
        Some(t) => format!("https://api.github.com/repos/{}/releases/tags/{}", repo, t),
        None => format!("https://api.github.com/repos/{}/releases/latest", repo),
    };

    let mut response = agent.get(&url).call()?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch release info: {}",
            response.status()
        ));
    }
    Ok(response.body_mut().read_json()?)
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

fn normalize_os(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "windows" | "macos" | "linux" => Ok(normalized),
        _ => Err(anyhow!(
            "Invalid OS '{}'. Supported: windows, macos, linux",
            input
        )),
    }
}

fn normalize_arch(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "x86_64" | "amd64" | "x64" => Ok("x86_64".to_string()),
        "aarch64" | "arm64" => Ok("aarch64".to_string()),
        _ => Err(anyhow!(
            "Invalid architecture '{}'. Supported: x86_64 (aliases: amd64, x64), aarch64 (alias: arm64)",
            input
        )),
    }
}

fn select_asset(
    assets: &[Asset],
    os: &str,
    arch: &str,
    first: bool,
    exclude: Option<&str>,
) -> Result<Asset> {
    let blacklist: Vec<String> = exclude.map_or_else(Vec::new, |s| {
        s.split(',').map(|w| w.trim().to_lowercase()).collect()
    });

    let matches: Vec<&Asset> = assets
        .iter()
        .filter(|a| {
            let name = a.name.to_lowercase();
            let os_match = match os {
                "windows" => {
                    name.contains("windows")
                        || name.contains("win64")
                        || name.contains("pc-windows")
                }
                "macos" => {
                    name.contains("apple-darwin")
                        || name.contains("macos")
                        || name.contains("darwin")
                }
                "linux" => name.contains("linux") || name.contains("unknown-linux"),
                _ => false,
            };
            let arch_match = match arch {
                "x86_64" => {
                    name.contains("x86_64") || name.contains("amd64") || name.contains("x64")
                }
                "aarch64" => name.contains("aarch64") || name.contains("arm64"),
                _ => false,
            };
            os_match && arch_match && !blacklist.iter().any(|b| name.contains(b))
        })
        .collect();

    match matches.len() {
        0 => Err(anyhow!("No matching asset found for {}-{}", os, arch)),
        1 => Ok(matches[0].clone()),
        _ => {
            if first {
                Ok(matches[0].clone())
            } else {
                println!("Multiple assets found. Select one:");
                for (i, asset) in matches.iter().enumerate() {
                    println!("{}. {} ({})", i + 1, asset.name, format_size(asset.size));
                }
                loop {
                    print!("Enter choice (1-{}): ", matches.len());
                    io::stdout().flush().unwrap();
                    let mut input = String::new();
                    io::stdin()
                        .read_line(&mut input)
                        .map_err(|_| anyhow!("Failed to read input"))?;
                    match input.trim().parse::<usize>() {
                        Ok(n) if n >= 1 && n <= matches.len() => return Ok(matches[n - 1].clone()),
                        _ => println!(
                            "Invalid choice. Enter a number between 1 and {}.",
                            matches.len()
                        ),
                    }
                }
            }
        }
    }
}

fn download_asset(agent: &Agent, asset: &Asset, memory_threshold: u64) -> Result<DownloadSource> {
    println!("Downloading...");
    let pb = ProgressBar::new(asset.size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>–"),
    );
    let mut response = agent.get(&asset.browser_download_url).call()?;
    let mut reader = response.body_mut().as_reader();
    let source = if asset.size > memory_threshold {
        println!("Using temp file due to size > {} bytes", memory_threshold);
        let mut temp_file = NamedTempFile::new()?;
        let writer = |buf: &[u8]| temp_file.write_all(buf);
        download_with_progress(&mut reader, &pb, writer)?;
        DownloadSource::Disk(temp_file)
    } else {
        let mut bytes = Vec::new();
        let writer = |buf: &[u8]| {
            bytes.extend_from_slice(buf);
            Ok(())
        };
        download_with_progress(&mut reader, &pb, writer)?;
        DownloadSource::Memory(bytes)
    };
    pb.finish_with_message("Downloaded");
    Ok(source)
}

fn download_with_progress<R: Read, F>(reader: &mut R, pb: &ProgressBar, mut writer: F) -> Result<()>
where
    F: FnMut(&[u8]) -> io::Result<()>,
{
    let mut buf = [0; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        writer(&buf[..n])?;
        pb.inc(n as u64);
    }
    Ok(())
}

fn extract_and_save(
    source: DownloadSource,
    filename: &str,
    bin_name: &str,
    dest_dir: &Path,
    no_decompress: bool,
) -> Result<()> {
    fs::create_dir_all(dest_dir)?;

    if no_decompress {
        // Save using the original asset name (do not rename or extract)
        save_raw(source, filename, dest_dir)?;
        println!("Saved raw asset to {:?}", dest_dir.join(filename));
        return Ok(());
    }

    let target_bin_name = if cfg!(windows) {
        format!("{}.exe", bin_name)
    } else {
        bin_name.to_string()
    };

    if filename.ends_with(".zip") {
        extract_zip(source, &target_bin_name, dest_dir)
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(source, &target_bin_name, dest_dir)
    } else {
        save_raw(source, &target_bin_name, dest_dir)
    }
}

fn extract_zip(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let rdr: Box<dyn ReadSeek> = match source {
        DownloadSource::Memory(bytes) => Box::new(Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let target_bin_name: &str = target_bin_name;
    let mut archive = ZipArchive::new(rdr)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name().ends_with(target_bin_name) {
            let out_path = dest_dir.join(target_bin_name);
            let mut outfile = File::create(&out_path)?;
            io::copy(&mut file, &mut outfile)?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    Err(anyhow!(
        "Executable '{}' not found in archive",
        target_bin_name
    ))
}

fn extract_tar_gz(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let rdr: Box<dyn Read> = match source {
        DownloadSource::Memory(bytes) => Box::new(Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let target_bin_name: &str = target_bin_name;
    let mut archive = tar::Archive::new(GzDecoder::new(rdr));
    for entry in archive.entries()? {
        let mut file = entry?;
        let path = file.path()?.to_path_buf();
        if path.to_string_lossy().ends_with(target_bin_name) {
            let out_path = dest_dir.join(target_bin_name);
            file.unpack(&out_path)?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    Err(anyhow!(
        "Executable '{}' not found in archive",
        target_bin_name
    ))
}

fn save_raw(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let out_path = dest_dir.join(target_bin_name);
    match source {
        DownloadSource::Memory(bytes) => {
            fs::write(&out_path, bytes)?;
        }
        DownloadSource::Disk(temp_file) => {
            fs::copy(temp_file.path(), &out_path)?;
        }
    }
    #[cfg(unix)]
    set_permissions(&out_path)?;
    Ok(())
}

#[cfg(unix)]
fn set_permissions(path: &Path) -> Result<()> {
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}
