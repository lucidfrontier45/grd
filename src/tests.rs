use std::env;

use tempfile::TempDir;
use ureq::Agent;

use crate::{
    asset::select_asset, download::download_asset, extract::extract_and_save,
    github::fetch_release_info,
};

#[test]
#[ignore = "Requires GitHub API access (may be rate limited)"]
fn test_select_asset_from_real_repo() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let agent = Agent::config_builder().user_agent(&ua).build().into();

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("14.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let result = select_asset(&release.assets, os, arch, true, None);
    assert!(result.is_ok());

    let selected = result.unwrap();
    println!("Selected asset for {}-{}: {}", os, arch, selected.name);
}

#[test]
#[ignore = "Requires GitHub API access (may be rate limited)"]
fn test_integration_download_extract_save() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let agent = Agent::config_builder().user_agent(&ua).build().into();

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("14.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let asset = select_asset(&release.assets, os, arch, true, None).unwrap();
    let memory_limit = 10 * 1024 * 1024;

    let source = download_asset(&agent, &asset, memory_limit).unwrap();

    let temp_dir = TempDir::new().unwrap();
    let dest_dir = temp_dir.path();
    let bin_name = "rg";

    let result = extract_and_save(source, &asset.name, bin_name, dest_dir, false);
    assert!(result.is_ok());

    let expected_name = if cfg!(windows) {
        format!("{}.exe", bin_name)
    } else {
        bin_name.to_string()
    };

    let extracted_path = dest_dir.join(&expected_name);
    assert!(extracted_path.exists());
}
