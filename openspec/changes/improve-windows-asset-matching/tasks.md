## 1. Core Data Structures

- [ ] 1.1 Define `PlatformAlias` struct with `alias`, `canonical_os`, and `inferred_arch` fields
- [ ] 1.2 Create static `PLATFORM_ALIASES` HashMap containing Windows platform mappings
- [ ] 1.3 Add `to_lowercase()` helper for case normalization

## 2. Platform Alias Normalization

- [ ] 2.1 Implement `normalize_platform_identifier()` function
- [ ] 2.2 Add alias lookup logic with case-insensitive matching
- [ ] 2.3 Return canonical OS name and optional inferred architecture
- [ ] 2.4 Handle unknown platform identifiers (return as-is for backward compatibility)

## 3. Architecture Inference

- [ ] 3.1 Implement `infer_architecture_from_platform()` function
- [ ] 3.2 Add logic to infer `x86_64` for `win32` and `win64` identifiers
- [ ] 3.3 Return `None` for generic `win` and `windows` identifiers

## 4. Integration with Asset Matching

- [ ] 4.1 Update asset matching logic to use `normalize_platform_identifier()`
- [ ] 4.2 Ensure architecture inference is applied when available
- [ ] 4.3 Maintain backward compatibility with existing exact-match behavior

## 5. Testing

- [ ] 5.1 Add unit tests for `win`, `win32`, `win64` → `windows` normalization
- [ ] 5.2 Add unit tests for case-insensitive matching (uppercase, lowercase, mixed case)
- [ ] 5.3 Add unit tests for architecture inference from `win32` and `win64`
- [ ] 5.4 Add unit tests for `win` and `windows` without architecture inference
- [ ] 5.5 Add integration tests for asset matching with Windows platform variants
- [ ] 5.6 Add edge case tests (unknown identifiers, empty strings)

## 6. Documentation

- [ ] 6.1 Document platform alias mapping in code comments
- [ ] 6.2 Add examples of supported Windows platform identifiers
