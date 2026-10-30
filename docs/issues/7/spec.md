# Issue #7: RegisterNode can lose labels when two registrations race — specification

## Behaviour

**S1. Atomic merge.** `RegisterNode` writes the merged record only if the stored record is still
the one it read: the same revision, or still absent if it read none. If another write came in
between, it reads the record again and merges again, after a short random backoff, as the
service's other updates do (`backoff`, up to `retries` attempts).

**S2. Contention.** After `retries` (32) failed attempts, it fails with `Aborted` ("too much
contention"), like the other updates.
- **Who retries:** the CSI node plugin's topology sync retries a failed registration every 5 s.
  The data plane sends its registration again every 3 s until one succeeds, with or without
  heartbeats, and again after a heartbeat answered `NotFound` (the service lost its record). Its
  heartbeats go out as before, whatever the registration's state, so a node whose record
  exists stays live while its registration is retried. The first heartbeat after a successful
  registration goes out at once. Before this, it
  registered once at start, so a metadata service that was down or contended then left the
  node unregistered, and never live, until a restart.
- **How rare it is:** an attempt fails only when another registration of the same node commits
  during it. A node has a handful of writers (the data plane, the CSI node plugin, an
  operator's `metactl`), far fewer than 32.

**S3. No address keeps the stored one.** A registration with an empty address keeps the stored
address, and merges its labels as above. For a node without a record (or without an address in
it), it fails with `NotFound`, as `NodeHeartbeat` does for an unregistered node: a node is first
registered by its data plane, with an address. The proto's comments on `RegisterNode` and
`Node.address` say so. The CSI node plugin's topology sync registers its labels this way, without an
address, and so can no longer put back an address it read before the data plane registered a
new one. It takes `NotFound` as "not registered yet" and retries every 5 s, as it did. A metadata
service older than this refuses the empty address with `InvalidArgument`. The sync then sends
its labels the old way, with the address it reads from `ListNodes`, so that a rolling upgrade
does not hold the labels back.

**S4. A registration that changes nothing writes nothing.** If the merged record has the stored
record's address and labels, `RegisterNode` succeeds without writing. So another registration in
flight does not have to retry for nothing.

**S5. Unchanged.**
- **Merge rules (intent, 2):** a request's label overrides a stored label of the same name.
  Stored labels the request does not name are kept, and the address is replaced.
- **Validation** of the id. (An empty address is no longer refused: S3.)
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
- **No address** (in-memory and etcd): labels without an address for an unregistered node get
  `NotFound`. After the node registers `n1:1` and then `n1:2`, labels without an
  address keep `n1:2`.
- **No change:** registering the same address and labels again, or no address and no labels,
  makes no `Txn` on the node's key.
- **Data plane** (`flaminio-node/tests/engine.rs`): with the first two registrations refused, the
  node is registered at a later heartbeat. After the service loses the record, a heartbeat's
  `NotFound` makes the node register again.
- **CSI topology sync** (`control/csi/topology_test.go`):
  - **In-process metadata service:** before the data plane registers, `registerTopology`
    reports the node as not registered. After each address the data plane registers, the
    labels are added and that address is kept.
  - **Recording fake:** the registration it sends carries no address. Against a fake that
    refuses an empty address with `InvalidArgument`, as an older service does, it sends again
    with the address from `ListNodes`. A refusal for another reason, with the node not listed,
    comes back as it is.
