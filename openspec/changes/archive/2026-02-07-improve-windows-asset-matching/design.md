## Context

Current asset matching logic in the codebase has limited support for Windows platform identifier variants. Assets are tagged with various Windows-specific identifiers (`win`, `win32`, `win64`) but the system only recognizes `windows` as the canonical OS name. This causes matching failures and incorrect platform detection for Windows assets. Additionally, the system lacks architecture inference capabilities for platform-specific identifiers like `win32` and `win64`.

The existing platform detection code uses string-based matching without normalization or alias support. Case sensitivity issues further compound the problem, requiring exact case matches that are often inconsistent in asset metadata.

## Goals / Non-Goals

**Goals:**
- Normalize platform identifier aliases to canonical OS names (e.g., `win`, `win32`, `win64` → `windows`)
- Infer architecture from platform-specific identifiers (`win32`, `win64` → `x86_64`)
- Ensure case-insensitive matching for all platform identifiers
- Maintain backward compatibility with existing asset tags
- Add comprehensive test coverage for Windows platform variants
- Prevent false positive matches from substring collisions (e.g., darwin assets matching Windows patterns)

**Non-Goals:**
- Platform alias support for non-Windows platforms (future scope)
- Dynamic architecture inference from arbitrary strings
- Breaking changes to existing API or data structures
- Cross-platform substring conflicts (addressed via explicit exclusion logic)

## Decisions

### 1. Alias Normalization Strategy

**Decision**: Implement a two-phase normalization approach:
1. **OS normalization**: Map platform aliases to canonical OS names
2. **Architecture inference**: Derive architecture from platform-specific identifiers

**Rationale**: Separating concerns allows for independent evolution of OS and architecture matching. This approach is extensible to other platforms (e.g., `linux`, `macos`) without code changes.

**Alternatives considered:**
- Single unified mapping table:Rejected due to tight coupling of OS and arch concerns
- Regex pattern matching:Rejected due to complexity and maintenance burden
- User-configurable aliases:Rejected as over-engineering for current needs

### 2. Case-Insensitive Matching

**Decision**: Convert all platform identifiers to lowercase before matching.

**Rationale**: Simple, deterministic, and follows Rust convention for case-insensitive comparisons. Avoids complex Unicode case folding requirements since platform identifiers are ASCII-only.

**Alternatives considered:**
- Case-folded comparisons:Rejected as unnecessary overhead for ASCII strings
- Normalize to uppercase:Rejected as lowercase is more common in Rust ecosystem

### 3. Architecture Inference Rules

**Decision**: Define explicit inference rules for Windows platform identifiers:
- `win32` → `OS=windows`, `Arch=x86_64`
- `win64` → `OS=windows`, `Arch=x86_64`
- `win` → `OS=windows` (no architecture inference)

**Rationale**: Both `win32` and `win64` historically refer to 32-bit and 64-bit Windows applications, but in modern contexts both run on x86_64 hardware. The `win` alias is generic and should not infer architecture.

**Alternatives considered:**
- `win32` → `x86`, `win64` → `x86_64`:Rejected due to WoW64 (32-on-64) complexity
- No architecture inference:Rejected as it loses valuable platform information

### 4. Data Structure

**Decision**: Use `HashMap<&'static str, (&'static str, Option<&'static str>)>` for alias mappings where key is the alias, and value is `(canonical_os, inferred_arch)`.

**Rationale**: Compile-time constant map with O(1) lookup performance. Static lifetime ensures no runtime allocation overhead.

**Alternatives considered:**
- `match` statements:Rejected for scalability (requires code changes per alias)
- Dynamic configuration:Rejected as over-engineering for stable set of aliases

## Risks / Trade-offs

**Risk**: Incorrect architecture inference for legacy `win32` binaries
→ **Mitigation**: Document that `win32` assets are assumed to be x86_64 in modern context. Users can explicitly specify architecture if needed.

**Risk**: Performance overhead from case conversion on every match
→ **Mitigation**: Case conversion is O(n) on small strings (typically < 10 chars). Negligible overhead compared to file I/O operations.

**Risk**: Breaking existing asset tags that rely on exact case matching
→ **Mitigation**: Change is additive - all existing exact matches continue to work. New aliases are additional match paths.

**Risk**: False positives from substring matching (e.g., "darwin" contains "win")
→ **Mitigation**: Add explicit exclusion logic to prevent darwin assets from matching Windows patterns. The Windows matching logic checks for darwin-specific substrings and excludes those assets.

**Trade-off**: Limited to Windows platform aliases in this change
→ **Justification**: Windows is the most problematic case based on issue reports. Other platforms can be added incrementally.

## Migration Plan

No migration required - this is a pure enhancement that maintains backward compatibility.

**Rollback strategy**: Remove alias normalization logic and revert to exact string matching.

## Open Questions

None - design is straightforward with clear technical decisions.
