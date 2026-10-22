use std::{
    fs::{self, File},
    io::{self, BufRead, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use tempfile::NamedTempFile;
use zip::ZipArchive;

use crate::{
    asset,
    download::DownloadSource,
    naming::{Platform, base_name, is_windows, strip_exe_suffix},
};

/// A download source that can be read more than once.
///
/// The platform-suffix lookup needs a second pass over a tar archive when the
/// binary is not found by an exact match, so the bytes (or the temp file) are
/// kept around instead of being consumed by the first pass.
enum ArchiveSource {
    Memory(Vec<u8>),
    Disk(NamedTempFile),
}

impl ArchiveSource {
    fn new(source: DownloadSource) -> Self {
        match source {
            DownloadSource::Memory(bytes) => Self::Memory(bytes),
            DownloadSource::Disk(file) => Self::Disk(file),
        }
    }

    /// A fresh reader over the source bytes.
    fn reader(&self) -> Result<Box<dyn Read + '_>> {
        Ok(match self {
            Self::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
            Self::Disk(file) => Box::new(File::open(file.path())?),
        })
    }

    /// A fresh buffered reader over the source bytes.
    fn buf_reader(&self) -> Result<Box<dyn BufRead + '_>> {
        Ok(match self {
            Self::Memory(bytes) => Box::new(io::BufReader::new(io::Cursor::new(bytes))),
            Self::Disk(file) => Box::new(io::BufReader::new(File::open(file.path())?)),
        })
    }

    /// A fresh seekable reader over the source bytes.
    fn read_seek(&self) -> Result<Box<dyn ReadSeek + '_>> {
        Ok(match self {
            Self::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
            Self::Disk(file) => Box::new(File::open(file.path())?),
        })
    }
}

/// An archive entry that matched the lookup name by a platform suffix.
struct SuffixCandidate {
    /// Path of the entry inside the archive.
    path: String,
    /// Platform match score of the entry's basename.
    score: i32,
}

/// True when `entry_path` refers to `lookup_name`.
///
/// The suffix must start on a path-component boundary (start of path or right
/// after `/` / `\\`), so nested lookups like `pkg/bin/app` still match
/// `dist/pkg/bin/app`, while collisions such as `LICENSE.pi` never match
/// `pi` (nor does `xpi`).
fn is_lookup_match(entry_path: &str, lookup_name: &str) -> bool {
    if !entry_path.ends_with(lookup_name) {
        return false;
    }
    let boundary = entry_path.len() - lookup_name.len();
    boundary == 0 || entry_path[..boundary].ends_with(['/', '\\'])
}

/// True when `entry_path` selects `lookup_name` for extraction.
///
/// On a Windows target the release archive carries the executable's `.exe`
/// suffix while the default lookup name (repository basename) does not, so the
/// suffix is ignored there. Elsewhere only the exact component-boundary suffix
/// matches.
fn is_entry_match(entry_path: &str, lookup_name: &str, os: &str) -> bool {
    if is_lookup_match(entry_path, lookup_name) {
        return true;
    }
    is_windows(os) && is_lookup_match(strip_exe_suffix(entry_path), strip_exe_suffix(lookup_name))
}

/// True when `entry_path` is a platform-suffixed candidate for `lookup_name`.
///
/// The entry's basename must start with the lookup basename and carry more
/// than it, so `app-bin` matches `app-bin-linux-x64`. The suffix must either
/// name the target OS or be tag-shaped (`-`/`_` separated), which keeps `pi`
/// from matching an unrelated `pilot`.
///
/// On a Windows target the `.exe` suffix is ignored on both sides, like in
/// the exact-match rule, so `app-bin.exe` finds `app-bin-windows-x64.exe`.
fn is_suffix_candidate(entry_path: &str, lookup_name: &str, os: &str) -> bool {
    let entry_base = base_name(entry_path);
    let lookup_base = base_name(lookup_name);
    let (entry_stem, lookup_stem) = if is_windows(os) {
        (strip_exe_suffix(entry_base), strip_exe_suffix(lookup_base))
    } else {
        (entry_base, lookup_base)
    };
    if lookup_stem.is_empty() || !entry_stem.starts_with(lookup_stem) {
        return false;
    }
    let suffix = &entry_stem[lookup_stem.len()..];
    if suffix.is_empty() {
        // An exact basename is the exact-match rule's business.
        return false;
    }
    if asset::names_os(entry_base, os) {
        return true;
    }
    suffix.starts_with('-') || suffix.starts_with('_')
}

