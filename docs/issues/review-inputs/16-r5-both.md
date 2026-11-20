# Issue #16 plan review, round 5 (incremental), design-code and design-security

## Earlier findings
- R4-1: fixed. session::connect returns after the handshake and then sends hello (session.rs:194-205), so reading a frame first is right. The client-under-test half (acceptor expired) fails in `connect` with the verifying text, and `accept` sees the alert. Residual risk: R5-B3.
- R4-2: fixed. `PKI_ROOT` under `FLAMINIO_SCRIPT_TEST=1` matches common.sh:19-23 and #12's decisions (R2-C6) and design. Positional passing and the no-switch case are specified. Residual gap: R5-B2.
- R4-3: mostly fixed (mkdir -m 0700, chown, chmod 0750; `file` under the lock; `due` unlocked is sound, since a half-renamed pair reads as mismatched and costs one extra renewal). See R5-B1, R5-B2.
- R4-N1: fixed.

## New findings

### R5-B1 — Low — A crash between `mkdir -m 0700` and `chmod 0750` leaves a group directory wrong for good
Design Installing: a new group directory (`meta-pki`, `meta-etcd` shares it, `io-pki`) is made 0700, then `chown`, then `chmod 0750`. The same section says an existing directory keeps its own owner and mode. A run killed (ssh drop on `csr`, which does not ignore SIGHUP; power loss; OOM) between `mkdir` and `chmod` leaves root 0700 or root:group 0700. Every later run keeps it, and the key inside (`root:flaminio-meta-pki` 0640) is unreadable to the group, so `meta.sh up` fails with no renewal fixing it. Round 4's order narrowed the exposure window but made this state permanent.
Fix: build the directory under a temporary name (`mkdir -m 0700 DIR.RUN.new`, `chown`, `chmod`) and `mv -T` it into place under the lock, so a directory is never visible half-set; or state that a root-owned empty 0700 directory is repaired by the list's owner and mode. Add a script case that kills between the steps, or names the repair.

### R5-B2 — Low — The pre-lock `mkdir` of `/etc/flaminio` has no stated privilege or race handling
Design Installing: "Only `/etc/flaminio` itself (`mkdir -m 0755`) is made before the lock". pki.sh runs as the VM user, so this needs `sudo`, which the text does not say for `csr`. Matters twice: (a) the no-switch case relies on the `sudo` stub refusing the real path. A bare `mkdir /etc/flaminio` would not go through it, and where the script tests run as root (a CI container) it would create the real directory. The stub list has `mkdir` running the real one. (b) Plain `mkdir` exits 1 when a concurrent run made it first, but the design says it "leaves it alone". Fix: say it is `sudo mkdir -m 0755 -- "$root" || sudo test -d "$root"`, with `$root` the already-resolved `PKI_ROOT` or default, and add that the no-switch case asserts no `mkdir` record without the sudo stub's refusal.

### R5-B3 — Low — The expired-connector half can see a reset instead of the alert
Design Tests, `reconnect_after_expiry_refused`, server under test: the connector's `connect` returns Ok, then `session::connect` sends hello (session.rs:204) while the server has already failed on the client's certificate, sent the alert and closed. A hello that reaches the closed socket makes the kernel send an RST, and the connector's read may return ECONNRESET rather than `received fatal alert: CertificateExpired`. Behaviour is racy on loopback and not guaranteed by the document. Fix: have the test connect with a bare `TlsConnector` (no hello) and read; or accept `connection reset` as an alternative only after asserting the server side's `invalid peer certificate: certificate expired`, which is deterministic. The same applies to the lab check if it greps only the connector's line (it matches either text on any host, so it is fine).
Reviewed commit: 7f245527e20fbe2ac27c2f1dfe3c4d96bec8c019
