# Issue #16 plan review, round 4 (incremental), design-code and design-security
Diff b40a661..3659ed2.

## Round-3 findings
- R3-1 fixed. rustls 0.23.46 (Cargo.lock; source in ~/.cargo/registry): `InvalidCertificate` prints `invalid peer certificate: {err}` (error.rs:987), `ExpiredContext` prints `certificate expired: verification time ...` (error.rs:701), `AlertReceived` prints `received fatal alert: {alert:?}` (error.rs:985). Both log lines carry them: SessionError::Io wraps the rustls error, so `why` (flaminio-node/src/net.rs:294, engine.rs:828-831) and the accept line (net.rs:196) contain the texts. See R4-1 for which side sees which.
- R3-2 mostly fixed (stubs, mkdir+chown+chmod, FLAMINIO_ETC named); see R4-2, R4-3.
- R3-3 fixed. `Identity` Display (Go pki.go:202, Rust identity.rs:44) gives `spiffe://td/flaminio/node/fl-w/io`, so the asserted text is right.
- R3-4 fixed (wording nit N1). R3-5, R3-6, R3-7, N1 fixed.

## New findings

### R4-1 (Low) The expiry texts are asserted on the wrong side in the Rust test if the expired side connects
`reconnect_after_expiry_refused` (design Tests) says the other side's error contains `received fatal alert: CertificateExpired`. In TLS 1.3 an expired client certificate is rejected by the server after the client's handshake already returned Ok (session::connect, session.rs:194-205 then sends hello). The alert surfaces only on the client's first read, not from `connect`. The test must say: when the expired pair is the connector's, read a frame (or await the hello's reply) before asserting. Same for the lab: fl-c's outgoing `Lost` line appears only after a read, and on that direction the texts swap (peer's accept line has `invalid peer certificate`, fl-c's connect side has `received fatal alert`). The design's per-side lines hold only for sessions that peers open to fl-c; say the check matches either line on any host (grep for the text, not the host).

### R4-2 (Low) `FLAMINIO_ETC` is an unconditional override of a root-owned tree
pki.sh runs as root or via sudo and writes keys, chown/chmod, rm -f. Nothing in the design restricts the variable to tests; "which only the test sets" is a convention. A stray exported value in an operator's shell (nothing else in the repo uses the name) silently sends keys and the lock to another tree and leaves the VM unrenewed. Also `sudo` env_reset drops it for `sudo flock $ETC/.pki.lock sh -c`: the design must pass the root as a positional parameter into that shell, as it does for other values. Fix: pass it as a positional parameter; honor it only when not root / when `FLAMINIO_TEST=1`, or name it `FLAMINIO_TEST_ETC` and refuse it when `id -u` is 0; add a case that root-path default holds when unset.

### R4-3 (Low) Directory creation vs the lock, and `file` outside it
The lock file lives in `$ETC`, which must exist before `flock`; `mkdir` of `$ETC` and of the name's directory, then `chown`/`chmod`, therefore run outside the lock in `csr` (a concurrent run can see the directory between `mkdir` and `chmod`, and write a key into it at the default mode). Say that creation uses `mkdir -m` with the final mode (or `umask 077`) before chown, and that creation is idempotent (`mkdir -p` race). `file NAME` is not stated to hold the lock, and the lock test lists only `csr` and `install`; either take it (it creates directories) or state why not, and test it.

## Nit
1. spec Tests line says "an existing directory's mode and group kept"; the S9 rule says owner, group and mode. Align (the design's test covers all three through the no-`chown`/`chgrp`/`chmod` assertion).