/// The best platform-suffixed candidate for the target platform.
///
/// `Ok(None)` when the archive holds no candidate at all. An error when
/// several candidates share the best score: the archive holds more than one
/// plausible binary and guessing would be worse than asking. A lone candidate
/// that names a platform other than the target still wins — the archive was
/// platform-matched at the asset level, so the entry is probably right — but
/// a warning is printed so the mismatch is not silent.
fn select_suffix_candidate<'a>(
    candidates: &'a [SuffixCandidate],
    lookup_name: &str,
    os: &str,
) -> Result<Option<&'a SuffixCandidate>> {
    let Some(best) = candidates.iter().max_by_key(|candidate| candidate.score) else {
        return Ok(None);
    };
    let tied: Vec<&SuffixCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.score == best.score)
        .collect();
    if tied.len() > 1 {
        let listing = tied
            .iter()
            .map(|candidate| format!("  - {}", candidate.path))
            .collect::<Vec<_>>()
            .join("\n");
        bail!(
            "Multiple executables match '{}':\n{}\nPass a more specific --bin-name.",
            lookup_name,
            listing
        );
    }
    let base = base_name(&best.path);
    if let Some(other) = asset::other_named_os(base, os) {
        eprintln!("Warning: '{base}' looks like a {other} build but the target is {os}.");
        eprintln!("Pass a more specific --bin-name to override.");
    }
    Ok(Some(best))
}

/// Extract a downloaded asset into `dest_dir` and return the written filename.
///
/// `lookup_name` selects which archive entry is extracted: an exact match
/// wins, otherwise a platform-suffixed entry (`app-bin-linux-x64` for
/// `app-bin`) is picked by platform score. `out_name` is the file name to
/// write; the caller decides it once — `--rename` > `--bin-name` > the
/// repository basename, see `main::resolve_out_name` — and the extractor
/// never re-derives it. `platform` is the selected target platform.
pub fn extract_and_save(
    source: DownloadSource,
    filename: &str,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
    no_decompress: bool,
    platform: Platform<'_>,
) -> Result<String> {
    fs::create_dir_all(dest_dir).context("Failed to create destination directory")?;

    let source = ArchiveSource::new(source);

    if no_decompress {
        save_raw(&source, out_name, dest_dir)?;
        println!("Saved raw asset to {:?}", dest_dir.join(out_name));
        return Ok(out_name.to_string());
    }

    if filename.ends_with(".zip") {
        extract_zip(&source, lookup_name, out_name, dest_dir, platform)
    } else if filename.ends_with(".tar.xz") {
        extract_tar_xz(&source, lookup_name, out_name, dest_dir, platform)
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(&source, lookup_name, out_name, dest_dir, platform)
    } else {
        // Not an archive (e.g. a bare `.exe`): the asset is the binary itself.
        save_raw(&source, out_name, dest_dir)?;
        Ok(out_name.to_string())
    }
}

fn extract_zip(
    source: &ArchiveSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
    platform: Platform<'_>,
) -> Result<String> {
    let (os, arch) = (platform.os, platform.arch);
    let mut archive =
        ZipArchive::new(source.read_seek()?).context("Failed to parse ZIP archive")?;
    let mut candidates: Vec<SuffixCandidate> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .context("Failed to read ZIP entry name")?
            .into_owned();
        if is_entry_match(&name, lookup_name, os) {
            write_zip_entry(&mut file, dest_dir, out_name)?;
            return Ok(out_name.to_string());
        }
        if is_suffix_candidate(&name, lookup_name, os) {
            let score = asset::platform_score(base_name(&name), os, arch);
            candidates.push(SuffixCandidate { path: name, score });
        }
    }

    let Some(candidate) = select_suffix_candidate(&candidates, lookup_name, os)? else {
        bail!("Executable '{}' not found in archive", lookup_name);
    };

    // The suffix match needs a second look: re-open the winning entry.
    let mut file = archive
        .by_name(&candidate.path)
        .with_context(|| format!("Executable '{}' not found in archive", lookup_name))?;
    write_zip_entry(&mut file, dest_dir, out_name)?;
    Ok(out_name.to_string())
}

