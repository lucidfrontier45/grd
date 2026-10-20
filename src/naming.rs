//! Platform-aware executable naming.
//!
//! The platform that matters is the *selected* one (`--os`, else the host),
//! carried around as the normalized OS string — never `cfg!(windows)`, which
//! only reflects the machine grd happens to run on.

/// The selected target platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform<'a> {
    /// Normalized OS name (`windows`, `macos`, `linux`).
    pub os: &'a str,
    /// Normalized architecture name (`x86_64`, `aarch64`, ...).
    pub arch: &'a str,
}

impl<'a> Platform<'a> {
    /// The platform pair for the given OS and architecture.
    pub fn new(os: &'a str, arch: &'a str) -> Self {
        Self { os, arch }
    }
}

/// True when the selected target platform is Windows.
pub fn is_windows(os: &str) -> bool {
    os == "windows"
}

/// The last path component of `name` (`a/b/c` → `c`).
///
/// Both separators are handled, so archive paths work the same on every
/// platform. A name made of separators only yields an empty string.
pub fn base_name(name: &str) -> &str {
    let trimmed = name.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or("")
}

/// Drop a trailing `.exe` (any case) so `fd.exe` and `fd` share a stem.
pub fn strip_exe_suffix(name: &str) -> &str {
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe") {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// On-disk name for an explicit `--rename`.
///
/// A Windows target always carries a single `.exe` suffix; other targets keep
/// the value verbatim.
pub fn rename_output(rename: &str, os: &str) -> String {
    if is_windows(os) {
        format!("{}.exe", strip_exe_suffix(rename))
    } else {
        rename.to_string()
    }
}

/// On-disk name for the installed binary.
///
/// An explicit `--rename` wins; otherwise the lookup name's last path
/// component is used, with the Windows `.exe` rule applied. The archive
/// entry's own name never decides the output name.
pub fn installed_name(rename: Option<&str>, lookup_name: &str, os: &str) -> String {
    match rename {
        Some(rename) => rename_output(rename, os),
        None => rename_output(base_name(lookup_name), os),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_windows() {
        assert!(is_windows("windows"));
        assert!(!is_windows("linux"));
        assert!(!is_windows("macos"));
    }

    #[test]
    fn test_strip_exe_suffix_is_case_insensitive() {
        assert_eq!(strip_exe_suffix("fd.exe"), "fd");
        assert_eq!(strip_exe_suffix("fd.EXE"), "fd");
        assert_eq!(strip_exe_suffix("fd.Exe"), "fd");
    }

    #[test]
    fn test_strip_exe_suffix_leaves_other_names_alone() {
        assert_eq!(strip_exe_suffix("fd"), "fd");
        assert_eq!(strip_exe_suffix("fd.tar.gz"), "fd.tar.gz");
        assert_eq!(strip_exe_suffix("fd.exe.bak"), "fd.exe.bak");
        // A bare ".exe" has no stem left to keep.
        assert_eq!(strip_exe_suffix(".exe"), ".exe");
    }

    #[test]
    fn test_rename_output_appends_exe_on_windows_target() {
        assert_eq!(rename_output("tool", "windows"), "tool.exe");
        assert_eq!(rename_output("tool.exe", "windows"), "tool.exe");
        assert_eq!(rename_output("tool.EXE", "windows"), "tool.exe");
    }

    #[test]
    fn test_rename_output_verbatim_off_windows() {
        assert_eq!(rename_output("tool", "linux"), "tool");
        assert_eq!(rename_output("tool.exe", "linux"), "tool.exe");
    }

    #[test]
    fn test_base_name_takes_the_last_component() {
        assert_eq!(base_name("app"), "app");
        assert_eq!(base_name("a/b/app"), "app");
        assert_eq!(base_name("a\\b\\app"), "app");
        assert_eq!(base_name("app-1.0/bin/app"), "app");
    }

    #[test]
    fn test_base_name_ignores_trailing_separators() {
        assert_eq!(base_name("a/b/"), "b");
        assert_eq!(base_name("a/b\\"), "b");
    }

    #[test]
    fn test_base_name_of_separators_only_is_empty() {
        assert_eq!(base_name(""), "");
        assert_eq!(base_name("/"), "");
        assert_eq!(base_name("\\"), "");
    }

    #[test]
    fn test_installed_name_prefers_rename() {
        assert_eq!(installed_name(Some("other"), "app-bin", "linux"), "other");
    }

    #[test]
    fn test_installed_name_falls_back_to_lookup_name() {
        assert_eq!(installed_name(None, "app-bin", "linux"), "app-bin");
        assert_eq!(installed_name(None, "pkg/bin/app", "linux"), "app");
    }

    #[test]
    fn test_installed_name_appends_exe_on_windows() {
        assert_eq!(installed_name(None, "app-bin", "windows"), "app-bin.exe");
        assert_eq!(
            installed_name(None, "app-bin.exe", "windows"),
            "app-bin.exe"
        );
        assert_eq!(
            installed_name(Some("tool"), "app-bin", "windows"),
            "tool.exe"
        );
    }
}
