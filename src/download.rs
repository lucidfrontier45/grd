use std::io::{self, Read, Write};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use tempfile::NamedTempFile;
use ureq::Agent;

use crate::github::Asset;

pub enum DownloadSource {
    Memory(Vec<u8>),
    Disk(NamedTempFile),
}

pub fn download_asset(
    agent: &Agent,
    asset: &Asset,
    memory_threshold: u64,
) -> Result<DownloadSource> {
    println!("Downloading...");
    let pb = ProgressBar::new(asset.size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})",
        )
        .context("Failed to create progress style")?
        .progress_chars("#>–"),
    );
    let mut response = agent
        .get(&asset.browser_download_url)
        .call()
        .context("Failed to download asset")?;
    let mut reader = response.body_mut().as_reader();
    let source = if asset.size > memory_threshold {
        println!("Using temp file due to size > {} bytes", memory_threshold);
        let mut temp_file = NamedTempFile::new().context("Failed to create temp file")?;
        let writer = |buf: &[u8]| temp_file.write_all(buf);
        download_with_progress(&mut reader, &pb, writer)?;
        DownloadSource::Disk(temp_file)
    } else {
        let mut bytes = Vec::new();
        let writer = |buf: &[u8]| {
            bytes.extend_from_slice(buf);
            Ok(())
        };
        download_with_progress(&mut reader, &pb, writer)?;
        DownloadSource::Memory(bytes)
    };
    pb.finish_with_message("Downloaded");
    Ok(source)
}

fn download_with_progress<R: Read, F>(reader: &mut R, pb: &ProgressBar, mut writer: F) -> Result<()>
where
    F: FnMut(&[u8]) -> io::Result<()>,
{
    let mut buf = [0; 8192];
    loop {
        let n = reader
            .read(&mut buf)
            .context("Failed to read from download stream")?;
        if n == 0 {
            break;
        }
        writer(&buf[..n]).context("Failed to write to destination")?;
        pb.inc(n as u64);
    }
    Ok(())
}