fn extract_tar_gz(
    source: &ArchiveSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
    platform: Platform<'_>,
) -> Result<String> {
    let (os, arch) = (platform.os, platform.arch);
    let mut candidates: Vec<SuffixCandidate> = Vec::new();

    {
        let rdr = source.reader()?;
        let mut archive = tar::Archive::new(GzDecoder::new(rdr));
        for entry in archive.entries().context("Failed to read tar archive")? {
            let file = entry.context("Failed to read tar entry")?;
            let path = file.path()?.to_path_buf();
            if file.header().entry_type().is_dir() {
                continue;
            }
            let path = path.to_string_lossy().into_owned();
            if is_entry_match(&path, lookup_name, os) {
                write_tar_entry(file, dest_dir, out_name)?;
                return Ok(out_name.to_string());
            }
            if is_suffix_candidate(&path, lookup_name, os) {
                let score = asset::platform_score(base_name(&path), os, arch);
                candidates.push(SuffixCandidate { path, score });
            }
        }
    }

    let Some(candidate) = select_suffix_candidate(&candidates, lookup_name, os)? else {
        bail!("Executable '{}' not found in archive", lookup_name);
    };

    // The suffix match needs a second pass over the archive.
    let rdr = source.reader()?;
    let mut archive = tar::Archive::new(GzDecoder::new(rdr));
    for entry in archive.entries().context("Failed to read tar archive")? {
        let file = entry.context("Failed to read tar entry")?;
        let path = file.path()?.to_path_buf();
        if file.header().entry_type().is_dir() {
            continue;
        }
        if path.to_string_lossy().as_ref() == candidate.path {
            write_tar_entry(file, dest_dir, out_name)?;
            return Ok(out_name.to_string());
        }
    }
    bail!("Executable '{}' not found in archive", lookup_name)
}

fn extract_tar_xz(
    source: &ArchiveSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
    platform: Platform<'_>,
) -> Result<String> {
    let (os, arch) = (platform.os, platform.arch);
    let mut candidates: Vec<SuffixCandidate> = Vec::new();

    // The suffix match may need a second pass, so the decompressed tar stays
    // on disk: one xz decompression serves both passes.
    let decompressed = xz_temp_file(source)?;
    {
        let mut archive = tar::Archive::new(decompressed.reopen()?);
        for entry in archive.entries().context("Failed to read tar archive")? {
            let file = entry.context("Failed to read tar entry")?;
            let path = file.path()?.to_path_buf();
            if file.header().entry_type().is_dir() {
                continue;
            }
            let path = path.to_string_lossy().into_owned();
            if is_entry_match(&path, lookup_name, os) {
                write_tar_entry(file, dest_dir, out_name)?;
                return Ok(out_name.to_string());
            }
            if is_suffix_candidate(&path, lookup_name, os) {
                let score = asset::platform_score(base_name(&path), os, arch);
                candidates.push(SuffixCandidate { path, score });
            }
        }
    }

    let Some(candidate) = select_suffix_candidate(&candidates, lookup_name, os)? else {
        bail!("Executable '{}' not found in archive", lookup_name);
    };

    // The suffix match needs a second pass over the decompressed tar.
    let mut archive = tar::Archive::new(decompressed.reopen()?);
    for entry in archive.entries().context("Failed to read tar archive")? {
        let file = entry.context("Failed to read tar entry")?;
        let path = file.path()?.to_path_buf();
        if file.header().entry_type().is_dir() {
            continue;
        }
        if path.to_string_lossy().as_ref() == candidate.path {
            write_tar_entry(file, dest_dir, out_name)?;
            return Ok(out_name.to_string());
        }
    }
    bail!("Executable '{}' not found in archive", lookup_name)
}

