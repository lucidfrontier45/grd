# GRD Project Overview

## Project Purpose
GRD (GitHub Release Downloader) is a command-line tool that downloads and installs binaries from GitHub releases. It automatically detects the user's platform, selects the appropriate asset, and can extract archives or save raw files.

## Key Features
- Download latest or specific release versions
- Automatic platform detection (OS and architecture)
- Support for .zip and .tar.gz archives
- Memory-efficient downloading (uses temp files for large downloads)
- Progress bar during downloads
- Asset selection with exclude filters
- Manual platform specification override
- List available releases
- Custom memory limit configuration

## Tech Stack
- **Language**: Rust 1.92+ (Edition 2024)
- **CLI Framework**: clap 4.5 (derive features)
- **HTTP Client**: ureq 3.1 (minimal, sync)
- **Compression**: flate2, tar, zip
- **Error Handling**: anyhow 1.0
- **Progress Display**: indicatif 0.18
- **Temp Files**: tempfile 3.24
- **Serialization**: serde 1.0

## Project Structure
```
src/
├── main.rs          # Entry point, orchestrates the workflow
├── cli.rs           # CLI argument definitions (Args struct)
├── github.rs        # GitHub API interaction (Release, Asset)
├── asset.rs         # Asset selection and platform normalization
├── download.rs      # Download logic with progress tracking
└── extract.rs       # Archive extraction and file saving
```

## Recent Development
- Recent refactor split monolithic file into modules (commit 87cae7b)
- Added OS/architecture specification support (v0.2.4)
- Replaced reqwest with ureq for smaller binary size (v0.2.2)
- Added progress bar support (v0.2.3)
