use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about = "GitHub Release Downloader")]
pub struct Args {
    pub repo: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long)]
    pub list_installed: bool,

    #[arg(short, long)]
    pub tag: Option<String>,

    #[arg(short, long)]
    pub list: bool,

    #[arg(short, long)]
    pub destination: Option<PathBuf>,

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

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Register a default install directory for future downloads
    Register {
        /// Default installation path
        path: PathBuf,
    },
    /// Remove an installed package
    Remove {
        /// Repository name (e.g., owner/repo)
        repo: String,
    },
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

    #[test]
    fn test_list_installed_flag_parses() {
        let args = Args::parse_from(["grd", "--list-installed"]);
        assert!(args.list_installed);
        assert!(args.repo.is_none());
    }

    #[test]
    fn test_list_installed_defaults_to_false() {
        let args = Args::parse_from(["grd", "owner/repo"]);
        assert!(!args.list_installed);
    }

    #[test]
    fn test_list_installed_with_repo_parses_but_repo_present() {
        let args = Args::parse_from(["grd", "--list-installed", "owner/repo"]);
        assert!(args.list_installed);
        assert_eq!(args.repo.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn test_register_subcommand_parses() {
        let args = Args::parse_from(["grd", "register", "/usr/local/bin"]);
        assert!(args.repo.is_none());
        assert!(args.command.is_some());
        let Command::Register { path } = args.command.unwrap() else {
            unreachable!()
        };
        assert_eq!(path, PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn test_register_subcommand_defaults_to_none() {
        let args = Args::parse_from(["grd", "owner/repo"]);
        assert!(args.command.is_none());
    }

    #[test]
    fn test_destination_defaults_to_none() {
        let args = Args::parse_from(["grd", "owner/repo"]);
        assert!(args.destination.is_none());
    }

    #[test]
    fn test_destination_flag_parses() {
        let args = Args::parse_from(["grd", "owner/repo", "-d", "/tmp"]);
        assert_eq!(args.destination, Some(PathBuf::from("/tmp")));
    }
}
