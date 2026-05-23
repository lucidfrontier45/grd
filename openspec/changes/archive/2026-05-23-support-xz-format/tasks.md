## 1. Dependency Setup

- [x] 1.1 Add `lzma-rs` dependency to `Cargo.toml`

## 2. Extraction Implementation

- [x] 2.1 Add `extract_tar_xz()` function that decompresses xz into a buffer using `lzma_rs::xz_decompress`, then parses tar and extracts the matching binary
- [x] 2.2 Add `.tar.xz` branch to format dispatch in `extract_and_save()`

## 3. Tests

- [x] 3.1 Add test for `.tar.xz` extraction with a valid in-memory archive
- [x] 3.2 Add test for `--no-decompress` with `.tar.xz` asset saving raw bytes

## 4. Verification

- [x] 4.1 Run `cargo check` to ensure compilation
- [x] 4.2 Run `cargo test` to pass all tests
- [x] 4.3 Run `cargo clippy` to confirm no warnings
