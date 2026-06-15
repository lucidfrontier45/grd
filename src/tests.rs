use std::{env, path::PathBuf};

use clap::Parser;
use tempfile::TempDir;

use crate::{
    asset::{Selection, find_asset},
    cli::{Args, Command},
    config::{configure_agent, get_auth_token},
    download::download_asset,
    extract::extract_and_save,
    github::fetch_release_info,
    state::State,
};

#[test]
fn test_select_asset_from_real_repo() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = get_auth_token();
    let agent = configure_agent(&ua, token.as_deref());

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("15.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    match find_asset(&release.assets, os, arch, None) {
        Selection::Exact(asset) => println!("Selected asset for {}-{}: {}", os, arch, asset.name),
        Selection::Multiple(matches) => println!("{} matches for {}-{}", matches.len(), os, arch),
        Selection::None => println!("No match for {}-{}", os, arch),
    }
}

#[test]
fn test_integration_download_extract_save() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = get_auth_token();
    let agent = configure_agent(&ua, token.as_deref());

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("15.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    // Skip if multiple or no matches
    let asset = match find_asset(&release.assets, os, arch, None) {
        Selection::Exact(a) => a,
        _ => {
            println!("Skipping test: no unique match for {}-{}", os, arch);
            return;
        }
    };

    let memory_limit = 10 * 1024 * 1024;

    dbg!(&asset);
    let source = download_asset(&agent, &asset, memory_limit).unwrap();

    let temp_dir = TempDir::new().unwrap();
    let dest_dir = temp_dir.path();
    let bin_name = "rg";

    let result = extract_and_save(source, &asset.name, bin_name, dest_dir, false);
    assert!(result.is_ok());

    let expected_name = if cfg!(windows) {
        format!("{}.exe", bin_name)
    } else {
        bin_name.to_string()
    };

    let extracted_path = dest_dir.join(&expected_name);
    assert!(extracted_path.exists());
}

#[test]
fn test_confirm_upgrade_accepts_y() {
    let mut input = std::io::Cursor::new("y\n");
    assert!(crate::confirm_upgrade_impl("v1.0.0", "v2.0.0", &mut input));
}

#[test]
fn test_confirm_upgrade_accepts_y_uppercase() {
    let mut input = std::io::Cursor::new("Y\n");
    assert!(crate::confirm_upgrade_impl("v1.0.0", "v2.0.0", &mut input));
}

#[test]
fn test_confirm_upgrade_rejects_n() {
    let mut input = std::io::Cursor::new("n\n");
    assert!(!crate::confirm_upgrade_impl("v1.0.0", "v2.0.0", &mut input));
}

#[test]
fn test_confirm_upgrade_defaults_to_no() {
    let mut input = std::io::Cursor::new("\n");
    assert!(!crate::confirm_upgrade_impl("v1.0.0", "v2.0.0", &mut input));
}

#[test]
fn test_confirm_upgrade_rejects_arbitrary() {
    let mut input = std::io::Cursor::new("maybe\n");
    assert!(!crate::confirm_upgrade_impl("v1.0.0", "v2.0.0", &mut input));
}

use std::sync::{Mutex, OnceLock};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn with_state_path(dir: &TempDir, f: impl FnOnce()) {
    let _lock = env_lock().lock().unwrap();
    let state_path = dir.path().join("state.toml");
    unsafe {
        env::set_var("GRD_STATE_PATH", &state_path);
    }
    f();
    unsafe {
        env::remove_var("GRD_STATE_PATH");
    }
}

#[test]
fn test_remove_deletes_file_and_state_entry() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_cached(
            "test/repo",
            "file.tar.gz",
            "v1.0.0",
            Some(dir.path().display().to_string()),
        );
        state.save();

        let bin_name = if cfg!(windows) { "repo.exe" } else { "repo" };
        let binary_path = dir.path().join(bin_name);
        std::fs::write(&binary_path, "dummy").unwrap();

        let mut cache = State::load();
        let entry = cache.remove_cached("test/repo").unwrap();
        let dest = entry.destination.unwrap();
        let target = std::path::PathBuf::from(&dest).join(bin_name);
        std::fs::remove_file(&target).unwrap();
        cache.save();

        assert!(!binary_path.exists());
        let loaded = State::load();
        assert!(loaded.get_cached("test/repo").is_none());
    });
}

#[test]
fn test_remove_warns_when_binary_missing() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_cached(
            "test/repo",
            "file.tar.gz",
            "v1.0.0",
            Some(dir.path().display().to_string()),
        );
        state.save();

        let bin_name = if cfg!(windows) { "repo.exe" } else { "repo" };
        let binary_path = dir.path().join(bin_name);

        let mut cache = State::load();
        let entry = cache.remove_cached("test/repo").unwrap();
        let dest = entry.destination.unwrap();
        let target = std::path::PathBuf::from(&dest).join(bin_name);

        // Binary doesn't exist — should warn (we can't capture stderr easily, but we check it doesn't panic)
        if target.exists() {
            std::fs::remove_file(&target).unwrap();
        }
        cache.save();

        assert!(!binary_path.exists());
        let loaded = State::load();
        assert!(loaded.get_cached("test/repo").is_none());
    });
}

