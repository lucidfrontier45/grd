## 1. Core Data Structures

- [x] 1.1 Define `PlatformAlias` struct with `alias`, `canonical_os`, and `inferred_arch` fields
- [x] 1.2 Create static `PLATFORM_ALIASES` HashMap containing Windows platform mappings
- [x] 1.3 Add `to_lowercase()` helper for case normalization

## 2. Platform Alias Normalization

- [x] 2.1 Implement `normalize_platform_identifier()` function
- [x] 2.2 Add alias lookup logic with case-insensitive matching
- [x] 2.3 Return canonical OS name and optional inferred architecture
- [x] 2.4 Handle unknown platform identifiers (return as-is for backward compatibility)

## 3. Architecture Inference

- [x] 3.1 Implement `infer_architecture_from_platform()` function
- [x] 3.2 Add logic to infer `x86_64` for `win32` and `win64` identifiers
- [x] 3.3 Return `None` for generic `win` and `windows` identifiers

## 4. Integration with Asset Matching

- [x] 4.1 Update asset matching logic to use `normalize_platform_identifier()`
- [x] 4.2 Ensure architecture inference is applied when available
- [x] 4.3 Maintain backward compatibility with existing exact-match behavior

## 5. Testing

- [x] 5.1 Add unit tests for `win`, `win32`, `win64` → `windows` normalization
- [x] 5.2 Add unit tests for case-insensitive matching (uppercase, lowercase, mixed case)
- [x] 5.3 Add unit tests for architecture inference from `win32` and `win64`
- [x] 5.4 Add unit tests for `win` and `windows` without architecture inference
- [x] 5.5 Add integration tests for asset matching with Windows platform variants
- [x] 5.6 Add edge case tests (unknown identifiers, empty strings)

## 6. Documentation

- [x] 6.1 Document platform alias mapping in code comments
- [x] 6.2 Add examples of supported Windows platform identifiers
