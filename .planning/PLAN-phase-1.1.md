# Phase 1.1: Dependency Audit & Upgrade

## Overview
Upgrade ant-quic from 0.10.3 to 0.20.0 and resolve all API breaking changes.

## Tasks

### Task 1: Document webrtc crate usage patterns
- **Files**: Analysis only (no modifications)
- **Description**: Create a detailed inventory of all webrtc crate imports and usages across the codebase. Document which modules use RTCPeerConnection, ICE, DTLS, SRTP, etc.
- **Tests**: N/A (analysis task)
- **Status**: completed
- **Output**: `.planning/specs/webrtc-usage-audit.md`

### Task 2: Upgrade ant-quic in Cargo.toml
- **Files**: `saorsa-webrtc-core/Cargo.toml`
- **Description**: Update ant-quic from version 0.10.3 to 0.20.0. Also check and update saorsa-transport dependency if needed.
- **Tests**: `cargo check` must pass (may have errors we fix in next task)
- **Status**: completed (already at 0.20)

### Task 3: Fix ant-quic API changes in transport.rs
- **Files**: `saorsa-webrtc-core/src/transport.rs`
- **Description**: Update all ant-quic API calls to match the 0.20.0 API. The old API uses `QuicP2PNode`, `QuicNodeConfig`, `EndpointRole`, etc. Research the new API and update accordingly.
- **Tests**: `cargo check` must pass
- **Status**: completed (API already updated)

### Task 4: Update remaining modules for ant-quic 0.20 compatibility
- **Files**: Any other files using ant-quic directly
- **Description**: Search for any other direct ant-quic usage and update to new API.
- **Tests**: `cargo check` must pass
- **Status**: completed (all modules compatible)

### Task 5: Run cargo clippy and fix all warnings
- **Files**: All source files
- **Description**: Run `cargo clippy --all-features --all-targets -- -D warnings` and fix every warning.
- **Tests**: Zero clippy warnings
- **Status**: completed (zero warnings)

### Task 6: Run cargo test and ensure all tests pass
- **Files**: All test files
- **Description**: Run full test suite, fix any test failures caused by API changes.
- **Tests**: 100% test pass rate
- **Status**: completed (8 tests passing)

## Completion Criteria
- [x] ant-quic 0.20.0 in Cargo.toml
- [x] Zero compilation errors
- [x] Zero clippy warnings
- [x] 100% test pass rate
- [x] webrtc usage documented in `.planning/specs/webrtc-usage-audit.md`

## Phase 1.1 Complete - 2026-01-25
