# Plan: `--rename` CLI option for `grd`

## Goal

Add `grd <repo> --rename <new-name>` so the extracted binary can be installed under a different filename than the one found inside the release archive, e.g. `grd owner/exe1 --rename exe2` writes `exe2`, and `grd owner/prj --bin-name exe1 --rename exe2` extracts `exe1` from the archive and writes it as `exe2`.

## Background

Today `--bin-name` (`src/cli.rs:22-23`) is resolved once in `src/main.rs:188-191` (defaulting to the repo basename) and then does **two different jobs with one value**:

1. **Lookup key** — `src/extract.rs:54, 75, 111` match archive entries with `path.ends_with(target_bin_name)`.
2. **Output filename** — `src/extract.rs:29-33, 55, 76, 113, 124`.

Because the value is shared, `--bin-name` can only mean "the name that is *also* what you want on disk". `--rename` is the missing second name.

Two related facts that shape the change:

- `--no-decompress` (`src/extract.rs:23-27`) bypasses the lookup entirely and saves under the **asset** filename (`save_raw(source, filename, …)`), so `--bin-name` is silently ignored today.
- `remove` and `info` (`src/main.rs:23-80`) rebuild the filename from `repo.split('/')` and ignore `--bin-name` entirely — so they are already broken for any custom-named install. Since `--rename` makes custom names the norm, the installed filename gets recorded in `state.toml`.

## Approach

1. **`src/cli.rs`** — add the flag next to `bin_name`:

   ```rust
   /// Rename the installed executable (does not change which file is
   /// extracted from the archive)
   #[arg(long)]
   pub rename: Option<String>,
   ```

   No short alias (all of `-b`, `-d`, `-l`, `-m`, `-t`, `-y` are taken, and `-r` reads as "recursive"). Add parse tests: `--rename` yields `Some`, default is `None`, and it coexists with `--bin-name`.

2. **Resolve two names in `src/main.rs`**, replacing `src/main.rs:188-191`:

   ```rust
   let lookup_name = args.bin_name.clone()
       .unwrap_or_else(|| repo.split('/').next_back().unwrap_or("app").to_string());
   let out_name = args.rename.clone().unwrap_or_else(|| lookup_name.clone());
   ```

   Then add one small helper used by the dry-run branch (`:194-200`), the force/upgrade check (`:226-232`), and the final path computation, so the target path is computed once instead of being duplicated in three places:

   ```rust
   fn target_file_path(dest: &Path, asset: &str, out_name: &str, no_decompress: bool) -> PathBuf
   ```

   - `no_decompress` → `dest.join(out_name)` (asset name is replaced by the rename value, per your choice)
   - `cfg!(windows)` → strip a trailing `.exe` from `out_name` (case-insensitive), then append `.exe`
   - otherwise → `dest.join(out_name)`

3. **Validate `--rename` in `src/main.rs`** before any network work: reject empty values and any value containing `/`, `\`, or a path separator / `..`. `dest.join(rename)` with a separator would escape the destination directory, so this is a containment guard, not just tidiness. Error message names the offending flag.

4. **Split the extract signature** in `src/extract.rs:14-20` so lookup and output are independent:

   ```rust
   pub fn extract_and_save(
       source: DownloadSource,
       filename: &str,
       lookup_name: &str, // matched inside the archive
       out_name: &str,      // written to disk (already .exe-normalized)
       dest_dir: &Path,
       no_decompress: bool,
   ) -> Result<()>
   ```

   - `no_decompress` → `save_raw(source, out_name, dest_dir)` and print the new path.
   - Otherwise the Windows `.exe` normalization moves out of `extract.rs` (step 2 owns it) and the three extractors match on `lookup_name` while writing to `out_name`. `extract_zip`/`extract_tar_gz`/`extract_tar_xz` each gain the extra parameter and use `dest_dir.join(out_name)`.

   Six parameters — under clippy's `too_many_arguments` threshold.

5. **Record the installed filename in `src/state.rs`**:
   - Add `#[serde(default, skip_serializing_if = "Option::is_none")] pub binary: Option<String>` to `CachedRelease`, holding the final on-disk filename (platform-specific, e.g. `fd.exe` on Windows).
   - Add `CachedRelease::installed_filename(&self, repo: &str) -> String` → `self.binary.clone()` when present, otherwise the legacy repo-basename + `.exe` rule. This keeps old state files working.
   - Extend `set_cached` with a `binary: String` parameter.

6. **Fix `remove` and `info`** in `src/main.rs:23-80` to use `entry.installed_filename(repo)` instead of the inline `repo.split('/')` + `.exe` block (also de-duplicates that block, which appears three times).

7. **Call sites**: `src/main.rs:264` passes both names to `extract_and_save`; `src/main.rs:267-272` passes the resolved filename to `set_cached`. Final message becomes `Successfully installed '{out_name}' to {dest:?}`, and the dry-run line likewise reports `out_name`.

8. **Tests** (in-module, per `AGENTS.md` — no top-level `tests/`):
   - `src/cli.rs`: `--rename` default `None`, parses to `Some`, parses alongside `--bin-name`.
   - `src/extract.rs`: tar.gz round-trip where the archive contains `my-app` and `lookup_name = "my-app"` / `out_name = "renamed"` → `renamed` exists and `my-app` does not; plus a `no_decompress` + rename case asserting the file lands under the rename value.
   - `src/state.rs`: round-trip with `binary` set; a legacy TOML string without `binary` still deserializes with `binary == None` and resolves via the repo basename; `installed_filename` returns the stored value when present.
   - `src/tests.rs`: extend the integration shape already used by `test_integration_download_extract_save` (`:79-117`) with a renamed-extraction case, and a `remove` case built on the existing `with_state_path` helper (`:156`).

9. **Docs**: `README.md:291` — add `--rename` next to `--bin-name`, and correct the `--bin-name` line to say it selects the archive entry *and* names the output unless `--rename` is given. Optionally a short "Binary Name" section in the `CONTEXT.md` glossary, matching the existing entry style.

## Trade-offs

- **Look up by `--rename` as a fallback** (rejected): `grd owner/exe1 --rename exe2` would succeed even if the archive only holds `exe2`. More forgiving, but it makes the flag's meaning input-dependent and hides typos in either flag.
- **`--rename` replaces the lookup key too** (rejected): collapses back into today's single-name model and makes `--bin-name X --rename Y` self-contradictory, which is exactly the combination you asked to support.
- **Reject `--rename` with `--no-decompress`** (rejected): since `--no-decompress` already ignores `--bin-name`, honoring `--rename` there is the consistent rule and makes the output name predictable across both modes.
- **Erroring on a `.exe` suffix** (rejected): users paste names from `grd info` output; stripping first makes `--rename fd.exe` and `--rename fd` agree.
- **Not storing `binary` in state** (rejected): `remove`/`info` stay wrong for every custom-named install.
- **Signature growth on `extract_and_save`**: a small options struct would avoid it, but six scalar params read better at the three call sites than a struct built for one call.

## Open questions

- None blocking. Two assumptions to confirm at review time: (a) rejecting separator-containing `--rename` values is acceptable even though the chosen `--no-decompress` behavior otherwise changes what that flag saves; (b) `binary` stores the final on-disk filename, so Windows entries read `fd.exe` in `state.toml`.

## Next step

Implement steps 1-4 (flag + name resolution + extract split), then run `cargo check -q`, `cargo test -q`, `cargo clippy -q --fix --allow-dirty`, `cargo clippy -q -- -D warnings` per `AGENTS.md`.