/// Decompress the xz payload into a temporary file, so the archive can be
/// read without holding it in memory.
fn xz_temp_file(source: &ArchiveSource) -> Result<NamedTempFile> {
    let mut compressed = source.buf_reader()?;
    let mut decompressed =
        NamedTempFile::new().context("Failed to create temp file for xz decompression")?;
    lzma_rs::xz_decompress(&mut compressed, decompressed.as_file_mut())
        .context("Failed to decompress xz archive")?;
    drop(compressed);
    Ok(decompressed)
}

/// Write a ZIP entry to `dest_dir` under `out_name`.
fn write_zip_entry<R: Read + ?Sized>(
    file: &mut zip::read::ZipFile<'_, R>,
    dest_dir: &Path,
    out_name: &str,
) -> Result<()> {
    let out_path = dest_dir.join(out_name);
    let mut outfile = File::create(&out_path).context("Failed to create output file")?;
    io::copy(file, &mut outfile).context("Failed to write extracted file")?;
    #[cfg(unix)]
    set_permissions(&out_path)?;
    Ok(())
}

/// Write a tar entry to `dest_dir` under `out_name`.
fn write_tar_entry<R: Read>(
    mut file: tar::Entry<'_, R>,
    dest_dir: &Path,
    out_name: &str,
) -> Result<()> {
    let out_path = dest_dir.join(out_name);
    file.unpack(&out_path)
        .context("Failed to unpack tar entry")?;
    #[cfg(unix)]
    set_permissions(&out_path)?;
    Ok(())
}

