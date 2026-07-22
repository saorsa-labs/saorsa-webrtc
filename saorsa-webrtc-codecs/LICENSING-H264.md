# H.264 licensing — read before enabling the `h264` feature in a product

**Status:** factual summary for engineering decisions, verified against the
dependency source on 2026-07-22. **Not legal advice.**

## How our dependency actually builds

The `h264` feature pulls `openh264` 0.7.x → `openh264-sys2` 0.7.x. As used
here (the crate's `source` strategy, its default):

- `openh264-sys2` **vendors Cisco's OpenH264 C++ source** inside the crate
  package (`upstream/` directory) and **compiles it locally at build time**.
- **Nothing is downloaded from Cisco** — we verified `build.rs` performs no
  network fetch in the `source` path.

## Why that distinction matters

Cisco's OpenH264 has two separate legal layers:

1. **Copyright:** BSD-2-Clause. Source builds are fine; retain the notice.
   This layer is unconditionally satisfied.
2. **Patents (AVC/H.264):** Cisco pays the AVC patent-pool royalties **only
   for the official binary modules downloaded from Cisco's servers**
   (their published terms are explicit that the grant covers their
   binaries, not recompiled source). A **source-built** OpenH264 — which is
   what the `source` strategy produces — carries **no** patent-royalty
   coverage from Cisco; any AVC patent exposure sits with the distributor
   of the product that ships it.

Additional context, jurisdiction-dependent: many foundational AVC/baseline
patents have expired over the last few years, and the remaining pool
coverage thins through the late 2020s — but "many" and "thins" are not
"all" and "gone", and enforcement posture varies by country.

## The alternative the crate offers

`openh264`/`openh264-sys2` also expose a **`libloading`** feature: the
application downloads Cisco's **official prebuilt binary** at
install/first-run time (the Firefox model) and the crate loads it at
runtime (`OpenH264API` from a blob). That path **does** ride Cisco's patent
grant, at the cost of a runtime download step and shipping Cisco's binary
license text alongside.

## What this means for tic-tac-toe (and any shipped app)

- `h264` is **off by default** in this crate — building/testing it in-repo
  is a development activity, not distribution.
- Before shipping video **enabled** in a product, pick one deliberately:
  1. **Runtime Cisco binary** (`libloading` route): patent grant applies;
     requires the download flow + Cisco's binary notice in the app. This is
     the shipping-safe default choice.
  2. **Source build**: get an explicit sign-off (counsel) that the AVC
     exposure is acceptable for the product's jurisdictions/timeframe.
  3. Longer term: prefer a royalty-free codec (AV1) as the default video
     codec and keep H.264 for interop only.
