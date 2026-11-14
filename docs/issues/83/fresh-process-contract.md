# Accepted with the plan, 2026-11-13: fresh-process target handoff (S5)

Part of #83; accepted with the plan, 2026-11-13. This preserves existing metadata per-method authorization and FLCT
replica capability encoding. It does not treat clock/profile state as replication authority.

## Authorized issuer and process challenge

After profile admission, before accepting target starts, daemon generates32 CSPRNG bytes from
the OS; failure/all-zero refuses startup. `attach-challenge` on the root-only control socket
returns `ok ` plus64 lowercase hex characters. The challenge remains constant only for this
healthy daemon process; quarantine makes the query and target starts refuse. It is distinct
from #15's nonzero u64 INSTANCE/GEN and host boot UUID: those are not cryptographic nonces.
#15 device-info/conditional-detach identity remains required and unchanged.

A node-agent client reads the challenge, calls existing Attach with only_if_target=true and
its own node/agent mTLS identity, then passes the signed results to that daemon. It never
lends agent credentials to node/io. Administrator clients use the same handshake with their
existing authorized Attach role. Rust metaclient does not gain Attach or agent credentials.
CSI `Node.device`, metactl/admin scripts and every startup target path use this sequence.
Changed target/metadata outage fails; only_if_target cannot steal ownership. A lost Attach
reply may advance the epoch again on retry, preserving the existing conditional transaction.

Add `AttachRequest.bytes target_nonce=6` (#79 reserves use_profile=5), and
`AttachResponse.bytes target_handoff=3`. Nonempty nonce must be exactly32 nonzero bytes;
reject before storage effects. Existing auth.go Attach policy is unchanged and tested.
A handoff is returned only after an actual new-epoch Attach transaction commits; nonce plus
unless_target=true is InvalidArgument, never a read-only mint for an old epoch/token. Empty
nonce retains legacy API response semantics but returns no handoff, so cannot start a strict
daemon. Keep #79 use-mode/initialization gates on every issuing path.

## One signed local envelope, unchanged replica tokens

Envelope body: magic4=`FLCH`, version u8=1, purpose u8 (1 normal,2 private initialization),
nonce32 raw bytes, SHA256(exact raw FLCT capability bytes)32; append metadata Ed25519 signature64
over all preceding bytes. Exact length134, no trailing bytes or alternate version/purpose.
Verify both signatures and all existing FLCT volume/node/FirstEpoch/epoch rules; digest binding
therefore binds the exact capability and incarnation/epoch/node, not a caller assertion. Clock
profile does not admit legacy FirstEpoch0 target lifetimes (#15). No expiry/issue time proves
freshness: the unpredictable process challenge and new-epoch mint do. Do not log either token.

Extend the existing attach/attach-operation wrappers with a required HANDOFF operand in the
strict profile; attach-init adds it after its existing FLCI and FLCT operands. Keep #79
operation IDs and #15 exact lifetime recovery semantics. Handoff is unpadded base64url (179
characters for134 bytes); token operands stay bounded4096 bytes, strict arity before effects.
Legacy attach VOLUME TOKEN, startup --attach/--token, automatic restart and sync/fallback paths
must refuse missing handoff before registry/engine effects. Startup with old flags exits with
an actionable error; start the daemon unattached, then use authorized agent/admin handshake.
Engine::start_target itself requires the verified current-process handoff, not just socket
validation; local/remote replica connections still use the unchanged FLCT capability.
Existing per-process ran_epochs, verified current metadata and replica fences remain: the
handoff cannot permit a second target session sequence at the same epoch within one process.
An exact operation retry may return its existing live device, never start another lifetime.

## Compose private initialization without reopening uncertain work

#79 FLCI remains a separate purpose-specific initializer ticket, never process-freshness proof.
Extend its InitializationRequest with bytes target_nonce=6 and InitializationCredentials with
bytes target_handoff=3. First successful Reserve stores the nonce in its permanent operation
state and signs purpose2 FLCH over the accompanying replica FLCT only after that new-epoch CAS.
Same Reserved op/node/epoch **and same nonce** may recover credentials after a lost reply.
A different daemon process/nonce cannot reuse that epoch: refuse without issuing a handoff.
Started never returns credentials or another Start permission. All existing Get/Start/Finish
per-method rules remain, with explicit nonce validation where required; no node/io mint path.

Thus a CSI plugin restart with the same daemon can recover operation-bound attachments, but
private initialization interrupted by a daemon restart remains blocked for quiesced manual
recovery; it is not automatically reformatted or resumed at an old epoch. Keep the physical
bytes, authority/operation history and local Started markers. No reset-to-fresh or nonce-rebind
API is added. Normal ReadyFs/Raw target recovery instead obtains an actual fresh Attach epoch.
This explicit availability limitation does not weaken #79's one-shot Start/history guarantees.

## Files, compatibility and executable checks

Update api/proto + make proto, Go service/authorization tests, Go captoken handoff codec,
Rust net handoff verifier, daemon control/engine startup, CSI dataplane/Node.device, metactl
and node startup scripts. Update #15 lifetime integration and #79 initializer calls against
this appendix; neither older plan already implements it. No FLCT/FLCI/replication-frame/disk
format change is implied; the new signed FLCH local ABI and reserved metadata fields are
Proposed. #80 counts the <=134-byte handoff response field in exact response fixtures; its
existing B/header allowance is checked, never raise limits. No steady-state metadata call/lock.
Settled at approval, 2026-11-13: the FLCH local ABI and reserved metadata fields are accepted with the plan (see decisions.md, Settled at approval).

Real mTLS Go-service/Rust-daemon/agent fixture: io/Reader/other-node/anonymous cannot mint;
wrong capability digest/node/incarnation/epoch/nonce/purpose, missing/oversized/malformed or
unknown-version envelope refuses before target allocation/Hello. Hold old token/handoff,
restart a target with no local replica fence and reject it; new challenge+agent Attach obtains
strictly newer epoch and succeeds only after ordinary reconciliation. Include lost replies,
metadata outage/changed target, every legacy/startup/fallback entry, same-process operation
retry, same-epoch second target refusal, private Reserved same/new-daemon behavior, and Started
refusal with no extra mkfs. Existing FLCT v1/v2 codec vectors and remote peers remain compatible.

Model process nonces/authorized issuance independently of admitted target epochs. Assert
FreshProcessEpoch and existing session/linearizability rules through quarantine/restart; catch
`accept_old_process_handoff` on a no-local-replica restart seed and
`mint_handoff_without_epoch_advance` on a same-epoch issuance seed. Exact Go CAS/crypto/daemon
integration replays those schedules; a valid signature alone cannot stand in for the model.
These are required future tests, not results of drafting. No lab or networked authority action.
