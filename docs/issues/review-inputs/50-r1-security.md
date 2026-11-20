# Issue #50 — plan review, round 1, focus: design-security

Scope: docs/issues/50/ (intent, spec, design, adr-proposal, contracts, decisions), checked against
the code in /home/jsfakian/Documents/src/flaminio-50 and the clear plans #12, #13, #16, #73, #74,
#79, #83 on origin/main.

### R1-S1 — Critical
**An older metadata binary destroys the wrapped key, and answers an encrypted volume as plain.**
The only copy of the wrapped key is a field of the volume's header. Every header write is
read-whole, change, write-whole: `readHeader` decodes into `domain.Header` with
`json.Unmarshal`, which drops unknown fields (`meta/service/service.go:275-291`), and Attach
writes the struct back (`service.go:723-725`; also `:541`). A metadata instance without this
change (one not yet upgraded, or rolled back) that serves one Attach of an encrypted volume
erases the descriptor: the key is gone for good and the record now says plain. Without any write,
the same instance answers GetVolume and Attach with no descriptor (`volumePB`, `service.go:337`;
it ignores `accepted_profile`), so a new data plane builds a plain target: the application reads
ciphertext and its writes reach every replica in clear. The design has only the sentence "All
metadata instances must support the new profile before encrypted provisioning" (design.md:47):
no mechanism, no test, nothing for a rollback; S12 covers only an old data plane.
**Fix:** (1) keep the envelope in a record of its own (keyed by id and first epoch), written in
the creation and deleted with the volume, which no header rewrite touches; the service and the
target take "encrypted" from it. (2) A data plane and a plugin with this change accept a
GetVolume/Attach answer only if it carries a field a profile-aware instance always sets, so an
old instance's answer fails the attach instead of reading as plain. (3) Encrypted creation is
refused until a durable store record says every instance is profile-aware; state what a rollback
below that does. Tests: an old-binary header rewrite keeps the key; an old instance's answer is
refused by the target; add both to S12.

### R1-S2 — High
**Whether a target encrypts follows the control-socket verb, and the other attach paths are not
composed.** The design adds `attach-encrypted V TOKEN xts-aes256-v1` and says the "plain attach
line stays unchanged" (contracts.md:60-61; design.md:50-52). It never says what a daemon with
this change does with the plain line for a record that has a descriptor (an older plugin, a
manual attach, the startup `--attach`/`--token` flags, `node.rs:42-43`; the socket's one attach
arm is `control.rs:84`). #13's single GetVolume constructor would send `accepted_profile` on
every read (13/design.md:312-315), so the service returns the encrypted record to a plain attach
and nothing in the plan refuses it. The clear plans add more attach paths that the new verb does
not cover: #79's `attach-init` (the private `mkfs` of every new ext4 volume, which must be
encrypted too) and #83's `attach-challenge` and required HANDOFF operand
(83/fresh-process-contract.md:8-10, :40-45; 79/initialization-contract.md). As written an
encrypted ext4 volume either cannot be initialized or is formatted in clear.
**Fix:** the record decides, never the caller: every path that starts a target (attach,
attach-init, startup flags) reads the descriptor and either holds the key or fails; the verb or
operand is only a capability check, given for each of #79's and #83's lines with its grammar.
Tests: plain line, startup flags and attach-init on an encrypted record, each with no device and
no plaintext on a replica.

### R1-S3 — Medium
**The release relies on a server check the code does not make.** contracts.md:54 says "mTLS
authenticates sender"; HPKE Base authenticates no sender, and Q7 gives the data plane no check
of the key. The metadata client verifies the CA and the address only
(`flaminio-metaclient/src/lib.rs:85-92`; `pkg/pki/pki.go:300-310`), and an `io` certificate has
the server usage and its node's address (`scripts/lab.sh:584`). In the lab every metadata
address is also a data node's. Whoever answers there with a node's `io` key (74/design.md:333-338
names this) can seal a key of its choice to the target, which then writes the tenant's data
under a key the attacker knows; it can equally answer GetVolume without a descriptor. Intent 7
("a key that is not the volume's is refused, not used") then fails.
**Fix:** name #16's S15 (the client accepts only a `meta` URI SAN) as a prerequisite of
ReleaseVolumeKey and of the encrypted GetVolume, or make that check in this change for both
calls. Test: an `io` server certificate at a metadata address is refused before any key is used.

