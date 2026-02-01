use std::io::{self, Write};

use anyhow::{Context, Result, bail};

use crate::github::Asset;

pub fn normalize_os(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "windows" | "macos" | "linux" => Ok(normalized),
        _ => bail!("Invalid OS '{}'. Supported: windows, macos, linux", input),
    }
}

pub fn normalize_arch(input: &str) -> Result<String> {
    let normalized = input.to_lowercase();
    match normalized.as_str() {
        "x86_64" | "amd64" | "x64" => Ok("x86_64".to_string()),
        "aarch64" | "arm64" => Ok("aarch64".to_string()),
        _ => bail!(
            "Invalid architecture '{}'. Supported: x86_64 (aliases: amd64, x64), aarch64 (alias: arm64)",
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

pub fn select_asset(
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
        0 => bail!("No matching asset found for {}-{}", os, arch),
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
                    io::stdout().flush().context("Failed to flush stdout")?;
                    let mut input = String::new();
                    io::stdin()
                        .read_line(&mut input)
                        .context("Failed to read input")?;
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
