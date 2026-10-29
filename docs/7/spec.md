# Issue #7: RegisterNode can lose labels when two registrations race — specification

## Behaviour

**S1. Atomic merge.** `RegisterNode` writes the merged record only if the stored record is still
the one it read: the same revision, or still absent if it read none. If another write came in
between, it reads the record again and merges again, after a short random backoff, as the
service's other updates do (`backoff`, up to `retries` attempts).

**S2. Contention.** After `retries` (32) failed attempts, it fails with `Aborted` ("too much
contention"), like the other updates.
- **Who retries:** the CSI node plugin's topology sync retries a failed registration every 5 s.
  The data plane registers once, when it starts, and does not retry.
- **Why the data plane never sees it in practice:** an attempt fails only when another
  registration of the same node commits during it. A node has a handful of writers (the data
  plane, the CSI node plugin, an operator's `metactl`), far fewer than 32.

**S3. Unchanged.**
- **Merge rules (intent, 2):** a request's label overrides a stored label of the same name.
  Stored labels the request does not name are kept, and the address is replaced.
- **Validation** of the request.
- **Unparseable record:** a stored record that does not parse counts as having no labels and is
  overwritten, as now. That is what lets the node register again, and `ListNodes` work again,
  after such a record.
- **Liveness:** the node's liveness stays in its own key.

## Tests

`meta/service/service_test.go`:
- **Interleaved:** a test store wraps the in-memory store. Once the first registration has read
  the node's key, the store lets a second registration, with other labels, complete before the
  first one writes. Afterwards the record holds both registrations' labels. Without S1, the first
  write drops the second's labels, and the test fails. It runs twice:
  - with no record yet, which tests the "still absent" compare;
  - with a record registered beforehand.
- **Concurrent** (in-memory and etcd, as the service's other tests run): twenty goroutines
  register a node that has no record yet, at once, each with its own label. Every registration
  succeeds, and every label is in the record at the end.
- **Overwrite:** register `zone=z1` and a second label, then register again with `zone=z2` and
  a new address. The record has `zone=z2`, the second label and the new address (in-memory and
  etcd).
- **Unparseable record:** write bytes that are not a record at the node's key, through the
  in-memory store. `RegisterNode` then succeeds, and `ListNodes` returns the node with the new
  address and labels.
