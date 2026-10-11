//! Platform-aware executable naming.
//!
//! The platform that matters is the *selected* one (`--os`, else the host),
//! carried around as the normalized OS string — never `cfg!(windows)`, which
//! only reflects the machine grd happens to run on.

/// True when the selected target platform is Windows.
pub fn is_windows(os: &str) -> bool {
    os == "windows"
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
}
