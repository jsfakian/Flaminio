# Issue #4: run every fuzz target for 10 minutes clean — intent

[Issue #4](https://github.com/jsfakian/Flaminio/issues/4), follow-ups §6.

## Why

Five fuzz targets cover the code that parses untrusted or crash-damaged bytes: the superblock,
the metadata pages and the journal (what recovery reads after a power loss), replication frames
and capability tokens (what a node accepts from the network). A panic in any of them is a crash
of the data plane, which a peer or a damaged disk could trigger at will.

The targets have only had a 5-second smoke run. The weekly Fuzz workflow now runs each for 10
minutes, but nothing turns what it finds into something that stays fixed: a crash is an
artifact on a workflow run, and the input that caused it is lost once the artifact expires.

## What we want

1. Every target runs for 10 minutes without a crash.
2. Every input that ever makes a target fail (a crash, a hang, too much memory) is kept and
   replayed: on every pull request for what a plain test can catch, and by every fuzz run for
   the rest, so the bug cannot come back unnoticed.
3. A failure found by the scheduled run cannot go unnoticed or be lost: someone is told, and
   the input survives the workflow artifact's expiry.

## Out of scope

- New fuzz targets (for example the metadata service's gRPC inputs, or the store's recovery as
  a whole). They can follow as their own issues.
- Fuzzing in CI on every pull request: 50 minutes of fuzzing does not fit there, and the weekly
  run covers it.