### R1-S4 — Medium
**An instance with authentication off would release keys to anyone.** With `TrustDomain == ""`
the interceptor calls the handler with no identity (`meta/service/auth.go:42-44`), and the
existing target check is skipped for an unauthenticated caller (`service.go:801-804`). The design
says only "authorizes io cert node==current target" and lets a KEK be configured independently
of TLS. #73 already refuses its new methods on insecure instances (73/contracts.md:74-76).
**Fix:** `--insecure` with a key source is refused at start; ReleaseVolumeKey and an encrypted
CreateVolume are refused whenever no identity is authenticated; add ReleaseVolumeKey's row to
#73's method table (the target's data plane, both columns). Test for each.

### R1-S5 — Medium
**The KEK's identity is not tied to its bytes.** `SourceIdentity{Kind, ID, KEKVersion}` are
strings whose origin for the static source is not given (contracts.md:5-6), and S5 only asks
that the file be "the same on every instance". Two instances with different KEKs under the same
strings both pass every check: a volume created through one is answered by the other as
INVALID_ENVELOPE, which is never retried elsewhere (design.md:69-70), and the Rust client then
prefers that instance (`lib.rs:158-161`), so attaches fail though another instance holds the
right KEK. A KEK regenerated and pushed again (the lab's "make once" rests on a workstation
file) silently orphans every existing volume while new creations succeed.
**Fix:** derive KEKVersion from the key (a labelled HMAC or HKDF check value); pin the cluster's
source identity in etcd with a conditional write at first use; an instance whose KEK differs has
no usable source (INSTANCE_UNAVAILABLE, try another) and refuses to wrap. Test: two instances
with different KEKs; a replaced KEK.

### R1-S6 — Medium
**"Given as the token key is" puts the KEK on a command line.** S5 and S18 give the KEK as the
token key is given. That path is `put_file`, which passes the file's base64 in `ENV_VARS` on the
ssh command (`scripts/lab.sh:556-559`, `:579`; `scripts/lib.sh:157`; `scripts/guest/pki.sh:27`):
the key is in the argv of `ssh` on the workstation and of `env` in the VM. Intent 6 and
design.md:158-159 forbid that, and the design names no other transfer, no path and no generator.
**Fix:** name them: the generator, the path (under `/etc/flaminio/pki`, never #12's `io-pki`),
and a transfer over ssh's stdin. The script test asserts that no recorded argument or
`ENV_VARS` holds the KEK's bytes or base64.

### R1-S7 — Medium
**The static file's checks and the "missing KEK" case do not fit how the service runs.** The
service is a `DynamicUser` unit that reads its keys as systemd credentials
(`scripts/guest/meta.sh:21-36`). (a) "Root owner/no group/world access/no symlink" (design.md:57)
cannot be checked by that process on the root-only file, which it cannot open; on the credential
copy the owner and mode are systemd's. (b) A `LoadCredential=` whose absolute path is missing
fails the unit's start, so with the KEK moved aside the instance does not "still serve
everything else" (S5), and S19's fourth bullet and the design's lab step see an unreachable
service, not the typed no-source refusal.
**Fix:** state the unit's delivery (for instance a credential that may be absent, or a readable
directory), which file the checks apply to and which owner they accept, and confirm on the lab's
systemd that a missing KEK leaves the unit running. The script test covers the unit text.

