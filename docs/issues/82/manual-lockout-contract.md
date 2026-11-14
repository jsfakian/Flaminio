# Accepted with the plan, 2026-11-13: manual-lockout-v1 authority and terminal return policy

This is the concrete first production profile for #82 S2–S4/S7/S9, not an accepted ADR.
It supports physically controlled machines with trusted lockout operators. No power command,
physical action, signature, Node deletion or deployment is performed while drafting.

## Authority, identity and signed input

The authority is an administrator operating physical power isolation and restart lockout.
After identifying the machine by immutable inventory serial and isolating every power/start
path, that operator signs the assertion below. This is a trusted assertion, not remotely
measured proof. The operator keeps the physical lock and its key until the terminal retirement
barrier below; neither time, Ready, a deleted proof file nor a controller crash releases it.
Machines without enforceable physical isolation (including ordinary lab VMs) are unsupported.

Failover-mode flag `flaminio-csi failover --failover-authority-config PATH` is absent by default: observe-only. Config v1
is root-owned, not group/world writable, opened no-follow, max16KiB JSON with exact fields
`version:1,profile:"manual-lockout-v1",cluster_uid,proof_dir,keys:[{key_id,public_key}],
inventory:[{node_name,node_uid,machine_id}]`.
At most8 distinct key IDs; IDs1–64 ASCII alphanumeric/_/-, public keys canonical base64 of
32-byte Ed25519 keys. `cluster_uid` equals the current kube-system namespace UID. Config has
no private key or command string. Pin the API CA and normal authenticated Kubernetes endpoint;
add namespace get for kube-system only to controller RBAC, without pod/attachment/node delete.

`proof_dir/SHA256(NodeUID).json` is a root-owned, immutable-to-controller regular file beneath
an approved pinned no-symlink directory. Open no-follow, <=8KiB, no unknown/duplicate fields:
`version:1,profile,cluster_uid,node_name,node_uid,machine_id,operation_id,key_id,assertion,
locked_at,signature`. Names obey Kubernetes grammar; UID/machine ID1–128 printable ASCII;
operation ID is64 lowercase hex characters; locked_at is diagnostic UTC, never expiry authority.
`assertion` is exactly `physically-isolated-until-node-uid-retired-v1`. Signature is canonical
base64 of64 bytes over domain `flaminio-manual-lockout-v1\n` plus a specified fixed field-order
JSON array `[version,profile,cluster_uid,node_name,node_uid,machine_id,operation_id,key_id,
assertion,locked_at]` (UTF-8, compact JSON, no optional fields).
Go signer/verifier share canonicalization vectors; never sign arbitrary unbounded input.

Root-owned inventory in config maps node name+UID to one immutable machine_id (max4096 rows,
config size cap still applies).
Duplicate/ambiguous mapping refuses. Inventory is explicitly maintained from physical asset
records, not a tenant annotation or mutable providerID. Node providerID, if populated, must
match inventory; its absence is supported for physical nodes. ResolveIdentity returns this
machine ID as MachineGeneration and binds exact UID. No lookup by name alone is authoritative.

`flaminio-csi fence-sign --config PATH --node NAME --node-uid UID --machine-id ID
--operation-id HEX --locked-at UTC --key-id ID --signing-key PATH --output PATH` validates
all fields/config bounds, requires the exact assertion via explicit
`--assert-physical-lockout`, and emits one signed proof by exclusive temp/fsync/rename/dir-fsync.
The key is an Ed25519 PKCS8 file, root-only/no-follow; it is never mounted in controller pods.
The command reports that it cannot verify physical isolation. Installing the signed proof is
an explicit administrative action. Controller credentials can read/verify, never mint proof.

## Mark and unknown-outcome lifecycle

