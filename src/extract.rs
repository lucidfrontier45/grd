use std::{
    fs::{self, File},
    io::{self, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use zip::ZipArchive;

use crate::download::DownloadSource;

pub fn extract_and_save(
    source: DownloadSource,
    filename: &str,
    bin_name: &str,
    dest_dir: &Path,
    no_decompress: bool,
) -> Result<()> {
    fs::create_dir_all(dest_dir).context("Failed to create destination directory")?;

    if no_decompress {
        save_raw(source, filename, dest_dir)?;
        println!("Saved raw asset to {:?}", dest_dir.join(filename));
        return Ok(());
    }

    let target_bin_name = if cfg!(windows) {
        format!("{}.exe", bin_name)
    } else {
        bin_name.to_string()
    };

    if filename.ends_with(".zip") {
        extract_zip(source, &target_bin_name, dest_dir)
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(source, &target_bin_name, dest_dir)
    } else {
        save_raw(source, &target_bin_name, dest_dir)
    }
}

fn extract_zip(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let rdr: Box<dyn ReadSeek> = match source {
        DownloadSource::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let mut archive = ZipArchive::new(rdr).context("Failed to parse ZIP archive")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;
        if file.name().ends_with(target_bin_name) {
            let out_path = dest_dir.join(target_bin_name);
            let mut outfile = File::create(&out_path).context("Failed to create output file")?;
            io::copy(&mut file, &mut outfile).context("Failed to write extracted file")?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    bail!("Executable '{}' not found in archive", target_bin_name)
}

fn extract_tar_gz(source: DownloadSource, target_bin_name: &str, dest_dir: &Path) -> Result<()> {
    let rdr: Box<dyn Read> = match source {
        DownloadSource::Memory(bytes) => Box::new(io::Cursor::new(bytes)),
        DownloadSource::Disk(temp_file) => Box::new(File::open(temp_file.path())?),
    };
    let mut archive = tar::Archive::new(GzDecoder::new(rdr));
    for entry in archive.entries().context("Failed to read tar archive")? {
        let mut file = entry.context("Failed to read tar entry")?;
        let path = file.path()?.to_path_buf();
        if path.to_string_lossy().ends_with(target_bin_name) {
            let out_path = dest_dir.join(target_bin_name);
            file.unpack(&out_path)
                .context("Failed to unpack tar entry")?;
            #[cfg(unix)]
            set_permissions(&out_path)?;
            return Ok(());
        }
    }
    bail!("Executable '{}' not found in archive", target_bin_name)
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
    fn test_extract_and_save_no_decompress() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let dest_dir = temp_dir.path();
        let source = DownloadSource::Memory(vec![1, 2, 3, 4, 5]);

        let result = extract_and_save(source, "test.bin", "app", dest_dir, true);
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

        let result = extract_and_save(source, "test.bin", "app", dest_dir, false);
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

        let result = extract_and_save(source, "test.bin", "app", dest_dir, false);
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
}