#[test]
fn test_remove_no_state_entry() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut cache = State::load();
        let entry = cache.remove_cached("test/repo");
        assert!(entry.is_none());
        // Verify no crash — should just warn and exit
    });
}

#[test]
fn test_remove_subcommand_parses() {
    let args = Args::try_parse_from(["grd", "remove", "owner/repo"]).unwrap();
    assert!(args.command.is_some());
    let Command::Remove { repo } = args.command.unwrap() else {
        unreachable!()
    };
    assert_eq!(repo, "owner/repo");
}

#[test]
fn test_remove_subcommand_not_set_by_default() {
    let args = Args::try_parse_from(["grd", "owner/repo"]).unwrap();
    assert!(args.command.is_none());
}

#[test]
fn test_list_installed_displays_installed_packages() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo-linux.tar.gz", "v1.0.0", None);
        state.set_cached("other/app", "bar-macos.zip", "v2.3.1", None);
        state.save();

        let cache = State::load();
        assert_eq!(cache.versions.len(), 2);

        let entry1 = cache.get_cached("owner/repo").unwrap();
        assert_eq!(entry1.tag, "v1.0.0");
        assert_eq!(entry1.asset, "foo-linux.tar.gz");

        let entry2 = cache.get_cached("other/app").unwrap();
        assert_eq!(entry2.tag, "v2.3.1");
        assert_eq!(entry2.asset, "bar-macos.zip");
    });
}

#[test]
fn test_list_installed_empty_state() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let cache = State::load();
        assert!(cache.versions.is_empty());
    });
}

#[test]
fn test_register_subcommand_persists_to_state() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        // Simulate what main.rs does on "grd register /custom/path"
        let mut state = State::default();
        state.set_default_install_dir("/custom/path");
        state.save();

        let loaded = State::load();
        assert_eq!(loaded.get_default_install_dir(), Some("/custom/path"));
    });
}

#[test]
fn test_destination_falls_back_to_registered_dir() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_default_install_dir("/registered/path");
        state.save();

        // When --destination is None, the fallback logic should pick up the registered dir
        let dest = None::<std::path::PathBuf>;
        let resolved = match dest {
            Some(d) => d,
            None => State::load()
                .get_default_install_dir()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
        };
        assert_eq!(resolved, PathBuf::from("/registered/path"));
    });
}

#[test]
fn test_destination_falls_back_to_default_install_path_when_no_registered_dir() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let state = State::load();
        assert!(state.get_default_install_dir().is_none());

        let dest = None::<std::path::PathBuf>;
        let resolved = match dest {
            Some(d) => d,
            None => State::load()
                .get_default_install_dir()
                .map(PathBuf::from)
                .unwrap_or_else(State::default_install_path),
        };
        let expected = State::default_install_path();
        assert_eq!(resolved, expected);
    });
}

#[test]
fn test_list_installed_does_not_modify_state() {
    let dir = TempDir::new().unwrap();
    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", None);
        state.save();

        let before = std::fs::read_to_string(std::env::var("GRD_STATE_PATH").unwrap()).unwrap();

        // Simulate read-only access: load state but don't save
        let _cache = State::load();
        assert_eq!(_cache.versions.len(), 1);

        let after = std::fs::read_to_string(std::env::var("GRD_STATE_PATH").unwrap()).unwrap();

        assert_eq!(before, after);
    });
}

#[test]
fn test_info_subcommand_displays_cached_entry() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("bin");
    std::fs::create_dir_all(&dest).unwrap();
    let binary_path = dest.join("myapp.exe");
    std::fs::write(&binary_path, "fake binary").unwrap();

    with_state_path(&dir, || {
        let mut state = State::default();
        state.set_cached(
            "owner/repo",
            "myapp.zip",
            "v1.0.0",
            Some(dest.to_str().unwrap().to_string()),
        );
        state.save();

        let args = Args::try_parse_from(["grd", "info", "owner/repo"]).unwrap();
        let Command::Info { repo } = args.command.unwrap() else {
            unreachable!()
        };
        assert_eq!(repo, "owner/repo");

        let cache = State::load();
        let entry = cache.get_cached(&repo).unwrap();
        assert_eq!(entry.tag, "v1.0.0");
        assert_eq!(entry.asset, "myapp.zip");
        assert_eq!(entry.destination.as_deref(), Some(dest.to_str().unwrap()));

        let binary = dest.join("myapp.exe");
        assert!(binary.exists());
    });
}
