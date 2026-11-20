# #52 plan review, round 2, security focus

Scope: `git diff c6efcd7..0d26cc2 -- docs/issues/52/ ':!docs/issues/52/report.md'`, checked against
`meta/service/auth.go`, `pkg/pki/pki.go`, `control/cmd/flaminio-ca/main.go`, `scripts/lab.sh`,
`scripts/guest/pki.sh`, `flaminio-net`/`flaminio-node` accept paths, and the clear plans #13, #16 and #73
(origin/main).

## Earlier findings
- R1-S1: fixed. AuthorizeSnapshotRead refuses by field 20 and by the header mark (contracts.md:116-120), and S8 is deferred.
- R1-S2: fixed. `flaminio-meta snapshot-read authorize|close` and the grant file; the worker refuses a non-`backup` pair; there are tests.
- R1-S3: fixed. #73-style rows, the worker rule, insecure instances refuse all six, OverrideDelete and the unparseable header run the barrier; there are tests.
- R1-S4: fixed. A follow-up is recorded and intent.md:156 is corrected.
- R1-S5: fixed. The `ListClosingSnapshotReads` poll, `forgotten` as the one exclusion proof, closing refuses new grants and sources, and the mark compares `reader`. See R2-S5 for a new gap in `closing`.
- R1-S6: fixed. The reader client accepts only the source node's `io` (contracts.md:219-223).
- R1-S7: fixed for issuance, renewal, pick-up and no resumption. Where the key lives on fl-w is a new finding (R2-S4).
- R1-S8: fixed.
- R1-S9: fixed on the node. The metadata half of the check is still missing (R2-S1).
- R1-S10: fixed for counts and idempotence. Pre-authentication slots are a new finding (R2-S2).
- R1-S11: fixed for scripts, id validation, quoting and the script test. The store's identity is a new finding (R2-S3).
- R1-S12: fixed.
- R1 Nit (prefix-relative AEAD key): fixed.

### R2-S1 — Medium
**ValidateSnapshotRead cannot check the worker, the ref or the current proof. With a caller-chosen operation id and grant collection, an old token can validate under another worker's grant.**
- Evidence:
  - The request is `node, grant reference, chunk, token digest` (contracts.md:127-130). It carries no worker and no ref. The source record holds `{device_uuid, store_generation, isr_version}` (contracts.md:91) and no token digest. Yet the method claims to refuse "another generation, worker or ref". The digest it receives is compared to nothing.
  - Operation ids may be supplied by the caller (`--operation-id`, contracts.md:342-344). #73 draws its operation ids in the service, "never by a caller" (73/contracts.md, Identifiers).
  - A closed grant is collected at `held` = 64 (contracts.md:100). After that, Authorize sees `grants/<op>` absent (contracts.md:204) and creates a fresh grant under the same `{volume, first_epoch, op}`. The grant `generation`'s value is never defined.
- Abuse case:
  1. Worker A's grant closes and is collected.
  2. An administrator retries or reuses that operation id for worker B.
  3. B registers chunk c on node N.
  4. A reconnects to N with its old FLBR token. The node's checks pass: A's certificate matches the token's worker, and the signature is valid.
  5. ValidateSnapshotRead finds an active grant at the same reference with a source for (N, c), so A reads under B's grant after its own was closed.
- The same gap lets a token superseded by a re-registration (which "replaces the record's proof in place", contracts.md:124) keep validating.
- **Fix:**
  - ValidateSnapshotRead's request carries the token's worker URI and its `SourceProofV1`, or the token itself.
  - Registration stores the issued token's digest in the source record.
  - Validation compares worker, ref, proof and digest.
  - Make the grant `generation` unique across a reference's lifetimes (for example the claim's etcd revision), or have the service draw the operation id.
  - Say that a non-identical repeat of an operation is `AlreadyExists`.
  - Tests:
    - a token from before a re-registration is refused;
    - a collected grant's token is refused under a new grant with the same operation id;
    - a different worker or ref is refused.

### R2-S2 — Low
**Reader connection caps have no rule for unauthenticated connections. Anyone who can reach the reader port can take all 16 slots.**
- Evidence:
  - "A node serves at most 16 reader connections … Over a cap: `Refused`" (contracts.md:224-227) does not say whether a connection counts during its TLS handshake and before ReaderHello is authenticated.
  - No handshake or Hello deadline and no idle limit is given.
  - Today's accept loop has none either (`flaminio-node/src/net.rs:168-200`).
- Effect:
  - TCP connections that stall, with or without a certificate, refuse every backup on that node.
  - A grant's worker can also cycle connections without limit. Each connection costs one ValidateSnapshotRead, because the caps bound concurrent connections, not their rate.
