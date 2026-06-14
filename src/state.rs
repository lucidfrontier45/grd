use std::{collections::HashMap, env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct CachedRelease {
    pub tag: String,
    pub asset: String,
    #[serde(default)]
    pub destination: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct State {
    #[serde(rename = "versions")]
    pub versions: HashMap<String, CachedRelease>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_install_dir: Option<String>,
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

    pub fn save(&self) {
        let path = match Self::state_path() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Warning: cannot determine home directory: {}", e);
                return;
            }
        };

        let Some(parent) = path.parent() else {
            eprintln!("Warning: state file {:?} has no parent directory", path);
            return;
        };
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!(
                "Warning: failed to create state directory {:?}: {}",
                parent, e
            );
            return;
        }

        let content = match toml::to_string_pretty(self) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: failed to serialize state: {}", e);
                return;
            }
        };

        if let Err(e) = fs::write(&path, content) {
            eprintln!("Warning: failed to write state file {:?}: {}", path, e);
        }
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

    pub fn set_cached(
        &mut self,
        repo: &str,
        asset_name: &str,
        tag: &str,
        destination: Option<String>,
    ) {
        self.versions.insert(
            repo.to_string(),
            CachedRelease {
                tag: tag.to_string(),
                asset: asset_name.to_string(),
                destination,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use super::*;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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
        state.set_cached("owner/repo", "foo-linux.tar.gz", "v1.0.0", None);
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
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", None);
        state.set_cached("owner/repo", "bar.tar.gz", "v2.0.0", None);
        let cached = state.get_cached("owner/repo").unwrap();
        assert_eq!(cached.tag, "v2.0.0");
        assert_eq!(cached.asset, "bar.tar.gz");
    }

    #[test]
    fn test_diff_repos_independent() {
        let mut state = State::default();
        state.set_cached("a/x", "asset-a.tar.gz", "v1", None);
        state.set_cached("b/y", "asset-b.tar.gz", "v2", None);
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
        state.set_cached("owner/repo", "foo-linux.tar.gz", "v1.0.0", None);
        state.set_cached("owner/repo", "foo-macos.tar.gz", "v1.0.0", None);
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
        state.set_cached("owner/repo", "foo-linux.tar.gz", "v1.0.0", None);
        state.set_cached("other/repo", "bar-macos.zip", "v2.3.1", None);
        state.save();

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
    fn test_remove_cached_returns_entry() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", None);
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
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", None);
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
        assert_eq!(parsed.destination, Some("/usr/local/bin".to_string()));

        let without_dest = r#"tag = "v1.0.0"
asset = "foo.tar.gz"
"#;
        let parsed: CachedRelease = toml::from_str(without_dest).unwrap();
        assert_eq!(parsed.destination, None);
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
        state.save();

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
    fn test_toml_format() {
        let mut state = State::default();
        state.set_cached("owner/repo", "foo.tar.gz", "v1.0.0", None);
        let content = toml::to_string_pretty(&state).unwrap();
        // serializes as nested table, e.g. ["versions"."owner/repo"]
        assert!(content.contains("versions"));
        assert!(content.contains("tag"));
        assert!(content.contains("asset"));
        assert!(content.contains("foo.tar.gz"));
        assert!(content.contains("v1.0.0"));
    }
}
