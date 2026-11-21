#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Write the state file without ever making it group/world readable.
///
/// Since the file can hold a GitHub PAT, an existing file is restricted to
/// owner-only mode before its contents are replaced, and a new file is
/// created with owner-only mode directly, so no wider mode is ever visible
/// on disk — not even briefly.
#[cfg(unix)]
fn write_state_file(path: &Path, content: &str) -> std::io::Result<()> {
    use std::{
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
    };

    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())
}

/// Non-Unix fallback: files under the user profile inherit ACLs that already
/// exclude other standard users, and std exposes no ACL API.
#[cfg(not(unix))]
fn write_state_file(path: &Path, content: &str) -> std::io::Result<()> {
    fs::write(path, content)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CachedRelease {
    pub tag: String,
    pub asset: String,
    pub destination: String,
    /// Final on-disk filename, platform-specific (e.g. `fd.exe` on Windows).
    /// Absent in state files written before `--rename` existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<String>,
}

impl CachedRelease {
    /// Filename this release was installed as.
    ///
    /// Falls back to the repo basename for entries recorded before
    /// `binary` was stored, so older state files keep working.
    pub fn installed_filename(&self, repo: &str) -> String {
        if let Some(binary) = &self.binary {
            return binary.clone();
        }
        let bin_name = repo.split('/').next_back().unwrap_or("app");
        if cfg!(windows) {
            format!("{}.exe", bin_name)
        } else {
            bin_name.to_string()
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct State {
    #[serde(rename = "versions", default)]
    pub versions: HashMap<String, CachedRelease>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_install_dir: Option<String>,

    /// GitHub personal access token used when neither `GITHUB_PAT` nor
    /// `GITHUB_TOKEN` is set in the environment. Stored in plaintext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_pat: Option<String>,
}

impl State {
    fn state_path() -> Result<PathBuf, String> {
        Self::state_path_from_env().or_else(|_| Self::state_path_default())
    }

    fn state_path_default() -> Result<PathBuf, String> {
        let home = env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .map_err(|_| "HOME or USERPROFILE environment variable not set".to_string())?;
        let mut path = PathBuf::from(home);
        path.push(".grd");
        path.push("state.toml");
        Ok(path)
    }

    pub fn default_install_path() -> PathBuf {
        let home = env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        let mut path = PathBuf::from(home);
        path.push(".grd");
        path.push("bin");
        path
    }

    #[cfg(test)]
    fn state_path_from_env() -> Result<PathBuf, String> {
        env::var("GRD_STATE_PATH")
            .map(PathBuf::from)
            .map_err(|_| "GRD_STATE_PATH not set".to_string())
    }

    #[cfg(not(test))]
    fn state_path_from_env() -> Result<PathBuf, String> {
        Err("GRD_STATE_PATH not set".to_string())
    }

    pub fn load() -> Self {
        let path = match Self::state_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: cannot determine home directory: {}", e);
                return Self::default();
            }
        };

        if !path.exists() {
            return Self::default();
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: failed to read state file {:?}: {}", path, e);
                return Self::default();
            }
        };

        match toml::from_str(&content) {
            Ok(state) => state,
            Err(e) => {
                eprintln!(
                    "Warning: corrupt state file {:?}: {}. Proceeding with fresh state.",
                    path, e
                );
                Self::default()
            }
        }
    }

    /// Serialize the state and write it to the state file.
    ///
    /// The file is created with owner-only permissions where the platform
    /// supports it (see [`write_state_file`]). All failures propagate:
    /// a failed save aborts the command with a non-zero exit instead of a
    /// silently lost setting or cache entry.
    pub fn save(&self) -> Result<()> {
        let path = Self::state_path()
            .map_err(|e| anyhow::anyhow!("cannot determine home directory: {e}"))?;
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("state file {path:?} has no parent directory"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create state directory {parent:?}"))?;
        let content = toml::to_string_pretty(self).context("failed to serialize state")?;
        write_state_file(&path, &content)
            .with_context(|| format!("failed to write state file {path:?}"))?;
        Ok(())
    }

    pub fn get_cached(&self, repo: &str) -> Option<&CachedRelease> {
        self.versions.get(repo)
    }

    pub fn remove_cached(&mut self, repo: &str) -> Option<CachedRelease> {
        self.versions.remove(repo)
    }

    pub fn get_default_install_dir(&self) -> Option<&str> {
        self.default_install_dir.as_deref()
    }

    pub fn set_default_install_dir(&mut self, path: &str) {
        self.default_install_dir = Some(path.to_string());
    }

    pub fn clear_default_install_dir(&mut self) {
        self.default_install_dir = None;
    }

    pub fn get_github_pat(&self) -> Option<&str> {
        self.github_pat.as_deref()
    }

    pub fn set_github_pat(&mut self, token: &str) {
        self.github_pat = Some(token.to_string());
    }

    pub fn clear_github_pat(&mut self) {
        self.github_pat = None;
    }

    pub fn set_cached(
        &mut self,
        repo: &str,
        asset_name: &str,
        tag: &str,
        destination: String,
        binary: &str,
    ) {
        self.versions.insert(
            repo.to_string(),
            CachedRelease {
                tag: tag.to_string(),
                asset: asset_name.to_string(),
                destination,
                binary: Some(binary.to_string()),
            },
        );
    }
}

/// Lock guarding the `GRD_STATE_PATH` environment variable.
///
/// Shared with the integration tests in `src/tests.rs`: both write the same
/// variable, so they must never run at the same time.
#[cfg(test)]
pub(crate) fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use std::sync::MutexGuard;

    use super::*;

    struct StatePathEnvGuard {
        _lock: MutexGuard<'static, ()>,
        previous: Option<String>,
    }

    impl StatePathEnvGuard {
        fn set(state_path: &std::path::Path) -> Self {
            let lock = env_lock().lock().unwrap();
            let previous = env::var("GRD_STATE_PATH").ok();

            unsafe {
                // Safety: the mutex guard held in this helper serializes access to
                // GRD_STATE_PATH across tests, so mutating the process environment
                // here cannot race with other test code using the same variable.
                env::set_var("GRD_STATE_PATH", state_path);
            }

            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for StatePathEnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => unsafe {
                    // Safety: this guard still holds the mutex acquired in `set`,
                    // so restoring the environment variable remains serialized.
                    env::set_var("GRD_STATE_PATH", previous);
                },
                None => unsafe {
                    // Safety: this guard still holds the mutex acquired in `set`,
                    // so removing the environment variable remains serialized.
                    env::remove_var("GRD_STATE_PATH");
                },
            }
        }
    }

    #[test]
    fn test_state_default_is_empty() {
        let state = State::default();
        assert!(state.versions.is_empty());
    }

    #[test]
    fn test_set_and_get_cached() {
        let mut state = State::default();
        state.set_cached(
            "owner/repo",
            "foo-linux.tar.gz",
            "v1.0.0",
            String::new(),
            "",
        );
        let cached = state.get_cached("owner/repo").unwrap();
        assert_eq!(cached.tag, "v1.0.0");
        assert_eq!(cached.asset, "foo-linux.tar.gz");
    }

    #[test]
    fn test_get_nonexistent_repo() {
        let state = State::default();
        assert!(state.get_cached("no/such").is_none());
    }

    #[test]
    fn test_set_overwrites_previous() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", String::new(), "");
        state.set_cached("owner/repo", "bar.tar.gz", "v2.0.0", String::new(), "");
        let cached = state.get_cached("owner/repo").unwrap();
        assert_eq!(cached.tag, "v2.0.0");
        assert_eq!(cached.asset, "bar.tar.gz");
    }

    #[test]
    fn test_diff_repos_independent() {
        let mut state = State::default();
        state.set_cached("a/x", "asset-a.tar.gz", "v1", String::new(), "");
        state.set_cached("b/y", "asset-b.tar.gz", "v2", String::new(), "");
        let a = state.get_cached("a/x").unwrap();
        assert_eq!(a.tag, "v1");
        assert_eq!(a.asset, "asset-a.tar.gz");
        let b = state.get_cached("b/y").unwrap();
        assert_eq!(b.tag, "v2");
        assert_eq!(b.asset, "asset-b.tar.gz");
    }

    #[test]
    fn test_same_repo_only_one_entry() {
        let mut state = State::default();
        state.set_cached(
            "owner/repo",
            "foo-linux.tar.gz",
            "v1.0.0",
            String::new(),
            "",
        );
        state.set_cached(
            "owner/repo",
            "foo-macos.tar.gz",
            "v1.0.0",
            String::new(),
            "",
        );
        // Second overwrites first — only one entry per repo
        assert_eq!(state.versions.len(), 1);
        let cached = state.get_cached("owner/repo").unwrap();
        assert_eq!(cached.asset, "foo-macos.tar.gz");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_cached(
            "owner/repo",
            "foo-linux.tar.gz",
            "v1.0.0",
            String::new(),
            "",
        );
        state.set_cached("other/repo", "bar-macos.zip", "v2.3.1", String::new(), "");
        state.save().unwrap();

        let loaded = State::load();
        let a = loaded.get_cached("owner/repo").unwrap();
        assert_eq!(a.tag, "v1.0.0");
        assert_eq!(a.asset, "foo-linux.tar.gz");
        let b = loaded.get_cached("other/repo").unwrap();
        assert_eq!(b.tag, "v2.3.1");
        assert_eq!(b.asset, "bar-macos.zip");
    }

    #[test]
    fn test_load_returns_default_for_corrupt_state_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        fs::write(&state_path, "not = [valid toml").unwrap();

        let loaded = State::load();
        assert!(loaded.versions.is_empty());
    }

    #[test]
    fn test_load_missing_versions_falls_back_to_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        fs::write(&state_path, "default_install_dir = \"/opt/bin\"\n").unwrap();

        let loaded = State::load();
        assert!(loaded.versions.is_empty());
        assert_eq!(loaded.default_install_dir.as_deref(), Some("/opt/bin"));
    }

    #[test]
    fn test_remove_cached_returns_entry() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", String::new(), "");
        let removed = state.remove_cached("owner/repo").unwrap();
        assert_eq!(removed.tag, "v1.0.0");
        assert_eq!(removed.asset, "foo.tar.gz");
        assert!(state.versions.is_empty());
    }

    #[test]
    fn test_remove_cached_empty_state() {
        let mut state = State::default();
        assert!(state.remove_cached("no/such").is_none());
    }

    #[test]
    fn test_remove_cached_idempotent() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", String::new(), "");
        assert!(state.remove_cached("owner/repo").is_some());
        assert!(state.remove_cached("owner/repo").is_none());
    }

    #[test]
    fn test_cached_release_destination_deserialization() {
        let with_dest = r#"tag = "v1.0.0"
asset = "foo.tar.gz"
destination = "/usr/local/bin"
"#;
        let parsed: CachedRelease = toml::from_str(with_dest).unwrap();
        assert_eq!(parsed.destination, "/usr/local/bin".to_string());

        let without_dest = r#"tag = "v1.0.0"
asset = "foo.tar.gz"
"#;
        let result: Result<CachedRelease, _> = toml::from_str(without_dest);
        assert!(result.is_err());
    }

    #[test]
    fn test_cached_release_binary_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_cached(
            "sharkdp/fd",
            "fd-v10.4.2-x86_64-unknown-linux-gnu.tar.gz",
            "v10.4.2",
            "/usr/local/bin".to_string(),
            "fd2",
        );
        state.save().unwrap();

        let loaded = State::load();
        let entry = loaded.get_cached("sharkdp/fd").unwrap();
        assert_eq!(entry.binary.as_deref(), Some("fd2"));
        assert_eq!(entry.installed_filename("sharkdp/fd"), "fd2");
    }

    #[test]
    fn test_cached_release_without_binary_falls_back_to_repo_basename() {
        // State written before `--rename` existed has no `binary` key.
        let legacy = r#"tag = "v1.0.0"
asset = "foo.tar.gz"
destination = "/usr/local/bin"
"#;
        let parsed: CachedRelease = toml::from_str(legacy).unwrap();
        assert!(parsed.binary.is_none());

        let expected = if cfg!(windows) {
            "repo.exe".to_string()
        } else {
            "repo".to_string()
        };
        assert_eq!(parsed.installed_filename("owner/repo"), expected);
    }

    #[test]
    fn test_installed_filename_prefers_stored_value() {
        let entry = CachedRelease {
            tag: "v1.0.0".to_string(),
            asset: "foo.tar.gz".to_string(),
            destination: "/usr/local/bin".to_string(),
            binary: Some("renamed".to_string()),
        };
        assert_eq!(entry.installed_filename("owner/repo"), "renamed");
    }

    #[test]
    fn test_installed_filename_fallback_uses_last_repo_segment() {
        let entry = CachedRelease {
            tag: "v1.0.0".to_string(),
            asset: "foo.tar.gz".to_string(),
            destination: String::new(),
            binary: None,
        };
        let expected = if cfg!(windows) {
            "tool.exe".to_string()
        } else {
            "tool".to_string()
        };
        assert_eq!(entry.installed_filename("owner/tool"), expected);
        assert_eq!(entry.installed_filename("tool"), expected);
    }

    #[test]
    fn test_binary_serialized_only_when_set() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", String::new(), "");
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(content.contains("binary"));

        // A None binary is omitted entirely, keeping the file backward compatible.
        let without = CachedRelease {
            tag: "v1".to_string(),
            asset: "a.tar.gz".to_string(),
            destination: String::new(),
            binary: None,
        };
        let content = toml::to_string_pretty(&without).unwrap();
        assert!(!content.contains("binary"));
    }

    #[test]
    fn test_default_install_dir_default_is_none() {
        let state = State::default();
        assert!(state.default_install_dir.is_none());
    }

    #[test]
    fn test_set_default_install_dir() {
        let mut state = State::default();
        state.set_default_install_dir("/usr/local/bin");
        assert_eq!(state.default_install_dir.as_deref(), Some("/usr/local/bin"));
    }

    #[test]
    fn test_clear_default_install_dir() {
        let mut state = State::default();
        state.set_default_install_dir("/usr/local/bin");
        state.clear_default_install_dir();
        assert_eq!(state.default_install_dir, None);
        assert_eq!(state.get_default_install_dir(), None);
    }

    #[test]
    fn test_get_default_install_dir() {
        let mut state = State::default();
        assert_eq!(state.get_default_install_dir(), None);
        state.set_default_install_dir("~/.local/bin");
        assert_eq!(state.get_default_install_dir(), Some("~/.local/bin"));
    }

    #[test]
    fn test_default_install_dir_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_default_install_dir("/opt/bin");
        state.save().unwrap();

        let loaded = State::load();
        assert_eq!(loaded.default_install_dir.as_deref(), Some("/opt/bin"));
    }

    #[test]
    fn test_default_install_dir_serialized_only_when_set() {
        let state = State::default();
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(!content.contains("default_install_dir"));

        let mut state = State::default();
        state.set_default_install_dir("/tmp");
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(content.contains("default_install_dir"));
    }

    #[test]
    fn test_github_pat_defaults_to_none() {
        let state = State::default();
        assert_eq!(state.get_github_pat(), None);
    }

    #[test]
    fn test_set_and_clear_github_pat() {
        let mut state = State::default();
        state.set_github_pat("ghp_0123456789abcdefghijklmnopqrstuv");
        assert_eq!(
            state.get_github_pat(),
            Some("ghp_0123456789abcdefghijklmnopqrstuv")
        );
        state.clear_github_pat();
        assert_eq!(state.get_github_pat(), None);
    }

    #[test]
    fn test_github_pat_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_github_pat("ghp_0123456789abcdefghijklmnopqrstuv");
        state.save().unwrap();

        let loaded = State::load();
        assert_eq!(
            loaded.get_github_pat(),
            Some("ghp_0123456789abcdefghijklmnopqrstuv")
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_save_error_includes_state_file_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        // A directory in place of the state file makes the final write fail.
        fs::create_dir(&state_path).unwrap();
        let err = State::default().save().unwrap_err().to_string();
        assert!(
            err.contains("state.toml"),
            "error should name the file: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_state_file_is_owner_only_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_github_pat("ghp_0123456789abcdefghijklmnopqrstuv");
        state.save().unwrap();

        let mode = fs::metadata(&state_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        // A pre-existing wider-mode file must be tightened on rewrite.
        fs::set_permissions(&state_path, fs::Permissions::from_mode(0o644)).unwrap();
        State::load().save().unwrap();
        let mode = fs::metadata(&state_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_github_pat_serialized_only_when_set() {
        let state = State::default();
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(!content.contains("github_pat"));

        let mut state = State::default();
        state.set_github_pat("ghp_0123456789abcdefghijklmnopqrstuv");
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(content.contains("github_pat"));
    }

    #[test]
    fn test_toml_format() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", String::new(), "");
        let content = toml::to_string_pretty(&state).unwrap();
        // serializes as nested table, e.g. ["versions"."owner/repo"]
        assert!(content.contains("versions"));
        assert!(content.contains("tag"));
        assert!(content.contains("asset"));
        assert!(content.contains("foo.tar.gz"));
        assert!(content.contains("v1.0.0"));
    }
}