- **Fix:**
  - Count only authenticated sessions against the 16 and 2-per-grant caps.
  - Bound pending handshakes separately, with a handshake and Hello deadline and an idle limit. These are liveness bounds, not safety rules.
  - Rate-limit validations per (grant, node).
  - Tests: stalled TCP connections and stalled handshakes do not block an authorized worker.

### R2-S3 — Low
**The lab store's TLS identity, and what the worker checks of it, are unspecified.**
- Evidence:
  - design.md:252 "makes the store's TLS server pair in the VM with `pki.sh` from the lab CA".
  - `pki.sh` only makes keys and CSRs (`scripts/guest/pki.sh:12-17`). The lab CA signs on the host through `flaminio-ca sign`, which accepts only a Flaminio SPIFFE ID (`control/cmd/flaminio-ca/main.go:90`, `pkg/pki/pki.go:222-240`). So the third-party store process must hold a workload-CA certificate with a node or administrator identity.
  - An administrator name has full authority before activation (`auth.go:64-65`).
  - contracts.md:263 gives the worker only "TLS CA/client cert/key paths". It names no server identity to require. With the lab CA as its trust anchor, any workload certificate that has the right IP SAN would pass as the store. fl-w's `io` and `meta` certificates both carry fl-w's IP (`lab.sh:577-578`).
  - A store impostor can acknowledge PUTs and GETs it does not keep, so the worker would record a completed archive that is not durable.
- **Fix:**
  - Name the store's identity, for example `spiffe://<td>/flaminio/node/fl-w/backup-store`, issued `--server` only (never `--client`, never an `admin` id), through `issue` with a new `lab.sh` line.
  - The worker requires that exact URI SAN (configured), as R1-S6's fix does for the reader server.
  - Test: an `io`, `meta` or admin server certificate at the store's address is refused.

### R2-S4 — Low
**On fl-w, the worker runs where the administrator's key and the token-signing key live. "Never holds an administrator pair" is a configuration check, not isolation.**
- Evidence:
  - `pki issue` installs `admin` (`lab.sh:589-592`) and `token.key` (`lab.sh:578`, mode 600) in fl-w's `/etc/flaminio/pki`. That directory is root 0700 (`pki.sh:8-10`).
  - #16's `pki.sh` list (16/design.md:144-153) has no entry for `backup`. The plan adds only an `identities VM` line (contracts.md:71-74; design.md:255).
  - To read its key from that directory, the worker must run as root, and root can read `admin.key` and `token.key`.
  - The plan's own secret-file rule ("owned by the running user", contracts.md:265-268) presupposes a dedicated user.
- **Fix:**
  - The `backup` line gets a `pki.sh` entry of its own, for example `/etc/flaminio/backup-pki` 0700, owned by a `flaminio-backup` system user, with its first key 0600. Renewal keeps owner and mode (#16).
  - The lab runs `flaminio-backup` (and `repo init`, and the S3 credential file) as that user.
  - Otherwise, state that the lab gives the worker no isolation from the administrator on fl-w.

### R2-S5 — Low
**`closing` is never cleared, so a volume whose deletion stops after its intent refuses every backup permanently. The named risk says the opposite.**
- Evidence:
  - The reader record's `closing` "is set by a deletion" (contracts.md:93), and "a closing record refuses AuthorizeSnapshotRead" (contracts.md:188).
  - No call clears it.
  - The named risk says "a deletion's intent is not undone if that deletion then fails … so its backups must be authorized again" (design.md:289; decisions.md:166-167). They cannot be authorized again.
  - After the intent, a deletion can still end `Aborted` on contention (`service.go:581`). If the caller does not retry, the volume stays externally present with backups barred.
- **Fix:** Pick one rule and test it.
  - Either state that `closing` is final (the volume is committed to deletion, the caller must re-run DeleteVolume, and AuthorizeSnapshotRead's error says so) and correct the risk line;
  - or clear `closing` in the transaction that brings `open` to 0 when no mark follows, and say how the mark's compare still prevents a race.

## Nits
- contracts.md:139-142: AckSnapshotReadClosed should require the grant to be `closing`. Otherwise a node's ack on an active grant can make it `closed` at `pending` = 0.
- contracts.md:112-113: "an invalid id or URI" should say that the worker URI must parse as a `backup` identity in the instance's trust domain, in `pki.ParseID(..).String()` form, as #73 requires of bound IDs.

Reviewed commit: ca86b81d5e88f4024700f5e62733010f7c63dd05
