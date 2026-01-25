# Project State: Unified QUIC Media Transport

## Current Position
- **Status**: DESIGNED
- **Milestone**: M1 - Dependency Upgrade & Foundation (0/3 phases)
- **Phase**: 1.1 - Dependency Audit & Upgrade

## Project Summary

Eliminating dual-transport architecture by routing all WebRTC media through ant-quic QUIC streams.

**Problems Solved:**
- Integration gap (signaling on ant-quic, media on separate ICE/UDP)
- Technical debt (duplicated NAT traversal)
- Missing functionality (QUIC-native media transport)
- Poor experience (STUN/TURN server requirements)

**Target Version:** v0.3.0

## Milestones Overview

| # | Milestone | Phases | Status |
|---|-----------|--------|--------|
| 1 | Dependency Upgrade & Foundation | 3 | ← CURRENT |
| 2 | Transport Unification | 3 | Pending |
| 3 | Call Manager Rewrite | 3 | Pending |
| 4 | Integration & Testing | 4 | Pending |

## Current Milestone: M1 - Dependency Upgrade & Foundation

| Phase | Name | Status |
|-------|------|--------|
| 1.1 | Dependency Audit & Upgrade | ← NEXT |
| 1.2 | Transport Adapter Layer | Pending |
| 1.3 | Feature Flag Infrastructure | Pending |

## Key Files to Modify

1. **call.rs** - Remove RTCPeerConnection, replace with QUIC streams
2. **transport.rs** - Migrate to ant-quic 0.20 API
3. **quic_bridge.rs** - Add stream type tagging
4. **media.rs** - Remove TrackLocalStaticSample dependency
5. **Cargo.toml** - Update dependencies, remove webrtc crates

## Next Action

Run `/gsd-plan-phase` to detail Phase 1.1 into executable tasks.
