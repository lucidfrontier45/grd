## Context

`grd` currently extracts `.zip` (via `zip` crate), `.tar.gz`/`.tgz` (via `tar` + `flate2`), and falls through to `save_raw()` for everything else. `.tar.xz` assets from GitHub releases hit the fallback and get saved as compressed archives instead of usable binaries — they're effectively broken.

The existing extraction pattern is consistent:
```
DownloadSource (Memory | Disk) → decoder → tar::Archive → find binary → unpack
```

Xz fits as a new decoder layer between `DownloadSource` and `tar::Archive`, identical to how `GzDecoder` works today.

## Goals / Non-Goals

**Goals:**
- Decompress `.tar.xz` archives and extract the matching binary
- Follow the existing extraction pattern with minimal new code
- Preserve all existing extraction behavior unchanged

**Non-Goals:**
- Xz encoding or compression — download-only tool
- `.lzma` format (raw LZMA without xz framing) — not seen in GitHub releases
- Multi-stream xz archives — the `tar` layer handles a single binary per archive

## Decisions

### Crate: `lzma-rs` over `xz`

| Criterion | `xz` (liblzma bindings) | `lzma-rs` (pure Rust) |
|---|---|---|
| Build deps | C compiler, CMake (via `lzma-sys`) | None |
| Build time | +30-60s compiling C | Rust-only |
| Performance | Native speed | ~2-3x slower decompress |
| API style | `XzDecoder::new(reader)` — identical to `flate2::GzDecoder` | `xz_decompress(&mut reader, &mut writer)` — stream-oriented |

**Decision**: `lzma-rs`. This is a CLI downloader — the bottleneck is network I/O, not decompression. Avoiding C build dependencies makes `cargo install` more reliable. The read-to-write API decompresses the entire xz stream into a buffer before feeding it to `tar::Archive`, which is acceptable since the download is already bounded by `memory_limit`.

### Extraction dispatch order

Dispatch is checked sequentially by `filename.ends_with()`:

```
.tar.xz  → extract_tar_xz()     ← NEW
.tar.gz  → extract_tar_gz()     (existing)
.tgz     → extract_tar_gz()     (existing)
.zip     → extract_zip()        (existing)
else     → save_raw()           (existing)
```

## Risks / Trade-offs

- **[Performance] `lzma-rs` is slower than liblzma** — Not an issue for a network-bound CLI. Large `.tar.xz` files (100MB+) may show a few seconds of extra decompression time. If this becomes a problem, swapping to the `xz` crate is a local change in one function.
- **[Compatibility] Edge-case xz features** — `lzma-rs` may not handle every xz variant (padding, certain block sizes). GitHub release assets use standard settings. If a rare format fails, the error message will bubble up clearly. The `xz` crate can serve as a fallback if needed.
- **[Dispatch priority] `.tar.xz` before `.tar.gz`** — Not required since the suffixes don't overlap, but keeping `.tar.xz` first in the chain maintains a logical compression-ratio ordering (best compression checked first).