fn save_raw(source: &ArchiveSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let out_path = dest_dir.join(target_bin_name);
    match source {
        ArchiveSource::Memory(bytes) => {
            fs::write(&out_path, bytes).context("Failed to write file")?;
        }
        ArchiveSource::Disk(temp_file) => {
            fs::copy(temp_file.path(), &out_path).context("Failed to copy file")?;
        }
    }
    #[cfg(unix)]
    set_permissions(&out_path)?;
    Ok(())
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek + ?Sized> ReadSeek for T {}

#[cfg(unix)]
fn set_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    #[test]
    fn test_readseek_trait() {
        let data = vec![1, 2, 3, 4, 5];
        let cursor = Cursor::new(data);
        let _: Box<dyn ReadSeek> = Box::new(cursor);
    }

    #[test]
    fn test_save_raw_memory() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![1, 2, 3, 4, 5]);

        let result = save_raw(&ArchiveSource::new(source), "test.bin", dest_dir);
        assert!(result.is_ok());

        let file_path = dest_dir.join("test.bin");
        assert!(file_path.exists());

        let content = fs::read(file_path).unwrap();
        assert_eq!(content, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_save_raw_disk() {
        use tempfile::{NamedTempFile, TempDir};

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"test content").unwrap();
        let source = DownloadSource::Disk(temp_file);

        let result = save_raw(&ArchiveSource::new(source), "test.bin", dest_dir);
        assert!(result.is_ok());

        let file_path = dest_dir.join("test.bin");
        assert!(file_path.exists());

        let content = fs::read(file_path).unwrap();
        assert_eq!(content, b"test content");
    }

    #[test]
    fn test_extract_tar_xz() {
        use tempfile::TempDir;

        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_path("my-app").unwrap();
            header.set_size(5);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, &b"hello"[..]).unwrap();
            builder.finish().unwrap();
        }

        let mut compressed = Vec::new();
        lzma_rs::xz_compress(&mut io::Cursor::new(&tar_bytes), &mut compressed)
            .expect("xz compression should succeed");

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let source = DownloadSource::Memory(compressed);

        let result = extract_tar_xz(
            &ArchiveSource::new(source),
            "my-app",
            "my-app",
            dest,
            Platform::new("linux", "x86_64"),
        );
        assert!(
            result.is_ok(),
            "extract_tar_xz failed: {:?}",
            result.as_ref().err()
        );

        let out_path = dest.join("my-app");
        assert!(out_path.exists());
        let content = fs::read(out_path).unwrap();
        assert_eq!(content, b"hello");
    }

    #[test]
    fn test_extract_tar_xz_not_found() {
        use tempfile::TempDir;

        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_path("other-binary").unwrap();
            header.set_size(4);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, &b"test"[..]).unwrap();
            builder.finish().unwrap();
        }

        let mut compressed = Vec::new();
        lzma_rs::xz_compress(&mut io::Cursor::new(&tar_bytes), &mut compressed)
            .expect("xz compression should succeed");

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let source = DownloadSource::Memory(compressed);

        let result = extract_tar_xz(
            &ArchiveSource::new(source),
            "my-app",
            "my-app",
            dest,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_tar_xz_corrupted() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let corrupted_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00];
        let source = DownloadSource::Memory(corrupted_data);

        let result = extract_tar_xz(
            &ArchiveSource::new(source),
            "my-app",
            "my-app",
            dest,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("decompress") || err_msg.contains("xz"),
            "Error message should mention decompression failure: {}",
            err_msg
        );
    }

    #[test]
    fn test_extract_and_save_tar_xz_no_decompress() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let source = DownloadSource::Memory(vec![1, 2, 3]);

        let result = extract_and_save(
            source,
            "foo.tar.xz",
            "app",
            "foo.tar.xz",
            dest,
            true,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_ok());

        let file_path = dest.join("foo.tar.xz");
        assert!(file_path.exists());
    }

    #[test]
    fn test_extract_and_save_no_decompress() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![1, 2, 3, 4, 5]);

        let result = extract_and_save(
            source,
            "test.bin",
            "app",
            "test.bin",
            dest_dir,
            true,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_ok());

        let file_path = dest_dir.join("test.bin");
        assert!(file_path.exists());
    }

    #[test]
    fn test_bare_asset_is_saved_under_out_name_on_windows() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![]);

        let written = extract_and_save(
            source,
            "test.bin",
            "app",
            "app.exe",
            dest_dir,
            false,
            Platform::new("windows", "x86_64"),
        )
        .unwrap();
        assert_eq!(written, "app.exe");
        assert!(dest_dir.join("app.exe").exists());
    }

    #[test]
    fn test_bare_asset_is_saved_under_out_name_off_windows() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![]);

        let written = extract_and_save(
            source,
            "test.bin",
            "app",
            "app",
            dest_dir,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();
        assert_eq!(written, "app");
        assert!(dest_dir.join("app").exists());
    }

    #[test]
    #[cfg(unix)]
    fn test_set_permissions() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.bin");
        fs::write(&file_path, b"test").unwrap();

        let result = set_permissions(&file_path);
        assert!(result.is_ok());

        let perms = fs::metadata(&file_path).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(perms.mode() & 0o777, 0o755);
        }
    }

    /// Build a single-entry `.tar.gz` holding `entry_name` with `content`.
    fn tar_gz_with(entry_name: &str, content: &[u8]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            let mut header = tar::Header::new_gnu();
            header.set_path(entry_name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, content).unwrap();
            builder.finish().unwrap();
        }

        let mut compressed = Vec::new();
        flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default())
            .write_all(&tar_bytes)
            .expect("gzip compression should succeed");
        compressed
    }

    /// Raw (uncompressed) tar bytes for [`tar_gz_with_entries`] / [`tar_xz_with_entries`].
    fn tar_bytes_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (name, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_path(name).unwrap();
                header.set_mode(0o755);
                if name.ends_with('/') {
                    header.set_entry_type(tar::EntryType::Directory);
                    header.set_size(0);
                } else {
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_size(content.len() as u64);
                }
                header.set_cksum();
                builder.append(&header, *content).unwrap();
            }
            builder.finish().unwrap();
        }
        tar_bytes
    }

    /// Build a multi-entry `.tar.gz`; names ending in `/` become directories.
    fn tar_gz_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let tar_bytes = tar_bytes_with_entries(entries);
        let mut compressed = Vec::new();
        flate2::write::GzEncoder::new(&mut compressed, flate2::Compression::default())
            .write_all(&tar_bytes)
            .expect("gzip compression should succeed");
        compressed
    }

    /// Build a multi-entry `.tar.xz`; names ending in `/` become directories.
    fn tar_xz_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let tar_bytes = tar_bytes_with_entries(entries);
        let mut compressed = Vec::new();
        lzma_rs::xz_compress(&mut io::Cursor::new(&tar_bytes), &mut compressed)
            .expect("xz compression should succeed");
        compressed
    }

    /// Build a multi-entry `.zip`; names ending in `/` become directories.
    fn zip_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use zip::write::SimpleFileOptions;

        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            for (name, content) in entries {
                if name.ends_with('/') {
                    writer
                        .add_directory(*name, SimpleFileOptions::default())
                        .unwrap();
                } else {
                    writer
                        .start_file(*name, SimpleFileOptions::default())
                        .unwrap();
                    writer.write_all(content).unwrap();
                }
            }
            writer.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_is_lookup_match_component_boundary() {
        // Exact and component-boundary matches.
        assert!(is_lookup_match("pi", "pi"));
        assert!(is_lookup_match("./pi", "pi"));
        assert!(is_lookup_match("pi-bolt-linux-x64/pi", "pi"));
        // Nested lookups still match under a different top-level directory.
        assert!(is_lookup_match("dist/pkg/bin/app", "pkg/bin/app"));
        // Suffix collisions on the same component must never match.
        assert!(!is_lookup_match("pi-bolt-linux-x64/LICENSE.pi", "pi"));
        assert!(!is_lookup_match("api", "pi"));
        assert!(!is_lookup_match("xpi", "pi"));
        assert!(!is_lookup_match("i", "pi"));
    }

    #[test]
    fn test_is_entry_match_accepts_exe_candidate_on_windows() {
        assert!(is_entry_match("a", "a", "windows"));
        assert!(is_entry_match("a.exe", "a", "windows"));
        assert!(is_entry_match("app-0.1/a.exe", "a", "windows"));
        assert!(is_entry_match("dist/pkg/a.exe", "pkg/a", "windows"));
        assert!(is_entry_match("a.exe", "a.exe", "windows"));
        // Boundary and stem collisions must still be rejected.
        assert!(!is_entry_match("xa.exe", "a", "windows"));
        assert!(!is_entry_match("a.exe.bak", "a", "windows"));
        assert!(!is_entry_match("LICENSE.pi", "pi", "windows"));
    }

    #[test]
    fn test_is_entry_match_is_exact_off_windows() {
        assert!(is_entry_match("a", "a", "linux"));
        assert!(!is_entry_match("a.exe", "a", "linux"));
    }

    #[test]
    fn test_extract_zip_finds_exe_entry_on_windows_target() {
        use tempfile::TempDir;

        // Repo basename is `a`, so the default lookup is `a`; the Windows
        // archive ships the executable as `a.exe`.
        let source = DownloadSource::Memory(zip_with_entries(&[
            ("a-0.1/", b""),
            ("a-0.1/a.exe", b"binary-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "a.zip",
            "a",
            "a.exe",
            dest,
            false,
            Platform::new("windows", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "a.exe");
        assert_eq!(fs::read(dest.join("a.exe")).unwrap(), b"binary-bytes");
    }

    #[test]
    fn test_extract_zip_exe_entry_not_matched_off_windows() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[
            ("a-0.1/", b""),
            ("a-0.1/a.exe", b"binary-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "a.zip",
            "a",
            "a",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_zip_names_the_output_after_the_lookup_name() {
        use tempfile::TempDir;

        // The entry has no `.exe`, but a Windows target always gets one.
        let source = DownloadSource::Memory(zip_with_entries(&[
            ("a-0.1/", b""),
            ("a-0.1/a", b"binary-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "a.zip",
            "a",
            "a.exe",
            dest,
            false,
            Platform::new("windows", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "a.exe");
        assert!(dest.join("a.exe").exists());
    }

    /// A platform-suffixed entry is selected by the lookup name and written
    /// under it: `--bin-name app-bin` finds `app-bin-linux-x64`.
    #[test]
    fn test_extract_tar_gz_matches_platform_suffix() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_gz_with_entries(&[
            ("app-1.0/", b""),
            ("app-1.0/app-bin-linux-x64", b"linux-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.tar.gz",
            "app-bin",
            "app-bin",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin");
        assert_eq!(fs::read(dest.join("app-bin")).unwrap(), b"linux-bytes");
    }

    /// The same on a Windows target, where the entry carries `.exe`.
    #[test]
    fn test_extract_zip_matches_platform_suffix_on_windows() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[
            ("app-1.0/", b""),
            ("app-1.0/app-bin-windows-x64.exe", b"windows-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.zip",
            "app-bin",
            "app-bin.exe",
            dest,
            false,
            Platform::new("windows", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin.exe");
        assert_eq!(
            fs::read(dest.join("app-bin.exe")).unwrap(),
            b"windows-bytes"
        );
    }

    /// An exact match wins over a platform-suffixed one.
    #[test]
    fn test_exact_match_wins_over_platform_suffix() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_gz_with_entries(&[
            ("app-1.0/app-bin", b"exact-bytes"),
            ("app-1.0/app-bin-linux-x64", b"suffix-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.tar.gz",
            "app-bin",
            "app-bin",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin");
        assert_eq!(fs::read(dest.join("app-bin")).unwrap(), b"exact-bytes");
    }

    /// Among platform-suffixed candidates the best platform score wins.
    #[test]
    fn test_platform_suffix_picks_the_target_platform() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[
            ("app-1.0/app-bin-linux-x64", b"linux-bytes"),
            ("app-1.0/app-bin-windows-x64.exe", b"windows-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.zip",
            "app-bin",
            "app-bin",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin");
        assert_eq!(fs::read(dest.join("app-bin")).unwrap(), b"linux-bytes");
    }

    /// Two candidates with the same score are ambiguous: ask for a better name.
    #[test]
    fn test_platform_suffix_tie_is_an_error() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[
            ("app-1.0/app-bin-linux-x64", b"one"),
            ("app-1.0/app-bin-linux-x86_64", b"two"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "app.zip",
            "app-bin",
            "app-bin",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );

        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Multiple executables"),
            "error should report the ambiguity: {err}"
        );
        assert!(!dest.join("app-bin").exists());
    }

    /// A name that merely starts with the lookup name is not a candidate.
    #[test]
    fn test_platform_suffix_ignores_unrelated_names() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_gz_with_entries(&[("pi-1.0/pilot", b"junk")]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "pi.tar.gz",
            "pi",
            "pi",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );

        assert!(result.is_err());
        assert!(!dest.join("pi").exists());
    }

    /// A version tag after the lookup name still matches.
    #[test]
    fn test_platform_suffix_accepts_a_version_tag() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_gz_with_entries(&[(
            "app-1.0/app-bin-v1.2.3",
            b"tagged-bytes",
        )]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.tar.gz",
            "app-bin",
            "app-bin",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin");
        assert_eq!(fs::read(dest.join("app-bin")).unwrap(), b"tagged-bytes");
    }

    /// On a Windows target the `.exe` suffix is ignored while matching, so
    /// `--bin-name app-bin.exe` finds `app-bin-windows-x64.exe`.
    #[test]
    fn test_platform_suffix_ignores_the_exe_suffix_on_windows() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[(
            "app-1.0/app-bin-windows-x64.exe",
            b"windows-bytes",
        )]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.zip",
            "app-bin.exe",
            "app-bin.exe",
            dest,
            false,
            Platform::new("windows", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app-bin.exe");
        assert_eq!(
            fs::read(dest.join("app-bin.exe")).unwrap(),
            b"windows-bytes"
        );
    }

    /// The default lookup (the repository basename) uses the same rule.
    #[test]
    fn test_default_lookup_matches_platform_suffix() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_gz_with_entries(&[(
            "app-1.0/app-bin-linux-x64",
            b"linux-bytes",
        )]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.tar.gz",
            "app",
            "app",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app");
        assert_eq!(fs::read(dest.join("app")).unwrap(), b"linux-bytes");
    }

    /// A lone candidate naming another OS still installs — the archive was
    /// platform-matched at the asset level, so the entry is probably right —
    /// but a warning is printed to stderr.
    #[test]
    fn test_platform_suffix_installs_a_lone_foreign_os_candidate() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[(
            "app-1.0/app-windows-x64.exe",
            b"windows-bytes",
        )]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let written = extract_and_save(
            source,
            "app.zip",
            "app",
            "app",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        )
        .unwrap();

        assert_eq!(written, "app");
        assert_eq!(fs::read(dest.join("app")).unwrap(), b"windows-bytes");
    }

    #[test]
    fn test_extract_tar_gz_skips_name_suffix_collision() {
        use tempfile::TempDir;

        // Regression: opensec-git/Pi-Bolt ships `pi-bolt-linux-x64/LICENSE.pi`
        // *before* `pi-bolt-linux-x64/pi`; a raw `ends_with("pi")` matched the
        // license file and installed it as the binary.
        let source = DownloadSource::Memory(tar_gz_with_entries(&[
            ("pi-bolt-linux-x64/", b""),
            ("pi-bolt-linux-x64/LICENSE.pi", b"MIT License"),
            ("pi-bolt-linux-x64/pi", b"#!/bin/sh\necho pi\n"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "pi-bolt.tar.gz",
            "pi",
            "pi-bolt",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(
            result.is_ok(),
            "extract_and_save failed: {:?}",
            result.as_ref().err()
        );
        assert_eq!(
            fs::read(dest.join("pi-bolt")).unwrap(),
            b"#!/bin/sh\necho pi\n"
        );
    }

    #[test]
    fn test_extract_tar_xz_skips_name_suffix_collision() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(tar_xz_with_entries(&[
            ("pi-bolt-linux-x64/", b""),
            ("pi-bolt-linux-x64/LICENSE.pi", b"MIT License"),
            ("pi-bolt-linux-x64/pi", b"binary-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "pi-bolt.tar.xz",
            "pi",
            "pi-bolt",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(
            result.is_ok(),
            "extract_and_save failed: {:?}",
            result.as_ref().err()
        );
        assert_eq!(fs::read(dest.join("pi-bolt")).unwrap(), b"binary-bytes");
    }

    #[test]
    fn test_extract_zip_skips_name_suffix_collision() {
        use tempfile::TempDir;

        let source = DownloadSource::Memory(zip_with_entries(&[
            ("app-0.1/", b""),
            ("app-0.1/LICENSE.pi", b"MIT License"),
            ("app-0.1/pi", b"binary-bytes"),
        ]));

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let result = extract_and_save(
            source,
            "app.zip",
            "pi",
            "pi-bolt",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(
            result.is_ok(),
            "extract_and_save failed: {:?}",
            result.as_ref().err()
        );
        assert_eq!(fs::read(dest.join("pi-bolt")).unwrap(), b"binary-bytes");
    }
    #[test]
    fn test_extract_and_save_tar_gz_writes_out_name() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        // Archive holds "my-app"; we look it up by that name but write "renamed".
        let source = DownloadSource::Memory(tar_gz_with("my-app", b"hello"));

        let result = extract_and_save(
            source,
            "app.tar.gz",
            "my-app",
            "renamed",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(
            result.is_ok(),
            "extract_and_save failed: {:?}",
            result.as_ref().err()
        );

        let renamed = dest.join("renamed");
        assert!(
            renamed.exists(),
            "output should land under the rename value"
        );
        assert_eq!(fs::read(&renamed).unwrap(), b"hello");
        assert!(
            !dest.join("my-app").exists(),
            "the lookup name must not also be written to disk"
        );
    }

    #[test]
    fn test_extract_and_save_tar_gz_rename_missing_lookup_is_error() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let source = DownloadSource::Memory(tar_gz_with("my-app", b"hello"));

        // --rename must not be used as a fallback lookup key.
        let result = extract_and_save(
            source,
            "app.tar.gz",
            "other",
            "renamed",
            dest,
            false,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("other"),
            "error should name the lookup name that was tried: {err}"
        );
        assert!(!dest.join("renamed").exists());
    }

    #[test]
    fn test_extract_and_save_no_decompress_uses_out_name() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(b"payload".to_vec());

        let result = extract_and_save(
            source,
            "asset.tar.gz",
            "app",
            "renamed",
            dest_dir,
            true,
            Platform::new("linux", "x86_64"),
        );
        assert!(result.is_ok());

        assert!(dest_dir.join("renamed").exists());
        assert!(
            !dest_dir.join("asset.tar.gz").exists(),
            "the asset filename must be replaced by the rename value"
        );
        assert_eq!(fs::read(dest_dir.join("renamed")).unwrap(), b"payload");
    }
}
