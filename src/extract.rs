use std::{
    fs::{self, File},
    io::{self, Read, Seek},
    path::Path,
};

use anyhow::{Context, Result};
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
    Err(anyhow::anyhow!(
        "Executable '{}' not found in archive",
        target_bin_name
    ))
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
    Err(anyhow::anyhow!(
        "Executable '{}' not found in archive",
        target_bin_name
    ))
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
