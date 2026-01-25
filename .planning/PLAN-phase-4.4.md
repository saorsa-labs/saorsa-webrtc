# Phase 4.4: Release Preparation

**Objective**: Prepare the unified QUIC media transport for v0.3.0 release.

## Prerequisites
- Phase 4.3 complete (Documentation & Cleanup) ✅

## Tasks

### Task 1: Version Bump to 0.3.0
**Files to update:**
- `saorsa-webrtc-core/Cargo.toml` - version = "0.3.0"
- `saorsa-webrtc-codecs/Cargo.toml` - update if needed

**Requirements:**
- Bump version from 0.2.1 to 0.3.0
- Ensure workspace dependencies align
- Verify `cargo check` passes after version bump

### Task 2: Update CHANGELOG.md
**Create/Update:** `CHANGELOG.md` in project root

**Content:**
- v0.3.0 release notes
- Breaking changes summary
- New features (QUIC-native transport)
- Deprecated APIs (SDP/ICE methods)
- Migration instructions reference

### Task 3: Update README.md
**Update:** `README.md` in project root

**Content:**
- New architecture overview
- Updated installation instructions
- Feature flags documentation
- Quick start examples for QUIC-native calls
- Link to migration guide

### Task 4: Deprecation Notices
**Tasks:**
- Ensure all deprecated methods have proper #[deprecated] attributes
- Add deprecation timeline in documentation
- Note legacy-webrtc feature removal in v0.4.0

### Task 5: Final Pre-Release Validation
**Run all quality gates:**
- `cargo fmt --all -- --check` - formatting
- `cargo clippy --all-features --all-targets -- -D warnings` - linting
- `cargo test --all-features` - all tests pass
- `cargo doc --all-features --no-deps` - documentation builds
- `cargo publish --dry-run` - verify publishable

## Success Criteria
- [ ] Version bumped to 0.3.0
- [ ] CHANGELOG.md updated with release notes
- [ ] README.md reflects new architecture
- [ ] All deprecation notices in place
- [ ] All quality gates pass
- [ ] `cargo publish --dry-run` succeeds

## Estimated Changes
- Version updates: 2-3 files
- Documentation updates: ~200-400 lines
- No code changes expected
