# Domain Glossary

## Platform

A combination of **Operating System** and **Architecture** that identifies a target environment.

### Operating System (OS)
The target OS for a binary asset. Supported values:

| Canonical | Aliases |
|-----------|---------|
| `linux`   | — |
| `macos`   | — |
| `windows` | `win`, `win32`, `win64` |

### Architecture (Arch)
The target CPU architecture for a binary asset. Supported values:

| Canonical | Aliases |
|-----------|---------|
| `x86_64`  | `amd64`, `x64` |
| `aarch64` | `arm64` |
| `loong64` | `loongarch64` |

### Default Architecture
When an asset filename contains an OS indicator but **no explicit architecture**, `grd` assumes one:

| OS      | Default Arch | Rationale |
|---------|-------------|-----------|
| linux   | `x86_64`    | Dominant in server/desktop Linux |
| macos   | `aarch64`   | Apple Silicon is now default for macOS releases |
| windows | `x86_64`    | Dominant in Windows ecosystems |

This avoids false negatives for projects that omit the arch from asset names (common when only one arch is shipped).

An asset that **does** contain an explicit arch pattern (e.g. `x86_64`, `aarch64`, `arm64`, `loong64`, `i386`, `armv7`) is always matched against its stated arch; the default-arch fallback only applies when no arch pattern is detected.

### Known Architecture Patterns
Release filenames commonly embed any of these substrings, which `has_explicit_arch_pattern` recognizes so that the default-arch fallback does not falsely match assets built for other architectures:

- `x86_64`, `amd64`, `x64`, `aarch64`, `arm64`, `loong64`, `loongarch64`
- `i686`, `i386`, `x86` (32-bit x86)
- `armhf`, `armv7` (32-bit ARM)
- bare `arm` token and `gnueabi`/`musleabi` triples (e.g. `arm-unknown-linux-gnueabihf`, `arm-unknown-linux-musleabihf` — the naming sharkdp/fd uses for 32-bit ARM)
- `riscv64`, `riscv32` (RISC-V)
- `ppc64le`, `ppc64`, `powerpc` (PowerPC)
- `s390x` (IBM Z)
- `mips64`, `mips` (MIPS)
- `win64`, `win32` (Windows-platform-derived)

## Asset
A downloadable file in a GitHub Release. Each asset has:
- `name` – filename (e.g. `app-linux-x86_64.tar.gz`)
- `browser_download_url` – download URL
- `size` – bytes

## Selection
The result of matching assets against a target OS + arch:

| Variant    | Meaning |
|------------|---------|
| `Exact`    | Exactly one match |
| `Multiple` | More than one match; user must select |
| `None`     | No match found |

## Version Cache
Persistent state at `~/.grd/state.toml` keyed by `"owner/repo"`, recording the last-downloaded version to skip re-downloads.

## Extension Filter
A default allowlist applied to asset names **before** OS/arch scoring, so non-binary artifacts (checksums, signatures, license files, manifests, package formats like `.dmg`/`.deb`/`.rpm`) never reach selection.

| Aspect | Behavior |
|--------|----------|
| Allowlist | `.exe`, `.zip`, `.tar.gz`, `.tgz`, `.tar.xz` (case-insensitive) |
| Always allowed | Files with no extension, and version-literal tails (e.g. `app-1.2.3`, `cli-rc1`) |
| Filter scope | Selection only — extraction pipeline still only knows the 5 allowlisted formats |
| Opt-out | `--no-ext-filter` (disables the filter; previous behavior restored) |