### R1-S8 — Medium
**#73's audit hand-off is not taken.** #73 leaves the audit of key operations to #50
(73/spec.md:132-133) and requires each later issue's hook, or its record in
`docs/followups.md` (73/decisions.md:112-115; batch ledger line 222). #50 only logs a release
(S7; design.md:76), calls the audit log out of scope because "neither exists yet"
(intent.md:120-121), and adds no follow-up. Refusals are logged per call with no bound, and any
node's `io` identity can call the method; #73 bounds refusal logs per caller (73/spec.md S7).
**Fix:** either append a key-release and a key-creation event through #73's chain (and say what
S7's "a release changes no record" then means), or record the follow-up in
`docs/followups.md` and correct the intent; bound refusal logging per caller. Test the choice.

### R1-S9 — Medium
**#73's unencrypted policy is not fed.** #73 refuses a tenant creation unless unencrypted volumes
are allowed, treating every volume as unencrypted "until #50" (73/spec.md:72-74), and the ledger
hands `CreateVolumeRequest.encrypted` to that check. #50 never mentions it. Unwired, a namespace
with `allow_unencrypted=false` can create nothing, encrypted or not, or the check keeps passing
plain volumes.
**Fix:** a rule and a test: the policy check reads `encrypted`; a refusal comes before any key
is made or any source call (S6); whichever of #50 and #73 lands second wires it.

### R1-S10 — Medium
**The ADR's statement of what the profile hides is incomplete.** It lists one leak, allocation
(adr-proposal.md:40). Not stated: (a) XTS is deterministic per key and block number, and the
store keeps earlier versions of a block on the same disk (redirect-on-write frees clusters
without erasing them; `DeferredWrite` journal records carry data,
`docs/format/on-disk-format-v1.md:363-364`), so a raw disk, and any replica over time, shows
which 16-byte units of a block changed between versions; (b) volume ids, sizes, chunk maps and
checksums are in clear; (c) "offline disclosure by never-target replicas" (:16) does not hold
for a node that also runs a metadata instance: its disks hold the KEK and etcd's wrapped keys,
which is every node of the lab.
**Fix:** add the three to Consequences and to the weaker-source notice of S5; word the claim as
"a data disk alone, or a node that holds no KEK".

### R1-S11 — Low
**The recipient's inputs to the HPKE `info` are not named.** contracts.md:52-53 lists the
fields; it does not say that the data plane takes the volume, first epoch, epoch and node from
its verified capability token and its own request. Taken from the response (`first_epoch=4`,
`profile=3`), the binding to the incarnation is empty.
**Fix:** say so, and that the response's two fields must equal the token's values or the attach
fails. Test: a response for another first epoch.

### R1-S12 — Low
**No rule for key material in the metadata service's memory.** S11's lock, dump and zeroize
rules are the data plane's. The KEK lives in the service for its whole life and each DEK for a
call, in a unit with no core-dump setting (`scripts/guest/meta.sh`); the design defers "Go GC
copies" to a review (design.md:95).
**Fix:** a rule: the service is not dumpable when a source is configured, and the unit says so;
or record the exposure in the ADR's Consequences beside the weaker-source notice.

### R1-S13 — Low
**The GetVolume refusal reaches replica-only nodes.** "io callers of GetVolume must advertise
accepted profile" (design.md:47-49) refuses every older data plane, though S17 says a replica
needs nothing new. #13 gives replica-only nodes GetVolume reads (the sweep and the lookups,
13/design.md:300-302, :312-315): an older replica node then skips an encrypted volume's
deletions and releases for as long as it stays old.
**Fix:** set `accepted_profile` in #13's one constructor and say so; state the effect on an
older replica-only node, or refuse only the reads that build a target.

## Nits
- spec.md:3-5 ("Nothing is settled yet", "the design come after approval"), design.md:170-173
  ("unresolved Q1–Q12", "S20 approval is not fabricated") and adr-proposal.md:11, :44-45 ("none
  exists", "remain open") contradict the settlement lines beside them.
- contracts.md:5-7: name the generator of the DEK and the KEK (the OS CSPRNG) and refuse a DEK
  whose two halves are equal, as XTS requires.
- contracts.md:19-20: the envelope's `Version` is not in the AAD; only the fixed label is. Say
  that a new version takes a new label.

Reviewed commit: 9edd5903655a66e3ffbfe6c1d0c47d5d5050b266
