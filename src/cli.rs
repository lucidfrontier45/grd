use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about = "GitHub Release Downloader")]
pub struct Args {
    pub repo: String,

    #[arg(short, long)]
    pub tag: Option<String>,

    #[arg(short, long)]
    pub list: bool,

    #[arg(short, long, default_value = ".")]
    pub destination: PathBuf,

    #[arg(short, long)]
    pub bin_name: Option<String>,

    #[arg(long)]
    pub select: bool,

    #[arg(long)]
    pub exclude: Option<String>,

    #[arg(long = "no-decompress")]
    pub no_decompress: bool,

    #[arg(short = 'm', long = "memory-limit", default_value = "104857600")]
    pub memory_limit: u64,

    #[arg(long)]
    pub force: bool,

    #[arg(short = 'y', long)]
    pub yes: bool,

    #[arg(long)]
    pub os: Option<String>,

    #[arg(long)]
    pub arch: Option<String>,

    #[arg(long)]
    pub list_platforms: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yes_flag_defaults_to_false() {
        let args = Args::parse_from(["grd", "owner/repo"]);
        assert!(!args.yes);
    }

    #[test]
    fn test_yes_flag_parses_as_true() {
        let args = Args::parse_from(["grd", "owner/repo", "--yes"]);
        assert!(args.yes);
    }

    #[test]
    fn test_yes_flag_short() {
        let args = Args::parse_from(["grd", "owner/repo", "-y"]);
        assert!(args.yes);
    }

    #[test]
    fn test_force_flag_defaults_to_false() {
        let args = Args::parse_from(["grd", "owner/repo"]);
        assert!(!args.force);
    }

    #[test]
    fn test_force_flag_parses_as_true() {
        let args = Args::parse_from(["grd", "owner/repo", "--force"]);
        assert!(args.force);
    }
}
