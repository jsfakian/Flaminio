# Issue #7: RegisterNode can lose labels when two registrations race — design

## The change

`meta/service/service.go`, `RegisterNode`. The validation stays first. Then the merge runs in
the loop that `MoveReplica` and the other updates use:

```go
for attempt := range retries {
	if attempt > 0 {
		if err := backoff(ctx, attempt); err != nil {
			return nil, errStatus(err)
		}
	}
	e, ok, err := s.kv.Get(ctx, nodeKey(id))
	if err != nil {
		return nil, errStatus(err)
	}
	// ... merge e's labels (if ok and it parses) with the request's, as now: the request's
	// labels last, so they override stored ones of the same name ...
	rev := int64(0) // absent: the record must still be absent
	if ok {
		rev = e.Rev
	}
	applied, err := s.kv.Txn(ctx, []kv.Cmp{{Key: nodeKey(id), Rev: rev}}, []kv.Op{{Key: nodeKey(id), Value: b}})
	if err != nil {
		return nil, errStatus(err)
	}
	if applied {
		return &metav1.RegisterNodeResponse{}, nil
	}
}
return nil, status.Error(codes.Aborted, "too much contention")
```

- **The compare.** `kv.Cmp{Rev: 0}` means "the key must not exist". Both stores read an absent
  key as revision 0: `Mem` compares its stored revision, and etcd compares `ModRevision`, which is
  0 for a missing key. So the first registration of a node cannot overwrite one that another
  registration wrote meanwhile.
- **An unparseable record** is treated as having no labels, as now, and overwritten. The compare
  still holds it to the revision that was read. This differs on purpose from the loops it is
  modelled on, which return an error when a record does not parse. Here, failing would leave
  the node unable to register, and `ListNodes`, which fails on such a record, broken.
- **No address.** An empty address takes the stored record's. If there is none, the call returns
  `FailedPrecondition` from inside the loop, before any write. The validation no longer requires
  an address.
- **No change, no write.** If the merged JSON equals `e.Value`, it returns success without a
  `Txn`.
- **CSI topology sync** (`control/csi/topology.go`): `syncTopologyOnce` reads the Kubernetes
  labels, then calls `registerTopology`. That function sends `RegisterNode` with the labels and
  no address, and maps `FailedPrecondition` to `errNodeNotRegistered`. It no longer reads the
  address with `ListNodes`.
- **Unchanged.** `RegisterNode` is the only code that writes a node record, so no other path
  needs the loop.

## Tests

`meta/service/service_test.go`:
- **Interleaved** (in-memory store). `interleaveKV` wraps `kv.KV`, holding a hook in an
  `atomic.Pointer`. Its `Get` of the node's key swaps the hook out, so it runs at most once and
  the nested call does not run it again. It then calls the hook after the read and before
  returning. The hook runs a second `RegisterNode` to completion, through the same service.
  - **Arming:** the hook is armed only after any setup registration.
  - **Counting:** the wrapper counts the `Txn` calls on the node's key made outside the hook,
    which are the first registration's. With the fix there are 2: the first fails its compare,
    the second applies. That shows the race happened.
  - **Cases:** two subtests, with no record yet and with a record registered beforehand.
- **Concurrent** (`stores`: in-memory and etcd): twenty goroutines, one label each, the same
  address, on a node with no record yet. All succeed, and the record ends with all twenty
  labels.
- **Overwrite** (`stores`): `zone=z1` with `rack=r1`, then `zone=z2` with a new address. The
  record shows `zone=z2`, `rack=r1` and the new address.
- **Unparseable record** (in-memory): the test puts bytes that are not JSON at the node's key
  directly in the `kv.Mem`. `RegisterNode` succeeds, and `ListNodes` returns the node with the
  request's address and labels.

## Decisions

- **The same loop as the other updates.** It reuses `backoff` and `retries`, rather than an
  etcd-specific merge or a lock.
- **Overwrite an unparseable record, rather than fail.** See "The change".
- **No label removal.** It is out of scope (intent).
