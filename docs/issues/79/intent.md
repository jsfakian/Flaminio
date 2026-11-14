# Issue #79: CSI mount retries accept the wrong source or incompatible read-only state — intent

[Issue #79](https://github.com/jsfakian/Flaminio/issues/79). Approved 2026-11-13; not implemented.
Related work: #70 (expansion/XFS), #74 (privilege separation).

## Why

- **Mount existence is treated as compatibility.** `control/csi/node.go:98–102` accepts any
  staging mount, after possibly attaching another device. `node.go:169–172` accepts any target
  mount before checking source, access mode or readonly. The audit's two fake-mount reproductions
  accept `/some/other/volume` and a writable mount retried with `Readonly=true`
  (`docs/audits/2026-11-12/repro/csi_test.go.txt` in the audit checkout).
- **Cleanup has no ownership proof.** `node.go:130–156` unmounts a caller-supplied staging path
  then detaches; `node.go:206–217` unmounts and removes a caller-supplied target. Neither checks
  which volume owns the path, or whether other publishes still depend on staging.
- **The interface cannot prove a retry safe.** `control/csi/mount.go:17–24` exposes only a
  boolean mount query. Its mountinfo reader (`mount.go:88–103`) discards identity and flags.
  Filesystem publish assumes the supplied staging directory is right; raw block checks only
  the attached volume name. Node state is currently process-local (`node.go:18–29`).
- **Privileged inputs need boundaries.** Paths are checked only for emptiness and may traverse
  symlinks; volume IDs reach control-socket lines by concatenation (`dataplane.go:107–126`).
  The node container has a writable, bidirectional kubelet host mount (`deploy/csi/driver.yaml:130`).
- **Readonly needs actual verification.** The current `bind,ro` command delegates semantics
  to util-linux. A successful command and mountpoint existence are not evidence that the
  requested access restrictions hold, especially for raw block devices.

## What we want

1. Exact compatible stage/publish retries succeed, including after plugin restart; wrong
   sources, volumes, capability or readonly states fail before destructive work.
2. Durable ownership and kernel mount/device evidence protect staging, every publication,
   unpublish and unstage. An unowned path is never formatted, unmounted or deleted.
3. Validate IDs, socket replies and permitted paths before side effects. Reject symlinks,
   traversal and unsafe mount options; protect the actual operation from path substitution.
4. Give first formatting positive, permanent cross-node authority, and reserve durable operation
   intent before attachment. This R2 safety amendment includes bounded metadata initialization
   state/API and its authorization/model; it does not change steady-state replication.
5. Prove filesystem and block behavior with fake lifecycle/restart tests and a disposable Linux
   mount test using the shipped toolchain. Do not claim raw-block readonly from a VFS flag alone.

## Out of scope

- Expansion and XFS support (#70), privilege separation/profiles (#74), multi-volume attachment,
  new steady-state replication rules and the replica-store format. Metadata changes are limited
  to the initialization authority specified in this issue; other schema work remains out of scope.
- Automatically adopting existing mounts whose ownership cannot be established, or remounting
  an incompatible live publication into compliance.
- Shared lab execution in this planning stage. This issue stays open until its chosen readonly
  behavior and restart/upgrade procedure are implemented and verified.
