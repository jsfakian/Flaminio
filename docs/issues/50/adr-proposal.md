# Proposal: A restricted XTS profile with authenticated key delivery

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12

## Context

[Issue 50](https://github.com/jsfakian/Flaminio/issues/50), [spec](spec.md),
[design](design.md). The spec scopes confidentiality-only XTS before KMS/integrity. Audit
proposed ADR 0009 requires stronger authenticated data identity/version binding for
malicious-replica protection and retained key references. S20 demands explicit acceptance
before implementation. Settled at approval, 2026-11-13: accepted with the plan (see
decisions.md, Settled at approval).

## Decision proposed for review

Offer xts-aes256-v1 only as confidentiality against offline disclosure of a data disk alone, or
by a node that holds no KEK (one that runs no metadata instance and was never the target); it
does not protect ciphertext tampering, substitution, zero-sentinel injection or replay.
Do not claim the architecture's stronger malicious-replica guarantee. Choosing that stronger
profile requires a revised integrity-bearing format/spec first; this draft does not accept 0009.

AES-256-XTS uses a 64-byte DEK (two independent keys) and the LE128 logical 4 KiB address as
tweak; the stored zero sentinel keeps sparse-zero semantics. Encrypted devices use verified
copied reads. Metadata wraps at creation, keeps the wrapped key in a record of its own, and
releases only to the current target's `io` at the current epoch; the static 32-byte KEK is the
weaker first source, its version derived from its bytes. Wrapping authenticates the original
volume incarnation, profile and source version. #57 may load the identical static KEK from TPM
without envelope aliases.

Propose HPKE RFC 9180 Base X25519/HKDF-SHA256/AES-256-GCM inside mTLS for release so normal
RPC buffers never hold the clear DEK; plaintext open and cipher schedules require locked secret
arenas. Go uses the standard `crypto/hpke`; the Rust library paths (RustCrypto, `hpke`) require
review, vectors, memory-lifetime audit and benchmark comparison before selection and
implementation. No Rust library acceptance implied.
Settled at approval, 2026-11-13: Q2: measured and reviewed before selection (see decisions.md, Settled at approval).

No erase or retained encrypted snapshot/backup API is added. Future retained references must
pin wrapped descriptors/source versions before live deletion. New authenticated profiles and
linked-clone data identities require an explicit version transition. KEK rewrap, DEK rewrite
and repository-key rotation remain separate later operations, not this slice's claims.

## Consequences and unresolved choices

What a reader of stored bytes still learns: the sparse-zero sentinel shows allocation. XTS is
deterministic per key and block number, and the store keeps earlier versions of a block on the
same disk (redirect-on-write frees clusters without erasing them; `DeferredWrite` journal
records carry data, `docs/format/on-disk-format-v1.md`), so a raw disk, or a replica watched
over time, shows which 16-byte units of a block changed between versions. Volume ids, sizes,
chunk maps and checksums are in clear. A node that runs a metadata instance holds the KEK and
etcd's wrapped keys, so its data disks are not protected from it: in the lab, every node that
runs one (fl-a, fl-b, fl-w). XTS permits malicious modifications/replay. A node that was target
may retain the key; fencing cannot revoke that knowledge. Static KEK loss loses data;
key-source outage blocks new attach, not healthy existing I/O. HPKE and copied reads add
dependencies/CPU. Locked raw DEK alone is insufficient if schedules/transient copies escape;
the metadata service is not dumpable with a key source, but Go/protobuf/library lifetime
guarantees (GC copies) need a real audit. If confidentiality-only scope is unacceptable, do not
implement this plan as-is.
Settled at approval, 2026-11-13: Q1–Q12 as recommended; confidentiality-only scope accepted (see decisions.md, Settled at approval).

## Confirmation

Exact envelope/HPKE Go-Rust vectors and authorization races, frontend cipher/zero/copy tests,
secret-lifetime tests/review and later authorized encrypted/plain measurements in the design.
Tamper/replay tests must demonstrate the explicitly excluded data threats without reporting
false protection (the design's "Excluded threats" row). No global ADR number/accepted file
changes and no crypto-erase claim.
