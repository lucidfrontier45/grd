use std::{collections::HashMap, env, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct State {
    #[serde(rename = "versions")]
    pub versions: HashMap<String, String>,
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

    pub fn get_version(&self, repo: &str) -> Option<&str> {
        self.versions.get(repo).map(|s| s.as_str())
    }

    pub fn set_version(&mut self, repo: &str, tag: &str) {
        self.versions.insert(repo.to_string(), tag.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

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
                    env::set_var("GRD_STATE_PATH", previous);
                },
                None => unsafe {
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
    fn test_set_and_get_version() {
        let mut state = State::default();
        state.set_version("owner/repo", "v1.0.0");
        assert_eq!(state.get_version("owner/repo"), Some("v1.0.0"));
    }

    #[test]
    fn test_get_nonexistent_repo() {
        let state = State::default();
        assert_eq!(state.get_version("no/such"), None);
    }

    #[test]
    fn test_set_overwrites_previous() {
        let mut state = State::default();
        state.set_version("owner/repo", "v1.0.0");
        state.set_version("owner/repo", "v2.0.0");
        assert_eq!(state.get_version("owner/repo"), Some("v2.0.0"));
    }

    #[test]
    fn test_multiple_repos() {
        let mut state = State::default();
        state.set_version("a/x", "v1");
        state.set_version("b/y", "v2");
        assert_eq!(state.get_version("a/x"), Some("v1"));
        assert_eq!(state.get_version("b/y"), Some("v2"));
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let state_path = dir.path().join("state.toml");
        let _env = StatePathEnvGuard::set(&state_path);

        let mut state = State::default();
        state.set_version("owner/repo", "v1.0.0");
        state.set_version("other/repo", "v2.3.1");
        state.save();

        let loaded = State::load();
        assert_eq!(loaded.get_version("owner/repo"), Some("v1.0.0"));
        assert_eq!(loaded.get_version("other/repo"), Some("v2.3.1"));
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
    fn test_toml_format() {
        let mut state = State::default();
        state.set_version("owner/repo", "v1.0.0");
        let content = toml::to_string_pretty(&state).unwrap();
        assert!(content.contains("[versions]"));
        assert!(content.contains("owner/repo"));
        assert!(content.contains("v1.0.0"));
    }
}
