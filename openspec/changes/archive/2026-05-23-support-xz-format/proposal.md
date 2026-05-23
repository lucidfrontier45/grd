## Why

GitHub release assets are increasingly distributed as `.tar.xz` archives (e.g., Rust toolchain, Python builds, FFmpeg, Linux kernel tools). `grd` currently only handles `.zip`, `.tar.gz`, and `.tgz`, leaving `.tar.xz` assets unsupported — they fall through to `save_raw()` which saves the compressed archive as-is instead of extracting the binary.

## What Changes

- Add `.tar.xz` archive extraction (decompress xz → parse tar → extract binary)
- Add a new Rust dependency for xz decompression
- Update extraction dispatch in `extract_and_save()` to recognize `.tar.xz`
- Existing asset selection, caching, auth, and download logic is unaffected

## Capabilities

### New Capabilities
- `xz-extraction`: Support for downloading and extracting `.tar.xz` archives from GitHub releases

### Modified Capabilities
*(none — no existing spec requirements change)*

## Impact

- **`extract.rs`**: New function `extract_tar_xz()`; updated dispatch in `extract_and_save()`
- **`Cargo.toml`**: New dependency `lzma-rs` for xz decompression
- **Asset selection** (`asset.rs`): No changes needed — `.tar.xz` filenames are matched by OS/arch patterns already
- **Tests**: New unit tests for `.tar.xz` extraction