Per attempt, fetch exact current Node and proof, verify signature/key/config/inventory/cluster
binding and compare operation ID. Re-read before the resourceVersion-conditional mark and
validate proof again. The permanent assertion covers the entire old UID lifetime, so there is
no revocable short-lived observation gap before PATCH. PATCH includes expected UID and RV,
exact owner record, and preserves foreign taints; existing ownership checks remain mandatory.
An unproved candidate, invalid proof, wrong UID/serial/cluster or unsupported profile does
nothing. There is one bounded point lookup per candidate; never scan all proof files.

Proof removal/key rotation stops new marks, but cannot physically unlock a held machine.
Operators must retain the lock until retirement even if proof/config is removed. A valid old
signed proof cannot authorize a replacement UID. Lost PATCH replies re-read and recognize
only the exact recorded operation; a restart repeats the same checks. Never generate another
operation merely because the previous reply was lost. Mutable Node annotations carry attribution,
not authority. Conflicting valid proofs for one UID block rather than select a new winner.

## Terminal retirement, not same-incarnation rejoin

This profile does not support automatic Clear or same-UID return. VerifyReturn/Clear refuse
with `terminal-retirement-required`; Ready/heartbeat cannot change it. Successful shutdown
recovery consists of the owned taint allowing Kubernetes's existing recovery; storage takeover
still uses all existing fences/waits/reconciliation. No pod/VolumeAttachment mutation is added.

A later separately authorized administrator performs the documented retirement procedure:
keep physical lock held; mark the old metadata node draining; inspect that recovery has moved
or explicitly accounted for every old workload/attachment; then DELETE the old Kubernetes Node
with exact UID and resourceVersion preconditions. An unknown delete result is resolved by GET:
404 or a different UID establishes retirement; the same UID requires retry/inspection. Never
delete a replacement Node. The controller's owned mark retires with its old UID; it does not
remove a foreign mark or require a broad controller DELETE permission.

Only after that API barrier may an operator prepare a cold replacement using a **fresh node
name and PKI identity**, a clean OS boot medium and no old workload/startup state. Preserve the
old OS medium, all replica disks and all data-encryption/recovery keys offline and protected;
do not wipe, reformat or automatically advertise surviving copies. Old NodeName/PKI identity,
old capability tokens and disk/bootstrap identity never become fresh because Kubernetes UID
changed. #13/#14 deletion/fence obligations and #61 identity/bootstrap/recovery constraints
must be accounted for before any retained disk is admitted again. With no supported recovery
procedure, keep those disks offline and the relevant obligation open. Never destroy a unique
surviving copy to make retirement appear complete. Physical unlock/cold boot is the final
operator action after these preparations, not something Ready or the controller performs.

A delayed old PATCH cannot affect the new name/UID, even after physical release. The operator
must not unlock and restart the old installation or reuse its node credentials. This is an
explicit trusted administrative boundary, like maintaining physical isolation itself. The
profile sacrifices automatic same-machine return for a concrete conservative lifecycle.

## Files, qualification and tests

Implement `control/failover/manual_lockout.go` plus signer/verifier tests, `control/cmd/
flaminio-csi` fence-sign/config wiring, `flaminio-failover` deployment args and read-only proof/config mounts,
and the scoped namespace-read RBAC. There is no new metadata method/proto/token/store format.
Inventory/config parsing and signature validation use bounded standard-library Go facilities.

Add real crypto/file and httptest API tests: valid physically asserted fixture marks; wrong
cluster/key/signature/UID/serial/profile, oversize/duplicate/unknown JSON, symlink, permissive
write modes and ambiguous inventory refuse; signed-file removal/key rotation never authorizes
release; lost mark/delete replies and controller restarts preserve ownership; old held requests
after retirement/new name cannot mutate replacement; Clear always refuses. A scripted operator
runbook test asserts retirement ordering, fresh identity and preservation of disks/keys, and
refuses missing workload/attachment accounting. Fake signed fixtures test protocol only; no
automated test claims to certify real physical lockout or hardware identity. No lab is needed
for the document fix. A future explicitly authorized disposable physical-host qualification
records actual lockout/recovery/cold-return evidence before production support is claimed.
