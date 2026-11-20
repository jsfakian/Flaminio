# Issue #16 plan review, round 6 (incremental), design-code and design-security
Reviewed: 7f24552..35b7a52 (docs/issues/16/, report.md excluded).

## Round-5 findings
- R5-B1: fixed. A directory is built as DIR.RUN.new and moved by `mv -T`, so it is never visible half-set. The cleanup cannot remove a directory another run is building: every build (csr, install, file) is under the one lock and the cleanup is in install's step 6, also under it. The age test is on a directory built in seconds. See R6-1 for the cleanup's unstated command and pattern.
- R5-B2: fixed. `sudo mkdir -p -m 0755 -- "$root"` is a no-op when the root exists or a concurrent run made it. In the no-switch case the arguments name the real /etc/flaminio, so the sudo stub refuses it before any mkdir runs, and "no mkdir recorded" holds.
- R5-B3: mostly fixed. The bare connector removes the hello that caused the reset. See R6-2 for the remaining claim.

## New findings

### R6-1 - Low - The cleanup of aged temporary directories names neither the command nor the match
Design Installing, step 6 says "removes ... the root's temporary directories (`DIR.RUN.new`) older than an hour", but the step's `find DIR -maxdepth 1 -mmin +60` looks inside one name's directory. The temporary directories are beside the directories, in the root, so the scan is of `$root` and must be anchored to the known directory names (`pki`, `meta-pki`, `meta-etcd`, `agent-pki`, `io-pki`, ...) followed by `.RUN.new`, with `-type d`. A bare `*.new` over the root would also hit a `.new` file of another name. The removal must be `rmdir` (the directory is empty by construction) and not `rm -rf`, which is a recursive delete as root on a pattern; an `rmdir` also refuses if a run did put something in it. The test case ("a temporary directory aged over an hour (`touch -t`) is removed") does not check that a younger one, and a final directory with a similar name, are kept.
Fix: say the scan is of `$root`, `-maxdepth 1 -type d -name '<known dir>.[0-9a-z]*.new' -mmin +60`, removed with `rmdir`. Extend the case: a fresh temporary directory and a final directory survive the same `install`.

### R6-2 - Low - "No reset can replace the alert" is stronger than the facts
Design Tests, `reconnect_after_expiry_refused`, server under test. Removing the hello does remove the known cause. rustls 0.23.46 (Cargo.lock:1463) with TLS 1.3 sends the alert when `process_new_packets` fails on the client's certificate, and the connector's read then returns `received fatal alert: CertificateExpired`. But the client's flight (Certificate, CertificateVerify, Finished) arrives after the server has started failing: if the server's read took only part of it (the certificate chain larger than one read buffer, or the flight split across segments), the server closes with unread bytes, and Linux sends an RST that can surface as ECONNRESET at the connector instead of the alert. Test certificates are small and loopback usually delivers the flight whole, so this is rare, but the text promises it cannot happen.
Fix: word it as "removes the known cause (the hello written after the close)"; keep the test's test certificates a single small leaf; and have the server-side text asserted unconditionally and the connector's text asserted with a retry-free read of the first error only. If the connector's text must stay deterministic, have the server read to the end of the client's flight (it has: `accept` returns the error only after `process_new_packets`; the test can `shutdown(Write)` the server's socket and drain before dropping it). Otherwise state the residual in Design Tests.

## Nits
None.
