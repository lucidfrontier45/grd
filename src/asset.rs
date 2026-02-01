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

#[cfg(test)]
mod unit_tests {
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
    fn test_select_asset_single_match() {
        let assets = vec![Asset {
            name: "app-x86_64-linux.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = select_asset(&assets, "linux", "x86_64", false, None).unwrap();
        assert_eq!(result.name, "app-x86_64-linux.tar.gz");
    }

    #[test]
    fn test_select_asset_multiple_matches_with_first() {
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

        let result = select_asset(&assets, "linux", "x86_64", true, None).unwrap();
        assert_eq!(result.name, "app-x86_64-linux.tar.gz");
    }

    #[test]
    fn test_select_asset_with_exclude() {
        let assets = vec![
            Asset {
                name: "app-x86_64-linux-musl.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-musl.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-x86_64-linux-gnu.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-gnu.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = select_asset(&assets, "linux", "x86_64", true, Some("musl")).unwrap();
        assert_eq!(result.name, "app-x86_64-linux-gnu.tar.gz");
    }

    #[test]
    fn test_select_asset_no_match() {
        let assets = vec![Asset {
            name: "app-x86_64-windows.zip".to_string(),
            browser_download_url: "https://example.com/app.zip".to_string(),
            size: 1024,
        }];

        assert!(select_asset(&assets, "linux", "x86_64", false, None).is_err());
    }

    #[test]
    fn test_select_asset_windows_patterns() {
        let assets = vec![
            Asset {
                name: "app-windows-x86_64.exe".to_string(),
                browser_download_url: "https://example.com/app.exe".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-pc-windows-msvc.zip".to_string(),
                browser_download_url: "https://example.com/app.zip".to_string(),
                size: 2048,
            },
        ];

        let result = select_asset(&assets, "windows", "x86_64", false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_asset_macos_patterns() {
        let assets = vec![
            Asset {
                name: "app-darwin-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app.tar.gz".to_string(),
                size: 1024,
            },
            Asset {
                name: "app-apple-darwin-aarch64.tar.gz".to_string(),
                browser_download_url: "https://example.com/app-arm.tar.gz".to_string(),
                size: 2048,
            },
        ];

        let result = select_asset(&assets, "macos", "aarch64", false, None).unwrap();
        assert_eq!(result.name, "app-apple-darwin-aarch64.tar.gz");
    }

    #[test]
    fn test_select_asset_linux_patterns() {
        let assets = vec![Asset {
            name: "app-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            browser_download_url: "https://example.com/app.tar.gz".to_string(),
            size: 1024,
        }];

        let result = select_asset(&assets, "linux", "x86_64", false, None);
        assert!(result.is_ok());
    }
}
