# 0003. Linux 6.6 minimum kernel

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

The data plane depends on `io_uring` and ublk (including `UBLK_F_USER_RECOVERY`, which keeps
devices alive through data-plane restarts) and on kTLS for TLS 1.3. Older kernels lack these
features or ship them in a less mature form. 6.6 is an LTS kernel.

## Decision

The minimum supported kernel is **6.6**, with `ublk_drv` available (built in or as a module).
Newer features (e.g. ublk zero-copy, NVMe/TCP TLS for the export feature) are detected at
runtime and used when present.

## Consequences

- Meets the baseline: Ubuntu 24.04 (6.8), Ubuntu 22.04 with the HWE kernel, RHEL/Rocky/Alma 10
  (6.12), Debian 13 (6.12).
- **Excluded:** the RHEL 9 family (5.14) and Debian 12 (6.1). This must be stated clearly in the
  docs and enforced by `kubectl flaminio doctor` and by the node agent at startup.
- Still to verify: whether `ublk_drv` is enabled in the kernels of every distribution we target,
  including the edge ones (k3s hosts, Talos, MicroK8s). Where it is not, the NVMe/TCP loopback
  frontend is the fallback.
- The node needs `io_uring` enabled (`kernel.io_uring_disabled=0`). Default container seccomp
  profiles block it, so Flaminio ships its own profile.
