# 0013. A certificate is rotated by replacing its files; every process watches its own

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

ARCHITECTURE §15.2 wants 24-hour certificates, rotated, issued by the operator's CA, with
cert-manager or SPIRE pluggable instead. Today every Flaminio process reads its certificate and
key once, at start ([issue #16](https://github.com/jsfakian/Flaminio/issues/16),
[intent](../issues/16/intent.md)). A restart is not a way to rotate: a restarted data plane
reports whole chunks as uncertain and resyncs (follow-ups §2, §9).

Several renewers will exist over time: the lab's script now, the operator's CA later,
cert-manager or SPIRE where a site has them. Processes run in pods and as host services; fl-c has
no kubelet. The interface between a renewer and a process has to be chosen once: every renewer
and every process is then built against it.

## Decision

- **The interface is the file system.** A renewer installs a renewed certificate and its key at
  the paths the process was started with, each file replaced whole (a rename, or a Secret
  volume's swap). It sends no signal and calls nothing. It does not know which processes read
  the files.
- **Each long-lived process watches its own pair.** It reads the two files every 15 seconds, off
  the I/O path, and swaps the pair in memory when it is usable. Each TLS handshake takes the
  pair in memory: a handshake reads no file.
- **A rotation changes the pair only.** The identity is fixed at start: a pair with another
  identity is refused. The CA bundles and the token key are read at start. Peer verification is
  never touched by a reload, and no failed reload falls back to a weaker mode.
- **An unusable pair is refused, logged once, and tried again;** the process keeps the pair it
  has and never exits for it ([spec](../issues/16/spec.md) S4, S5).
- **Open connections are left alone.** Expiry is checked at handshakes, by the peer.
- **No TLS session is resumed,** in Go or in Rust. Flaminio's servers keep no session and send
  no ticket (rustls `NoServerSessionStorage`, `send_tls13_tickets = 0`; Go
  `SessionTicketsDisabled`); its clients, its etcd client included, do not resume (rustls
  `Resumption::disabled()`; no Go `ClientSessionCache`). etcd's own servers keep Go's default
  tickets, but no etcd client resumes. A resumed handshake sends no certificate: it would carry
  its first handshake's pair past a renewal, and in rustls past its expiry, which rustls does
  not check on resumption. So every handshake presents and verifies the pairs in use then.

## Options

1. **Files, polled (chosen).** Works for pods and host services, for every renewer named in
   §15.2, in Go and in Rust, with no new dependency. Cost: up to 15 s before a new pair is used;
   a directory, not a file, must be mounted into a pod; the pair must be written so that a reader
   never sees half a file.
2. **Files, with inotify.** Immediate, but a watch is lost when a directory is replaced (Secret
   volumes swap a symlink), needs a platform-specific path in two languages, and still needs a
   poll as a fallback.
3. **A signal or a control-socket command.** The renewer must then know every process, reach
   into pods, and be privileged to signal them. cert-manager and SPIRE's file helpers do not.
4. **Each process requests its own certificate** (a CSR client, or the SPIFFE Workload API).
   Ties every process to one issuer and puts issuance logic in the data plane. etcd cannot do it.

## Consequences

- etcd already fits: it loads its files at each handshake.
- A renewer that writes a file in place, or one file long before the other, makes handshakes
  fail or pairs be refused until both are whole. It cannot make a handshake succeed that would
  have failed.
- Whoever can write a process's certificate files can replace its key, but not its identity
  without the CA. The files' directory must stay writable by root only.
- **Risk accepted:** a connection may outlive its peer's certificate (spec, open question 3).
  Write safety does not rest on certificates: epochs, fencing and capability tokens (§8, §15.2).
- **Risk accepted:** rotating a CA or the token key still needs a restart.
- Every reconnection is a full handshake, a signature each way more than a resumed one.
  Flaminio's connections are long-lived, so this is rare.
- The 15-second period bounds how short a life can be: a pair must be installed at least a
  minute before the old one expires.

## Confirmation

- Tests on loopback, in `pkg/pki` and `flaminio-net`: a renewal on each side, every unusable
  pair, an expired peer, and a reconnection after a renewal or an expiry, which is a full
  handshake ([design](../issues/16/design.md), Tests).
- Review: no code path reads a certificate file in a handshake or on the I/O path; no reload
  rebuilds the verifier or the CA pool.
- The lab check of issue #16: many rotations under load with no restart.
