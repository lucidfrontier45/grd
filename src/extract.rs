use std::{
    fs::{self, File},
    io::{self, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use tempfile::NamedTempFile;
use zip::ZipArchive;

use crate::download::DownloadSource;

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

/// Extract a downloaded asset and write it to `dest_dir`.
///
/// `lookup_name` selects which archive entry is extracted; `out_name` is the
/// filename written to disk. They differ when the user asked for a rename, and
/// `out_name` arrives already `.exe`-normalized for the target platform.
pub fn extract_and_save(
    source: DownloadSource,
    filename: &str,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
    no_decompress: bool,
) -> Result<()> {
    fs::create_dir_all(dest_dir).context("Failed to create destination directory")?;

    if no_decompress {
        save_raw(source, out_name, dest_dir)?;
        println!("Saved raw asset to {:?}", dest_dir.join(out_name));
        return Ok(());
    }

    if filename.ends_with(".zip") {
        extract_zip(source, lookup_name, out_name, dest_dir)
    } else if filename.ends_with(".tar.xz") {
        extract_tar_xz(source, lookup_name, out_name, dest_dir)
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(source, lookup_name, out_name, dest_dir)
    } else {
        save_raw(source, out_name, dest_dir)
    }
}

fn extract_zip(
    source: DownloadSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    let rdr: Box<dyn ReadSeek> = match source {
        DownloadSource::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let mut archive = ZipArchive::new(rdr).context("Failed to parse ZIP archive")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;
        if file.is_dir() {
            continue;
        }
        if is_lookup_match(file.name(), lookup_name) {
            let out_path = dest_dir.join(out_name);
            let mut outfile = File::create(&out_path).context("Failed to create output file")?;
            io::copy(&mut file, &mut outfile).context("Failed to write extracted file")?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    bail!("Executable '{}' not found in archive", lookup_name)
}

fn extract_tar_gz(
    source: DownloadSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    let rdr: Box<dyn Read> = match source {
        DownloadSource::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let mut archive = tar::Archive::new(GzDecoder::new(rdr));
    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut file = entry.context("Failed to read tar entry")?;
        let path = file.path()?.to_path_buf();
        if file.header().entry_type().is_dir() {
            continue;
        }
        if is_lookup_match(&path.to_string_lossy(), lookup_name) {
            let out_path = dest_dir.join(out_name);
            file.unpack(&out_path)
                .context("Failed to unpack tar entry")?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    bail!("Executable '{}' not found in archive", lookup_name)
}

fn extract_tar_xz(
    source: DownloadSource,
    lookup_name: &str,
    out_name: &str,
    dest_dir: &Path,
) -> Result<()> {
    // Stream-decompress xz into a temp file rather than buffering the entire
    // decompressed archive in memory, so the user's --memory-limit setting is
    // respected for .tar.xz payloads.
    let mut compressed: Box<dyn io::BufRead> = match source {
        DownloadSource::Memory(bytes) => Box::new(io::BufReader::new(io::Cursor::new(bytes))),
        DownloadSource::Disk(temp_file) => {
            Box::new(io::BufReader::new(File::open(temp_file.path())?))
        }
    };

    let mut decompressed =
        NamedTempFile::new().context("Failed to create temp file for xz decompression")?;
    lzma_rs::xz_decompress(&mut compressed, decompressed.as_file_mut())
        .context("Failed to decompress xz archive")?;
    drop(compressed);

    let tar_file = decompressed
        .reopen()
        .context("Failed to reopen decompressed tar for reading")?;
    let mut archive = tar::Archive::new(tar_file);
    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut file = entry.context("Failed to read tar entry")?;
        let path = file.path()?.to_path_buf();
        if file.header().entry_type().is_dir() {
            continue;
        }
        if is_lookup_match(&path.to_string_lossy(), lookup_name) {
            let out_path = dest_dir.join(out_name);
            file.unpack(&out_path)
                .context("Failed to unpack tar entry")?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    bail!("Executable '{}' not found in archive", lookup_name)
}

fn save_raw(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let out_path = dest_dir.join(target_bin_name);
    match source {
        DownloadSource::Memory(bytes) => {
            fs::write(&out_path, bytes).context("Failed to write file")?;
        }
        DownloadSource::Disk(temp_file) => {
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

        let result = save_raw(source, "test.bin", dest_dir);
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

        let result = save_raw(source, "test.bin", dest_dir);
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

        let result = extract_tar_xz(source, "my-app", "my-app", dest);
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

        let result = extract_tar_xz(source, "my-app", "my-app", dest);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_tar_xz_corrupted() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        let corrupted_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00];
        let source = DownloadSource::Memory(corrupted_data);

        let result = extract_tar_xz(source, "my-app", "my-app", dest);
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

        let result = extract_and_save(source, "foo.tar.xz", "app", "foo.tar.xz", dest, true);
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

        let result = extract_and_save(source, "test.bin", "app", "test.bin", dest_dir, true);
        assert!(result.is_ok());

        let file_path = dest_dir.join("test.bin");
        assert!(file_path.exists());
    }

    #[test]
    #[cfg(windows)]
    fn test_target_bin_name_windows() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![]);

        // out_name arrives already .exe-normalized from main.rs
        let result = extract_and_save(source, "test.bin", "app", "app.exe", dest_dir, false);
        assert!(result.is_ok());

        let file_path = dest_dir.join("app.exe");
        assert!(file_path.exists());
    }

    #[test]
    #[cfg(not(windows))]
    fn test_target_bin_name_unix() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![]);

        let result = extract_and_save(source, "test.bin", "app", "app", dest_dir, false);
        assert!(result.is_ok());

        let file_path = dest_dir.join("app");
        assert!(file_path.exists());
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
        let result = extract_and_save(source, "pi-bolt.tar.gz", "pi", "pi-bolt", dest, false);
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
        let result = extract_and_save(source, "pi-bolt.tar.xz", "pi", "pi-bolt", dest, false);
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
        let result = extract_and_save(source, "app.zip", "pi", "pi-bolt", dest, false);
        assert!(
            result.is_ok(),
            "extract_and_save failed: {:?}",
            result.as_ref().err()
        );
        assert_eq!(fs::read(dest.join("pi-bolt")).unwrap(), b"binary-bytes");
    }
    #[test]
    fn test_extract_and_save_tar_gz_renames_output() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path();
        // Archive holds "my-app"; we look it up by that name but write "renamed".
        let source = DownloadSource::Memory(tar_gz_with("my-app", b"hello"));

        let result = extract_and_save(source, "app.tar.gz", "my-app", "renamed", dest, false);
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
        let result = extract_and_save(source, "app.tar.gz", "other", "renamed", dest, false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("other"),
            "error should name the lookup name that was tried: {err}"
        );
        assert!(!dest.join("renamed").exists());
    }

    #[test]
    fn test_extract_and_save_no_decompress_uses_rename_value() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(b"payload".to_vec());

        let result = extract_and_save(source, "asset.tar.gz", "app", "renamed", dest_dir, true);
        assert!(result.is_ok());

        assert!(dest_dir.join("renamed").exists());
        assert!(
            !dest_dir.join("asset.tar.gz").exists(),
            "the asset filename must be replaced by the rename value"
        );
        assert_eq!(fs::read(dest_dir.join("renamed")).unwrap(), b"payload");
    }
}
