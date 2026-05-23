use std::env;

use tempfile::TempDir;

use crate::{
    asset::{find_asset, Selection},
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

    match find_asset(&release.assets, os, arch, None) {
        Selection::Exact(asset) => println!("Selected asset for {}-{}: {}", os, arch, asset.name),
        Selection::Multiple(matches) => println!("{} matches for {}-{}", matches.len(), os, arch),
        Selection::None => println!("No match for {}-{}", os, arch),
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

    // Skip if multiple or no matches
    let asset = match find_asset(&release.assets, os, arch, None) {
        Selection::Exact(a) => a,
        _ => {
            println!("Skipping test: no unique match for {}-{}", os, arch);
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
