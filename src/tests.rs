use std::env;

use tempfile::TempDir;

use crate::{
    asset::select_asset,
    config::{configure_agent, get_auth_token},
    download::download_asset,
    extract::extract_and_save,
    github::fetch_release_info,
};

#[test]
fn test_select_asset_from_real_repo() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = get_auth_token();
    let agent = configure_agent(&ua, token.as_deref());

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("15.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    let result = select_asset(&release.assets, os, arch, false, None);
    // In non-terminal environment, multiple matches should error
    match result {
        Ok(selected) => println!("Selected asset for {}-{}: {}", os, arch, selected.name),
        Err(e) => println!("No unique match for {}-{}: {}", os, arch, e),
    }
}

#[test]
fn test_integration_download_extract_save() {
    let ua = format!("lucidfrontier45/grd-{}", env!("CARGO_PKG_VERSION"));
    let token = get_auth_token();
    let agent = configure_agent(&ua, token.as_deref());

    let release = fetch_release_info(&agent, "BurntSushi/ripgrep", Some("15.1.0")).unwrap();
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    // Skip if multiple matches (non-terminal environment)
    let asset = match select_asset(&release.assets, os, arch, false, None) {
        Ok(a) => a,
        Err(_) => {
            println!("Skipping test: multiple assets found for {}-{}", os, arch);
            return;
        }
    };

    let memory_limit = 10 * 1024 * 1024;

    dbg!(&asset);
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
