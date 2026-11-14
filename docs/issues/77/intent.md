# Issue #77: Consistent volume reads across replacement — intent

[GitHub issue](https://github.com/jsfakian/Flaminio/issues/77). This is a metadata snapshot
consistency fix, not evidence that acknowledged data was lost in the reproduction.

## Why

- `Service.readVolume` reads a header, then lists chunk keys independently
  (`meta/service/service.go:303`). Matching chunk count is its only normal consistency check.
- Header and prefix reads are individually linearizable (`meta/kv/kv.go`); that does not make
  their combination a snapshot. Chunk ISR transitions compare the header revision, and
  incarnation creation/deletion changes it, so it can validate the read boundary.
- The audit's `TestAuditReadVolumeDoesNotMixIncarnations` returns an old RF=2 header with new
  RF=1 chunk records after delete/confirm/recreate of the same name and chunk count.
- Attach commits a new header, calls readVolume, then overwrites its returned header with the
  earlier attached header (`service.go:695`). A correct ordinary read helper alone must not
  leave this second response-construction race intact.
- Existing tests cover deletion mid-call and epoch fencing, but not coherent same-count
  replacement or an attach racing its response construction.

## What we want

1. A successful volume response describes header and chunks that coexisted at a valid read
   point; different incarnations or attach epochs cannot be spliced together.
2. Concurrent changes cause bounded retries or a precise refusal, not false corruption reports.
3. Attach never signs a token paired with chunks read under a different header incarnation.
4. Keep etcd writes conditional and preserve first-epoch, tombstone and ISR semantics.

## Out of scope

- Creation resume (#76), placement admission (#78), expansion (#70), resource admission (#80),
  new metadata consistency models, or a promise that read responses remain current forever.
- Changing replication/fencing rules or treating a rejected/stale attach as a data-loss event.
