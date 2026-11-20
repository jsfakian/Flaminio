# Issue #61: Instance-store NVMe — intent

[Issue #61](https://github.com/jsfakian/Flaminio/issues/61) · [spec](spec.md) · [design](design.md).

This remains the originally approved first half of step1: store identity before cloud recovery.
The user approved moving that shared root cause to a dedicated issue on2026-11-13. It now lives
in [#90](https://github.com/jsfakian/Flaminio/issues/90), whose `docs/issues/90/` plan is the sole
technical authority. This #61 folder is the composition and acceptance wrapper for that same
slice. It adds no classification, claim, placement or other platform implementation stage.

## Why

An instance-store disk can disappear while its node name survives. The identity slice prevents
an empty replacement from inheriting the old store's earned replication authority; #90 owns
the precise rules, contracts, model and adapter checks. This root cause also exists without a
cloud and is needed by #45's later store-loss recovery. Keeping it once avoids divergent rules.

The original technical draft and historical approvals remain available at
`issue/61-instance-store-nvme@5696573a126c0abe26347d3c9700e557489613cf:docs/issues/61/`.
The real predecessor R1 findings follow the moved authority into #90; the move alone resolves none.

## What we want

1. Keep the original disk-identity outcomes delegated to #90: observed versus earned identity,
   exact replica authority, full replacement, retained fences, honest unavailability, format
   identity and diagnostics. No second #61 contract or implementation duplicates them.
2. Accept this first half of step1 only when #90's implemented checks and adapter evidence meet
   the original outcomes, including live/detached replacement and independently verified data.
   A review-clear plan is neither an implementation nor a cloud test result.
3. Keep the tracker dependencies explicit: later #45 store-loss recovery and #61's remaining
   instance-store work consume #90. Existing review-clear dependency plans stay unchanged.
4. Preserve the original boundaries and keep #61 open after this slice.

## Out of scope

The rest of step1 (automatic rebuilding where ordinary resync cannot proceed), cloud device
classification/automatic claiming, surviving-copy placement/defaults, new node names/autoscaling,
multiple disks, below-minimum/force recovery, same-UUID rollback/cloning and real cloud evidence
remain the already-described later work. This wrapper adds no runtime behavior or lab action.
