# Phase 1.3: Feature Flag Infrastructure

## Overview
Implement feature flags to enable dual-mode operation (QUIC-native and legacy WebRTC), allowing gradual migration without breaking changes.

## Context from Phase 1.2
- LinkTransport abstraction layer created
- StreamType support fully integrated
- AntQuicTransport stream-aware
- Connection sharing infrastructure in place
- All tests passing (131+ tests, zero warnings)

## Tasks

### Task 1: Add feature flags to Cargo.toml
- **Files**: `saorsa-webrtc-core/Cargo.toml`
- **Description**: Define feature flags and conditional dependencies
- **Requirements**:
  - Add `[features]` section if not present
  - Define `quic-native` feature (default, no dependencies)
  - Define `legacy-webrtc` feature (gated WebRTC dependencies)
  - Set `quic-native` as default feature
  - Gate WebRTC dependencies on `legacy-webrtc` feature
- **Tests**: `cargo check` with different feature combinations
- **Status**: pending

### Task 2: Add conditional compilation for dual-mode
- **Files**: All relevant source files
- **Description**: Add `#[cfg(feature = "...")]` attributes for feature-gated code
- **Requirements**:
  - Guard legacy WebRTC code with `#[cfg(feature = "legacy-webrtc")]`
  - Ensure QUIC-native path works without legacy features
  - No dead code warnings when feature disabled
  - Document which code is feature-gated
- **Tests**: Compile with and without legacy-webrtc feature
- **Status**: pending

### Task 3: Test feature combinations
- **Files**: All test files
- **Description**: Verify both feature configurations work correctly
- **Requirements**:
  - Run tests with default features (quic-native)
  - Run tests with `legacy-webrtc` feature
  - Run tests with all features
  - Ensure no regressions in either configuration
- **Tests**: 100% pass rate for all feature combinations
- **Status**: pending

### Task 4: Update documentation
- **Files**: README.md, Cargo.toml doc comment
- **Description**: Document feature flags and usage
- **Requirements**:
  - Explain what each feature enables
  - Document migration path from legacy-webrtc to quic-native
  - Add examples for using different feature combinations
  - Document deprecation timeline for legacy-webrtc
- **Tests**: Documentation builds without warnings
- **Status**: pending

### Task 5: Run cargo clippy and fix all warnings
- **Files**: All source files
- **Description**: Run `cargo clippy --all-features --all-targets -- -D warnings`
- **Tests**: Zero clippy warnings with all features
- **Status**: pending

### Task 6: Run cargo test with all feature combinations
- **Files**: All test files
- **Description**: Run full test suite with different feature combinations
- **Tests**: 100% pass rate with all feature combinations
- **Status**: pending

## Completion Criteria
- [ ] Feature flags defined in Cargo.toml
- [ ] Conditional compilation implemented throughout codebase
- [ ] WebRTC dependencies properly gated
- [ ] Tests passing with quic-native (default)
- [ ] Tests passing with legacy-webrtc
- [ ] Tests passing with all features
- [ ] Zero clippy warnings with all features
- [ ] Documentation updated
- [ ] 100% test pass rate across all configurations

## Dependencies
- Requires Phase 1.2 completion (transport adapter layer)
- Prepares foundation for Phase 2 (transport unification)

## Expected Outcomes
After Phase 1.3:
1. Users can opt into legacy WebRTC support
2. Default build uses QUIC-native only
3. Clear migration path documented
4. Both code paths tested and validated
5. Ready for Phase 2 implementation
