# 0001. Go for the control plane, Rust for the data plane

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

Flaminio has two very different halves. The data plane (I/O engine, replica store, volume
target, frontends, node agent) is performance-critical, long-running and security-sensitive,
because it parses network and on-disk input. The control plane (operator, CSI driver,
controllers, metadata service, CLI) mostly talks to Kubernetes and etcd.

## Decision

- **Rust** for everything in `dataplane/`: `flaminio-io`, the node agent and the privileged
  helper.
- **Go** for everything that faces Kubernetes: operator, CSI driver, controllers, metadata
  service, `kubectl-flaminio`.
- The two halves communicate only through gRPC/protobuf contracts defined in `api/`.

## Consequences

- Data plane: memory safety without GC pauses, direct access to `io_uring` and ublk, a small
  `unsafe` surface that can be audited.
- Control plane: first-class use of controller-runtime, the CSI libraries, client-go and the
  etcd client, which match what Kubernetes operators already know.
- Cost: two toolchains, two sets of dependency and security tooling (`cargo-deny`/`cargo-audit`
  and `govulncheck`). Contracts in `api/` must be versioned carefully.
