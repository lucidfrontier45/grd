# grd

GitHub Release Downloader

A command-line tool to download and install binaries from GitHub releases.

## Installation

### From crates.io

If published on crates.io:

```bash
cargo install grd
```

### From source

Ensure you have Rust installed, then:

```bash
cargo install --path .
```

### Prebuilt binaries

Download from [releases](https://github.com/yourusername/grd/releases).

## Usage

Download the latest release of a repository:

```bash
grd owner/repo
```

Download a specific version:

```bash
grd owner/repo --tag v1.0.0
```

List available versions:

```bash
grd owner/repo --list
```

Specify destination directory:

```bash
grd owner/repo --destination /usr/local/bin
```

## Options

- `repo`: GitHub repository (owner/repo)
- `--tag`: Specific version tag (defaults to latest)
- `--list`: List available releases
- `--destination`: Destination directory (default: current directory)
- `--bin-name`: Override executable name
- `--first`: Select first matching asset without prompting
- `--exclude`: Comma-separated words to exclude from asset matching

## Building

```bash
cargo build
cargo test
cargo clippy
```
