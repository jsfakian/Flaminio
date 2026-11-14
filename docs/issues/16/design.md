# Issue #16: Short-lived certificates, rotated — design

How the [spec](spec.md)'s rules are built. Settled choices: [decisions](decisions.md).
Approved 2026-11-13; not implemented. Changed after approval in plan review round 1
(2026-11-14): see decisions.md.
Settled at approval, 2026-11-13: the spec's open questions 1 to 5, each with its recommendation (see decisions.md, Settled at approval).

The facts it starts from:
- **Go reads a pair once:** `Files.ServerConfig` and `ClientConfig` fill `tls.Config.Certificates`
  (`pkg/pki/pki.go:267-310`). Callers: the metadata service's listener and etcd client
  (`meta/cmd/flaminio-meta/main.go:116`, `:121`), the CSI driver's three roles through `DialMeta`
  (`control/csi/meta.go:32-38`), the client that runs and exits (`main.go:333`). The node agent
  opens no connection (no `tls` or `pki` under `control/agent`).
- **Rust reads a pair once:** `with_single_cert`, `with_client_auth_cert`
  (`flaminio-net/src/tls.rs:68-90`; `flaminio-io/src/node.rs:295`, `:298`), kept by the network
  thread (`flaminio-node/src/net.rs:109`, `:121`). The metadata client turns PEM strings
  (`node.rs:311-313`) into one tonic identity (`flaminio-metaclient/src/lib.rs:85-87`).
- **etcd** loads its files at each handshake (`client/pkg/v3/transport/listener.go:474`, `:493`;
  v3.7.2, the lab's, and v3.7.3 alike).
- **The lab:** `pki.sh csr` keeps an existing key, `install` writes in place
  (`scripts/guest/pki.sh:15`, `:22`). `issue()` signs with `--days`, an `Int`
  (`scripts/lab.sh:547-554`, `control/cmd/flaminio-ca/main.go:85`); `cmd_pki issue` lists the
  identities inline (`lab.sh:573-593`); `NotBefore` is 5 minutes back (`pki.go:124`).
- **Files that do not reach the process:** `LoadCredential` copies under `DynamicUser`
  (`scripts/guest/meta.sh:21-27`, `:36`); three `File` hostPaths (`deploy/csi/driver.yaml:168-173`);
  a Secret created only when missing (`lab.sh:1003`), mounted by both Deployments (`:62`, `:217`).
- **Session resumption:** rustls 0.23's defaults keep 256 server sessions, send two TLS 1.3
  tickets and resume on the client (`server/builder.rs:113`, `:123`; `client/builder.rs:172`). A
  resumed handshake sends no certificate, and the rustls server takes the stored chain without
  checking its expiry (`server/tls13.rs:371-375`). `flaminio-net/src/tls.rs:68-90` changes none
  of this. Go servers re-check the stored leaf's `NotAfter` (`handshake_server_tls13.go:370`); a
  Go client without a `ClientSessionCache` never resumes.
- **Metadata clients check the CA and the address only** (`pkg/pki/pki.go:300-310`;
  `flaminio-metaclient/src/lib.rs:85-92`), and the `io` certificate has the server usage and the
  node's address. #74 hands this follow-up to #16 (74/decisions.md, "Not closed here").
- **Other plans:** #12 moves `io` to `/etc/flaminio/io-pki`, with copies of `ca.crt` and
  `token.pub` (12/design.md, The PKI directory); #74 makes `io.key` 0640, group `flaminio-io`
  (74/design.md, The lab node).

## The reload rule (S1, S2, S4, S5, S7, S8)

One rule, built in Go and in Rust
([ADR 0013](../../adr/0013-certificates-rotate-by-file-replacement.md)).
- **At start,** as today: the pair must load and match, else the process does not start. No new
  check. The identity is the leaf's URI SANs, sorted. The CA bundle is read here, once.
- **Every 15 s** a watcher reads both files and takes the first case that applies:
  1. **same bytes** as the pair in use: nothing;
  2. **unreadable:** a file missing, unreadable or cut short, or the key not the certificate's;
  3. **another identity:** the leaf's URI SANs differ from the start's;
  4. **not valid:** now before `NotBefore`, or not before `NotAfter`;
  5. **usable:** swapped in as one value (certificate, chain, key).
- **A refusal (2–4)** keeps the pair in use and is evaluated at every read: a pair not valid yet
  is taken once it is. It is logged once, again only when the bytes or the reason change. Case 2
  is logged at the second read in a row that finds the same bytes: one file of each (S9) is
  never logged.
- **A handshake** takes the pair in memory: Go `GetCertificate`, `GetClientCertificate`; rustls
  `ResolvesServerCert`, `ResolvesClientCert`. Reconnections are handshakes (S2), always full
  ones: no session is resumed (S6). An expired pair stays in use: its handshakes fail at the peer
  (S5).
- **Logs** (Go `slog`, with `use`; Rust `eprintln!`, `flaminio-io: <message>: key=value ...`):
  `certificate picked up` (`identity`, `serial` in hex, `not_after`), at start and at each swap;
  `certificate refused` (`cert`, `reason`); `certificate expires soon`, a warning, when under a
  third of the life is left, then every twelfth of the life; `certificate expired`, once.

## Go (S1–S8, S15)

- **`pkg/pki/reload.go`:** `const ReloadInterval = 15 * time.Second`;
  `func (f Files) Watch(ctx context.Context, use string) (*Pair, error)` loads, logs, and starts
  the watcher, which ends with `ctx`; `(*Pair).ServerConfig()`, `ClientConfig()`: the
  configurations of `pki.go:290-309`, a callback in place of `Certificates`;
  `(*Pair).Reload(now time.Time)`: one read, the whole rule (the watcher passes the clock, tests
  any time). The pair is an `atomic.Pointer[tls.Certificate]`. `Files.ServerConfig` and
  `ClientConfig` stay, for commands that run and exit.
- **No resumption (S6):** both server configurations set `SessionTicketsDisabled`. No client
  sets a `ClientSessionCache`, so none resumes.
- **The `meta` identity (S15):** `func VerifyMeta(trustDomain string) func(tls.ConnectionState)
  error`, for `tls.Config.VerifyConnection`: `PeerIdentity(state, trustDomain)`, then a node's
  identity whose component is `meta`, else the error `<id> is not the metadata service`.
  `(*Pair).TrustDomain()` and `Files.TrustDomain()`: the trust domain of the start leaf's single
  URI SAN, or "" when it has none, so that every handshake fails (the service refuses such a
  caller anyway).
- **`flaminio-meta serve`:** the signal context moves above the TLS setup;
  `tlsFiles.Watch(ctx, "meta")`, `etcdFiles.Watch(ctx, "etcd-client")`. The etcd client keeps
  `ClientConfig()` alone: its server is etcd.
- **`flaminio-meta`'s client** (`main.go:333`): with `--cert`, `VerifyConnection =
  pki.VerifyMeta(files.TrustDomain())`.
- **`flaminio-csi`:** `DialMeta(ctx, endpoints, files)` calls `files.Watch(ctx, "csi")`, then
  `pair.ClientConfig()` with `VerifyConnection = pki.VerifyMeta(pair.TrustDomain())`: one path
  for the three roles.

## Rust (S1–S8, S15)

- **`flaminio-net/src/tls.rs`:** `Files::pair() -> Result<Arc<Pair>, TlsError>`;
  `Pair::server_config()`, `Pair::client_config(alpn: &[&[u8]])`;
  `Pair::reload(now: SystemTime) -> Vec<Event>`, with `Event::{PickedUp, Refused, ExpiresSoon,
  Expired}` carrying the log's fields; `Pair::watch(&Arc<Self>, every: Duration, log: impl
  Fn(Event) + Send + 'static)` spawns the thread `flaminio-certs`, which holds a `Weak`. The pair
  is a `RwLock<Arc<CertifiedKey>>`, taken in handshakes only; `CertifiedKey::from_der` checks the
  key; dates and serial by `x509-parser`. `Files::server_config`, `client_config` stay.
- **No resumption (S6),** in all four configurations: the servers set `session_storage =
  Arc::new(NoServerSessionStorage {})` and `send_tls13_tickets = 0`, the clients `resumption =
  Resumption::disabled()`.
- **`flaminio-io/src/node.rs`:** one `Pair` of `io.crt`, `io.key`: `server_config()`,
  `client_config(&[])` for replication, `client_config(&[b"h2"])` and `trust_domain:
  c.trust_domain` for the metadata client, one `watch`. `flaminio-node/src` does not change.
- **`flaminio-metaclient`:** `Config { endpoints, tls: Arc<ClientConfig>, trust_domain, timeout }`
  (rustls's, through `tokio_rustls`). Each endpoint keeps its `https://` URI, with
  `connect_with_connector_lazy`: the connector (a `tonic::codegen::Service<Uri>`) opens TCP,
  handshakes with `tokio-rustls` for the endpoint's host, refuses an ALPN other than `h2`, and
  requires the server leaf's identity to be a node's `meta` in `trust_domain`
  (`flaminio_net::tls::peer_identity`, then `node_with("meta")`, as `session::connect` checks
  `io`; S15), else fails with `<id> is not the metadata service`, and the call goes to the next
  endpoint. Then it returns a `TokioIo`. tonic's `tls-ring` goes: its identity cannot change,
  and with it tonic refuses an `https` URI behind a connector (tonic 0.14.9,
  `channel/service/connector.rs:87-93`). Direct dependencies added, all in `Cargo.lock`
  already: `tokio-rustls`, `hyper-util`, tokio's `net`, and the workspace's `flaminio-net`.

## Installing (S9, S13)

- **`pki.sh`** knows the names and files it writes and refuses any other (`unknown name`), so a
  name reaches a path or a root shell only from its list. Values reach a root shell as its
  positional parameters, never in its script text. Each name has one directory, with an owner
  and mode given to it only when it is created, and an owner and mode for a key with none to
  replace. An existing directory or key keeps its own (R2-B1): so #74's `/etc/flaminio/pki`
  0750, group `flaminio-io`, while `io` lives there, survives every `csr` and `install`, as does
  its `io-pki` once `io` has moved. A missing directory is built, under the lock, under a
  temporary name beside it, `DIR.RUN.new` (for `file`, which has no `RUN`, its pid instead):
  `mkdir -m 0700`, `chown`, then, for a group directory, `chmod 0750` (not `install -d`), and
  only then `mv -T` to its final name (R5-B1).
  So a directory is never visible half set: a crash leaves either no directory, which the next
  run builds, or the finished one, and a temporary directory left behind is removed like the
  `.new` files (`install`'s step 6). Those are the only commands that set a directory's owner
  and mode. Only the root itself is made before the lock, which lives in it:
  `sudo mkdir -p -m 0755 -- "$root"`, with `$root` the resolved root (below); `-p` makes it a
  no-op when it exists, or when a concurrent run made it first (R5-B2). Every command that
  touches the tree goes through `sudo`.

  Every path, the lock's included, is under `/etc/flaminio`. As #12 does, `PKI_ROOT` replaces
  it only under `FLAMINIO_SCRIPT_TEST=1`, the switch `common.sh` already honours for
  `CONTROL_SOCKET`; otherwise it is ignored. Chosen over refusing it for a root caller: one rule
  the scripts already follow, and the name #12 uses for the same override. The root reaches the
  root shell as a positional parameter, as the other values do, since `sudo` resets the
  environment. The list:
  - `meta`, `meta-etcd`: `/etc/flaminio/meta-pki`, `root:flaminio-meta-pki` 0750
    (`groupadd -f --system`); a first key `root:flaminio-meta-pki` 0640.
  - `agent`: `/etc/flaminio/agent-pki`, root 0700; a first key root 0600.
  - `io`: `/etc/flaminio/io-pki`, root 0700, a first key root 0600; once #74 has landed,
    `root:flaminio-io` 0750 and a first key `root:flaminio-io` 0640. `io-pki` is #12's
    directory: as #12 states, whichever of #12 and #16 lands second routes `io` there. Until #12
    has landed, `io` stays in `/etc/flaminio/pki`, where `node.sh` reads it.
  - `etcd`, `admin`, `csi-controller`: `/etc/flaminio/pki`, root 0700; a first key root 0600.
  - Files: `ca.crt` and `token.pub` to `/etc/flaminio/pki` and, once `io` is in `io-pki`, there
    too; `etcd-ca.crt` and `token.key` to `pki` only. Certificates 0644.

  The lock is one file for every name, `/etc/flaminio/.pki.lock`, outside every directory that
  is mounted into a pod, so `io-pki` holds exactly #12's four files after every `install` and
  `file`, as #12's test asserts (between a `csr io` and its `install`, the run's `.new` key sits
  there too). The `.new` files stay beside their targets, for the rename; a completed run leaves
  none. The commands:
  - **`csr NAME`** (with `RUN`, which must match `^[0-9a-z]{1,32}$`): holding `flock` on the
    lock, makes the name's directory if missing, and always a new key, `NAME.key.RUN.new`
    (`umask 077`), in it; prints the CSR.
  - **`install NAME`** (with `RUN`, `CERT`): one root shell, `sudo flock /etc/flaminio/.pki.lock
    sh -c`, that ignores SIGHUP and SIGPIPE, so that an ssh drop cannot stop it between two steps:
    1. makes the name's directory if missing; writes `NAME.crt.RUN.new`. The two public keys
       must be equal (this run's key missing counts as unequal): else this run's two files go,
       `refused: certificate and key do not match`, exit 1;
    2. gives the new key the owner, group and mode of the key it replaces, read with
       `stat -c '%u %g %a'` (`stat -f '%u %g %Lp'` where that fails: the script tests run on
       macOS too) and applied with `chown` and `chmod`; or the list's for a first key. So
       `io.key` keeps #74's 0640, group `flaminio-io`, in either landing order;
    3. `sync` the two `.new` files, then `mv -f` the key, then the certificate;
    4. for a name that moved (`meta`, `meta-etcd`, `agent`; `io` once in `io-pki`): `rm -f
       /etc/flaminio/pki/NAME.crt /etc/flaminio/pki/NAME.key`, by name, never a glob, which the
       VM user's shell would expand in a directory it cannot list;
    5. `sync -f` the name's directory, and `/etc/flaminio/pki` when step 4 removed files there
       (R2-S1: a first key in a new directory replaces nothing, so no rename-over flush covers
       it);
    6. removes what a run that died left behind, over an hour old:
       - this name's `.new` files, left between a `csr` and its `install` (until then such a
         key, root 0600, sits beside the pair, in `io-pki` too):

         ```sh
         find "$dir" -maxdepth 1 -type f -name "$name.*.new" -mmin +60 -delete ||
             echo "pki.sh: $dir: an old .new file left in place" >&2
         ```

       - temporary directories, left while one was built. They sit beside the directories, so
         the scan is of the root, anchored to the known directory names and limited to
         directories (R6-1):

         ```sh
         find "$root" -maxdepth 1 -type d \( -name 'pki.[0-9a-z]*.new' \
             -o -name 'meta-pki.[0-9a-z]*.new' -o -name 'agent-pki.[0-9a-z]*.new' \
             -o -name 'io-pki.[0-9a-z]*.new' \) -mmin +60 -exec rmdir {} + ||
             echo "pki.sh: $root: an old temporary directory left in place" >&2
         ```

         `rmdir`, not a recursive delete: the directory is empty by construction, and an
         `rmdir` that refuses one that is not leaves it. Each `find` ends in `|| echo … >&2`,
         so a refused removal is logged and the install still succeeds (R7-2).
  - **`file NAME`:** holding the lock, as it may make a directory: for each destination, the
    directory if missing, a temporary file beside it, `sync`, `mv -f`, then `sync -f` the
    directory (a first `ca.crt` or `token.pub` in a new directory replaces nothing, as in
    `install`'s step 5).
  - **`due NAME...`:** a line each, `NAME due|fresh|absent`. `absent`: neither file, in either
    layout. `due`: for a name that moved (as in `install`'s step 4), a pair still only in
    `/etc/flaminio/pki`, not migrated; not readable; not matching; or past `NotBefore` plus half
    the life (the VM's clock). With `FORCE=1`, every pair that is there. It writes nothing and
    takes no lock: a pair caught between `install`'s renames reads as not matching, so at worst
    it is renewed once more.
- **`meta.sh up`:** `LoadCredential` for `ca.crt`, `etcd-ca.crt`, `token.key` only;
  `SupplementaryGroups=flaminio-meta-pki`; the pairs' flags point into `/etc/flaminio/meta-pki`.
  `DynamicUser` and the sandbox stay. Without `meta.crt` there it stops, naming `make pki-issue`.
- **The node plugin:** one `Directory` hostPath `/etc/flaminio/agent-pki`, read-only, same path
  inside; `ca.crt` stays a `File`. `scripts/guest/confirm.sh:31` and `lab.sh:999` follow.
- **The Secret:** `csi_controller_secret`, for `csi deploy` and the renewal: `kubectl create
  secret generic … --dry-run=client -o yaml | kubectl apply --server-side --force-conflicts -f -`
  on fl-w, one update. Server-side, because a client-side apply copies the whole object, key
  included, into the `last-applied-configuration` annotation, which `kubectl describe` prints.
- **`make pki-issue`** uses the same `csr` and `install`: a new key each time, no restart after.

## The lab's renewal (S10–S12)

- **`identities VM`** (`lab.sh`): a line per identity, `NAME CA ID FLAGS...`: `etcd`, `meta-etcd`,
  `meta`, `io`, `agent` (not fl-w), `admin` (fl-a, fl-w), `csi-controller` (fl-w). `pki issue`
  reads it (`csi-controller` stays `csi deploy`'s); so does the renewal. Where each lands is
  `pki.sh`'s list: `io` in `io-pki` once #12 has landed.
- **`issue()`** gives each identity a new `RUN`, the time in seconds and the shell's pid
  concatenated with no separator (a `-` or `.` would fail `RUN`'s pattern), for its `csr` and
  `install`.
- **`lab.sh pki renew [VM...]`** (`make pki-renew`), default `vms_of overlay`. Per VM: one
  `pki.sh due` of `identities VM`'s names. A name goes through `issue` only from a line that is
  exactly `NAME due`, with `NAME` among those names; any other line is ignored. On fl-w, when
  the Secret exists and its `tls.crt` is not the file's: `csi_controller_secret` (the demo's is
  left, and said). Nothing else.
- **Failures:** a VM whose ssh ends 255 is skipped, a failed identity too. The pass ends with
  `VM: unreachable; not renewed: NAMES` or `VM: failed: NAMES`; exit 255 when only unreachable
  VMs failed, else 1.
- **The schedule:** `EVERY=<duration>` (`make pki-renew EVERY=1h`) repeats the pass on the
  workstation until interrupted; a failed pass is logged and the next runs.
- **Lives:** `flaminio-ca sign --life <Go duration>`, at least 10 minutes, not with `--days`;
  default 30 days. Shorter is refused: `NotBefore` is 5 minutes back, so a life L spans L + 5
  minutes, and with L of 5 minutes or less the half-life rule finds a pair due as soon as it is
  installed. `lab.conf`: `CERT_LIFE` (empty): `issue()` passes `--life` when set, else `--days`.

## Files touched

- `pkg/pki/reload.go`, `reload_test.go` (new); `meta/cmd/flaminio-meta/main.go`, `main_test.go`,
  `control/csi/meta.go`, `control/cmd/flaminio-csi/main.go`; `meta/service/auth_test.go`,
  `meta/kv/etcd_tls_test.go` (new), `control/csi/csi_test.go`: S1–S8, S15.
- `control/cmd/flaminio-ca/main.go`, `main_test.go` (new): S11.
- `dataplane/crates/flaminio-net/src/tls.rs`, `src/identity.rs`, `tests/rotation.rs` (new);
  `flaminio-io/src/node.rs`; `flaminio-metaclient/src/lib.rs`, `Cargo.toml`, `tests/`;
  `flaminio-node/tests/engine.rs`: S1–S8, S15.
- `scripts/guest/pki.sh`, `meta.sh`, `confirm.sh`; `deploy/csi/driver.yaml`: S9.
- `scripts/lab.sh`, `scripts/lab.conf`, `Makefile`: S10, S11, S13.
- `scripts/tools/pki-renew-test.sh` (new), `node-scripts-test.sh`: S9–S11;
  `scripts/checks/rotation.sh` (new): the lab checks.
- `docs/ARCHITECTURE.md`, `docs/lab.md`, `docs/followups.md`: S14.
  `docs/adr/0013-certificates-rotate-by-file-replacement.md` and its row in `docs/adr/README.md`
  are in the plan already, Proposed.

## Contracts

No proto, RPC, wire frame or on-disk record changes. What the units share:
- **Files** (ADR 0013): paths fixed at start; a file replaced by a rename; read every 15 s; the
  five cases and the four log messages above, word for word.
- **TLS:** no session resumed, by servers or clients, in Go and in Rust (ADR 0013). A metadata
  client requires the server's `meta` identity (S15).
- **Paths:** `/etc/flaminio/meta-pki/{meta,meta-etcd}.{crt,key}`,
  `/etc/flaminio/agent-pki/agent.{crt,key}`, `/etc/flaminio/io-pki/{io.crt,io.key,ca.crt,token.pub}`
  (#12's; until #12 has landed, `io` stays in `/etc/flaminio/pki`); the rest in
  `/etc/flaminio/pki`. Owners and modes as in Installing, given only to what `pki.sh` creates;
  an existing directory keeps its own, and a renewed key those of the key it replaces. The lock
  is `/etc/flaminio/.pki.lock`.
- **CLI:** `flaminio-ca sign --life` (10 minutes or more); `pki.sh csr|install|file|due`, `csr`
  and `install` with `RUN`, `PKI_ROOT` only under `FLAMINIO_SCRIPT_TEST=1`;
  `lab.sh pki renew [VM...]` with `EVERY`, `FORCE`, `CERT_LIFE`; exits 0, 1, 255.
- **Go:** `pki.Files.Watch`, `pki.Pair`, `pki.VerifyMeta`, `(*Pair).TrustDomain`,
  `Files.TrustDomain`, `csi.DialMeta(ctx, …)`. **Rust:** `tls::Pair`, `tls::Event`,
  `flaminio_metaclient::Config.{tls, trust_domain}`.

## Tests

- **S1, S2, S4–S7, S15, Go** (`make test race`), real mutual TLS on loopback, `Reload` called by
  hand:
  - `pkg/pki/reload_test.go`: `TestRenewalBothSides` (the next handshake shows the new serial);
    `TestUnusablePairKeepsTheOld` (mismatch, each file missing, cut short, not valid yet, expired,
    another identity: the old serial; then a usable one is taken); `TestExpiredPeerRefused` (both
    ways; accepted once renewed, same listener and client); `TestChecksUnchanged` (TLS 1.2, no
    client certificate, another CA: refused after a reload and after a refused one);
    `TestStartWithoutPair`; `TestWatchPicksUp` (goleak); `TestNoResumption`, each side against a
    peer that resumes: a client with a `ClientSessionCache` connects twice to
    `(*Pair).ServerConfig()`, and `(*Pair).ClientConfig()` connects twice to a server with Go's
    default tickets; neither second handshake is resumed (`DidResume` false). The server half
    fails if `SessionTicketsDisabled` goes; the client half guards against a `ClientSessionCache`
    being added to `(*Pair).ClientConfig()`, the only way a Go client resumes; `TestVerifyMeta`
    (S15: a `meta` server answered; an `io` one with the server usage at 127.0.0.1, another
    trust domain's `meta`, two URI SANs: refused).
  - `meta/service/auth_test.go` `TestAuthorizationAcrossRenewal`: the caller rules across a
    renewal of the server, then of the caller; an expired caller: `Unavailable`, then answered
    again on the same `ClientConn`.
  - `meta/kv/etcd_tls_test.go` (new) `TestEtcdClientAcrossRenewal`: embedded etcd with
    `ClientCertAuth`. The renewed client pair is signed by a second CA, and the member,
    restarted, trusts that CA alone for clients: the next call succeeds only if it presents the
    new pair. (A second CA, not an old pair left to expire: no wait on the clock.)
    `control/csi/csi_test.go` `TestDialMetaAcrossRenewal`: the same through `DialMeta`;
    `TestDialMetaChecksMeta` (S15): a server with an `io` certificate at 127.0.0.1, the call
    fails with `<id> is not the metadata service`; with a `meta` one, answered.
  - `meta/cmd/flaminio-meta/main_test.go` `TestClientChecksMeta` (S15): a new
    `newFixtureAs(t, serverID string)` builds the fixture with that server identity, and
    `newFixture(t)` becomes `newFixtureAs(t, pki.NodeID(td, "fl-w", "meta"))`, so the other tests
    are unchanged; `writeIdentity` already signs for 127.0.0.1. With
    `newFixtureAs(t, pki.NodeID(td, "fl-w", "io"))` (server usage), `f.run(admin, "nodes")` fails
    and its error contains `spiffe://<td>/flaminio/node/fl-w/io is not the metadata service`;
    with `newFixture`, answered.
- **S1, S2, S4–S7, S15, Rust** (`make test`; `make contracts`: the client against the Go service):
  - `flaminio-net/tests/rotation.rs`: the `pkg/pki` cases with `rcgen`, through
    `session::accept` and `connect`; the token check still refuses after a reload. Resumption,
    each side on its own, against a peer that resumes: `Pair::server_config()` against a rustls
    default `ClientConfig`, and `Pair::client_config()` against a default `ServerConfig`, which
    sends tickets. `reconnect_after_renewal_is_full`: the side under test is renewed; the
    reconnection's `handshake_kind()` is `Full`, and the peer sees the new serial.
    `reconnect_after_expiry_refused`: the peer's pair is valid for 2 s; after its `NotAfter` the
    reconnection is refused, with the texts the lab's Expiry check matches (rustls 0.23.46).
    The verifying side's error contains `invalid peer certificate: certificate expired`, the
    other side's `received fatal alert: CertificateExpired`. In the half with the server under
    test, the expired pair is the connecting peer's: TLS 1.3's `connect` returns Ok and the alert
    comes on the first read. The peer is a bare `TlsConnector` that sends nothing after the
    handshake and reads (R5-B3), which removes the known cause of a reset replacing the alert: a
    hello written after the server closed. A second cause, the server closing with part of the
    client's flight unread, is removed too (R6-2): the certificates are one small leaf each, no
    chain, and the test keeps a `try_clone` of the server's socket, so `accept`'s drop does not
    close it; once `accept` has failed, the test shuts down that socket's writing and reads it to
    the end before dropping it. So both texts stay asserted. The drain (R7-1): the connector runs
    in its own task (`smol::spawn`), so its read and its drop, which ends the drain, do not wait
    on the server half; the test awaits the server half first (`accept`'s error, then the
    drain), then the connector's task. The clone shares the runtime's `O_NONBLOCK`, so it is
    read through the runtime (`smol::Async`), and the drain has a 5 s timeout. The drain only
    delays the close: end of file, any read error (`connection reset` included) or the timeout
    ends it, and none of them is asserted (R8-1); the assertions stay on `accept`'s error and the
    connector's error text. Chosen over tolerating `connection
    reset`, which would leave the connector's text unchecked; the server under test still runs
    `session::accept`.
    In the half with the client under test, the expired pair is the acceptor's: `connect` itself
    fails with the first text and `accept` with the second. Each fails if the side under test
    keeps rustls's default resumption.
  - `flaminio-node/tests/engine.rs` `a_session_outlives_a_renewal`: an open session goes on
    writing across `reload`; the next session shows the new serial, each way.
  - `flaminio-metaclient/tests/rotation.rs` (new): a loopback rustls server records the
    client's serial and keeps rustls's default tickets, so that only the client's own setting
    is tested; two calls around a renewal, with the server restarted between them so that its
    connections close and the second call reconnects: the new serial, in a full handshake.
    `refuses_a_server_that_is_not_meta` (S15): a server with an `io` certificate with the server
    usage at 127.0.0.1, the call fails and the server receives no HTTP/2 preface; with a `meta`
    one, the preface arrives. `tests/interop.rs` builds the new `Config`, `trust_domain: "lab"`
    (its server is `spiffe://lab/flaminio/node/m/meta`, `scripts/dev/meta-interop.sh:20`).
- **S3:** in these tests no connection is accepted or closed at a renewal (the metaclient
  test's server restart is a step of its own). By review: the callbacks run in handshakes only.
  `flaminio-repl` gets no rule: `make sim` unchanged.
- **S8:** `TestRotationLogs` (Go, `slog` captured) and its Rust twin (the events): one line per
  pick-up; one per refused pair over many reads, another when the reason changes; one file of
  each, then whole: none; the warning at a third and a twelfth later; `expired` once.
- **S9–S11, scripts** (`scripts/tools/pki-renew-test.sh`, new, in `make lint`; bash 4.4 guard):
  - `pki.sh`, run as a non-root user with `FLAMINIO_SCRIPT_TEST=1` and `PKI_ROOT` set to a
    temporary directory, with real `openssl`. `sudo` runs its command, but refuses without
    running it any command with an argument that contains `/etc/flaminio`, so a fallback to the
    real path fails the case, as in #12's tests. Stubs, first on `PATH`, record their arguments and
    whether they ran under the lock: `flock` runs its command with a marker in its environment;
    `chown`, `chgrp`, `groupadd` and `sync` only record; `chmod`, `mv`, `rm`, `find`, `stat`,
    `mkdir` and `openssl` record, then run the real one. So no root and no `flaminio-*` group
    are needed. The `mv` wrapper runs `-T` as given with GNU `mv`; BSD `mv` lacks it, so there
    it checks that the destination is absent, which is what `-T` guards here, and drops it.
    Cases:
    - `csr` twice: two keys. A certificate for another key: refused, the old pair intact. Each
      final path gets a new inode.
    - The lock: `csr`'s directory and key, every step of `install` (the directory, the key
      check by `openssl`, the read by `stat`, `chown`, `chmod`, `sync`, the renames by `mv`, the
      removals by `rm` and `find`) and every step of `file` (the directory, `sync`, the rename)
      run under one `flock` of `$PKI_ROOT/.pki.lock`, so the case fails if the lock goes or a
      step runs outside it. No `.lock` in any directory; `io-pki` holds exactly its four files
      after `install io` and after `file`.
    - A directory made by `pki.sh`: `mkdir -m 0700` of `DIR.RUN.new`, `chown`, `chmod 0750` for
      a group directory, then `mv -T` to `DIR`, in that order in the record; the root made by
      `sudo mkdir -p -m 0755`, before the lock.
    - A crash while building one (`meta-pki`): the `chown` stub kills the root shell after
      recording the temporary directory's `chown`. No directory at the final path; the next run
      builds it whole, with its final mode; a temporary directory aged over an hour
      (`touch -t`) is removed by the next `install`, while that same `install` keeps a fresh
      temporary directory, every final directory, and an aged `.new` file of another name. An
      aged temporary directory that holds a file stays: the `install` succeeds and logs
      `an old temporary directory left in place`.
    - `PKI_ROOT` without the switch: with `FLAMINIO_SCRIPT_TEST` unset, `pki.sh csr io`,
      `install io` and `file ca.crt` write nothing in the temporary directory, the `sudo` stub's
      refusal of the real path fails each, and no `mkdir` is recorded. Every such command goes
      through `sudo`, so the refusal also keeps a test run as root (a CI container) off the
      real `/etc/flaminio`.
    - Modes, read for real: `meta.key` 0640; an `io.key` found 0640 stays 0640; `agent.key`
      0600; certificates 0644. Owners and groups, from the `chown` stub: the found key's
      numeric owner and group, or the list's names for a first key (`root:flaminio-meta-pki`).
      The found key is read by the host's `stat` (GNU on Linux, BSD on macOS).
    - Both landing orders of #74 and #12: `io`'s directory (`pki` before #12, `io-pki` after)
      existing at 0750 keeps its mode through `csr io` and `install io`, and so does `pki` at
      0750 through `install etcd`; no `chown`, `chgrp` or `chmod` is recorded for either.
    - `sync` of the two `.new` files before the first rename; `sync -f` of the directories after
      the last removal; for `file`, `sync` before its rename and `sync -f` of its directory
      after.
    - `file ca.crt` and `token.pub` in `pki` and `io-pki`; `file token.key` never in `io-pki`.
    - An unknown name or file, and a bad `RUN`: refused, nothing written.
    - Two runs interleaved (`csr` A, `csr` B, `install` A, `install` B, and with the installs
      swapped): a matching pair each time. A SIGHUP sent during `install` by the `mv` wrapper
      after the key's rename: the certificate's rename still happens. A `.new` over an hour old
      removed, a fresh one kept.
    - `due` for fresh, half-life, mismatch, absent, a moved name's pair still only in
      `/etc/flaminio/pki` (`due`), an `etcd` pair there (`fresh`), `FORCE`.
    - The old layout's files removed, with the faked `/etc/flaminio/pki` made unlistable (0300)
      so that a glob would match nothing.
  - `meta.sh up`: the unit's text. `driver.yaml`: no `File` hostPath but `ca.crt`.
    `node-scripts-test.sh`: `confirm.sh`'s new `--cert` and `--key`.
  - `cmd_pki`, `identities`, `issue`, extracted as `lab-deletion-test.sh` does, with fake
    `run_on_vm`, `vssh`, `ca_tool`, `sleep`: only `due` names issued; `--life` when `CERT_LIFE`
    is set; fl-c unreachable: the others renewed, exit 255, the message; a failed signature:
    1; a `due` line for a name not in `identities VM`, or not exactly `NAME due`: ignored; the
    Secret applied with `--server-side --force-conflicts`, only when it differs; no call has
    `restart`, `rollout`, `delete` or `systemctl`; `EVERY`: a failed pass, then another.
- **S11** (`control/cmd/flaminio-ca/main_test.go`): `--life 30m`, `--life 10m`, `--life 9m`,
  `--days 2`, neither, both, `--life 0`, `--life x`: `NotAfter`, or the refusal.
- **S12, S13:** no test above uses Kubernetes; `make contracts`; the unchanged tests.
  **S14:** by review. Not concerned: `crash-test`, `miri`, `sanitize`.

## Lab

On request only: `scripts/checks/rotation.sh`, through `make rotation-test`.
- **Before, once,** the new layout and the new binaries, and the last restarts: `make pki-issue`
  and `make pki-issue VM=c` (`pki-issue` covers fl-a, fl-b and fl-w only); `make deploy` and
  `make deploy VM=c`; `make node-up`, and fl-c's node stopped (`make node-down VM=c`) for the
  check to start it with `ensure_node_c`, on its claimed disk; `make meta-up`; `make csi-deploy`.
- **Many rotations:** `HOURS=6 CERT_LIFE=30m EVERY=2m`. It issues at 30 minutes, starts the
  renewal and a pod writing verified data on a PVC, and runs `node-fault`, `partition-test`,
  `csi-test`, `meta-smoke`, an hour apart. Pass: ten `certificate picked up` or more per Flaminio
  process (etcd logs none: the `openssl s_client` check on 2379 and 2380 covers it); over each
  quiet hour no unit's `ActiveEnterTimestamp`, no pod's restart count or UID changes; at the end
  `openssl s_client` on every listener (7400, 2379, 2380, the node's) and each process's last
  pick-up show a `NotBefore` under 30 minutes old; no I/O error.
- **Expiry:** `EXPIRE=c`: the renewal leaves fl-c out until its pairs expire. Its sessions go on
  past the expiry. Then `partition.sh on` on fl-c for `HOLD` (30 s by default), and `off`. What
  the check reads:
  - **replication:** the lines that carry the TLS error, with the rustls expiry text in them:
    `flaminio-node: <volume>: session to <node> lost: <why>` (`engine.rs:828-831`, `<why>` from
    `net.rs:294`) on the connecting side, and `flaminio-net: session from <peer>: <e>`
    (`net.rs:196`) on the accepting side, for a session to or from fl-c. The texts swap with the
    direction: the side that verifies the expired certificate says
    `invalid peer certificate: certificate expired`, the other side
    `received fatal alert: CertificateExpired`, and an expired connector sees it only at its
    first read. So the check matches either text in either line, on any host. Not
    `session to fl-c failed: a request timed out` (`engine.rs:1772`), which the partition alone
    produces;
  - **metadata:** a new connection from fl-c, `metactl` on fl-c with `AS=io` (its expired `io`
    pair), is refused, naming the expiry (Go's server sends `expired certificate`,
    `crypto/tls/handshake_server.go:987`). fl-c's open metadata connection is not what it reads:
    with no HTTP/2 keepalive it can outlive a 30 s partition, and the service logs no failed
    handshake at grpc's default level.

  One `pki renew c` later fl-c is in every ISR, its node not restarted.
- **The week:** `HOURS=168 CERT_LIFE=24h EVERY=1h`.
- **Deletes:** its PVC and pod; `node-fault` restarts fl-b's node, whose replicas reconcile. No
  disk is formatted. **Restore:** the loop stopped; `FORCE=1 make pki-renew` with `CERT_LIFE`
  unset: 30 days again, no restart.

## Docs

- **`docs/lab.md`:** the layout (`io-pki` once #12 has landed) and its one-time order, fl-c
  included; `make pki-renew`, `EVERY`, `CERT_LIFE` (10 minutes or more); the log lines; an
  expired certificate: renew, no restart; `make rotation-test`.
- **`docs/followups.md` §7:** step 2 done, with the check's result. The settled answers to questions
  1 and 2 (decisions.md). Step 1 open. #43's issue reads: "2. CA bootstrap: the operator holds the
  internal CA. Issuing on CSR is #16." So #43's step 2 bootstraps the CA, and issuing on CSR, with
  its TokenReview and question 2's answer, stays this issue's step 1, built once that CA exists; §7
  links #43 for the CA. No new issue is filed. #74's follow-up "metadata clients check the `meta`
  URI SAN" is closed here (S15): whichever of #16 and #74 lands second leaves it out.
- **`docs/ARCHITECTURE.md` §15.2:** a renewer replaces files, and every process picks them up,
  with no TLS session resumed; a metadata client checks the server's `meta` identity. Question
  2's answer: an administrator issues a node's first pairs by hand; the node renews by presenting
  its current, valid certificate; a node down for longer than a life is issued by hand again;
  the CA stops renewing a removed node.
- **Makefile help:** `pki-renew`, `rotation-test`. `README.md`, the performance plan: nothing.

## Risks

- **Open questions, designed on the recommended answer of each:**
  1. **(a):** with (b), an issuing service, its RPC, its authorization and its client are added.
  2. **(a):** nothing here depends on it.
  3. **Not closed at expiry:** closing adds a rule in `flaminio-repl`, a simulator check and an
     injected bug; S3 and S5 change.
  4. **The workstation:** elsewhere, only where `pki renew` runs changes.
  5. **30 days:** a 24-hour default makes the loop a standing requirement of the lab.
  Settled at approval, 2026-11-13: 1 to 5 as designed above (see decisions.md, Settled at approval).
- **Findings against the spec:**
  - **S10 "never leaving it", "the old key is gone":** the demo's `csi deploy` copies the
    controller's key to the workstation (`lab.sh:1005-1013`): the renewal leaves the demo's
    Secret alone. k3s keeps a Secret's old revisions until it compacts.
  - **S9 "refuses them and reads again":** etcd and the commands that run and exit load at each
    use (`listener.go:474`, `main.go:333`): between the two renames one handshake or one
    command fails, and the next succeeds.
  - **The lab's "no service's start time changes":** `node-fault` restarts a node and
    `meta-smoke` stops etcd members. The check compares over the quiet hours between them.
  - **S1 "within a minute":** counted from the file's change in the pod; the kubelet projects a
    changed Secret up to about a minute after the renewal.
  - **S4 against S7:** an expired pair at start has no pair to keep: it starts, as today, and
    logs `certificate expired`.
- **Security:** only root writes the four directories (`pki`, `meta-pki`, `agent-pki`, and
  `io-pki` once #12 has landed). Whoever can, replaces a key, but cannot change an identity
  (case 3) or the verification (built once). The metadata service's keys become readable by a
  group without members, not by root only; `io.key` by `flaminio-io` under #74, as #74 sets it.
  A certificate of another CA with the right identity is taken, and fails at peers. An `io` key
  no longer answers as the metadata service (S15).
- **#12, #74:** `io` goes to #12's `io-pki` by #12's rule (whichever lands second). A renewed key
  keeps the owner, group and mode it finds, and an existing directory keeps its own, so #74's
  `pki` 0750 and `io.key` 0640, group `flaminio-io`, hold across renewals in either landing
  order. What `pki.sh` creates first (`io`'s directory and key once #74 has landed) takes the
  list's defaults, which whichever of #16 and #74 lands second sets. `io-pki` holds #12's four
  files after every `install` and `file`. Between a `csr io` and its `install` the run's `.new`
  key (root 0600) sits there too, and a run that dies between them leaves it until a later
  install removes it (step 6); a pod that mounts `io-pki` sees it, unreadable under #74's group.
  #74's follow-up is S15.
- **Not compiled here:** the tonic connector (`make contracts` proves it; rebuilding channels
  instead would close a connection at each renewal, against S3), and `SupplementaryGroups`
  under `DynamicUser` (the lab proves it; else a static user owns the directory).
- **Migration:** between `make pki-issue` and `meta-up`, `csi-deploy`, a restart of the old unit
  or pod fails on its missing file. A pair left in the old layout is `due`, so the renewal
  migrates it rather than skipping it.
- **Two renewals at once, or one cut off:** each run has its own `.new` files and `install`
  holds the lock, so the last install wins, with a matching pair. An ssh drop cannot split the
  two renames (one root shell, SIGHUP ignored). The `.new` files are synced before the renames
  and the directories after them and the removals, so a power cut cannot leave an empty file
  in place, a first key included. A power cut between the renames leaves a new key beside the
  old certificate: processes refuse it (case 2) and keep their pair, but etcd's
  handshakes and any restart (S7) fail until the next pass, whose `due` finds the pair not
  matching and renews it.
- **ADR 0013's open choices:** a poll, not inotify; 15 s; the identity as URI SANs.
  Settled at approval, 2026-11-13: a poll, not inotify; 15 s; the identity as URI SANs (see decisions.md, Settled at approval).
- **Accepted:** up to 15 s before a pair is used; a connection may outlive its certificate;
  the workstation stays on; a CA or the token key still needs a restart.

## ADRs

- **[0013](../../adr/0013-certificates-rotate-by-file-replacement.md), Proposed here:** the file
  contract, and no TLS session resumed. Its index row says Proposed. Approving this plan does
  not accept it.
  Settled at approval, 2026-11-13: the file contract as ADR 0013 states it; ADR 0013 stays Proposed in docs/adr/ until accepted in its own step (see decisions.md, Settled at approval).
- **0001, 0002:** consistent (no shared code; etcd untouched). **0003–0006:** not touched.

## Parallelizable?

Yes. Three units against the contracts, with no back-and-forth: Go (`pkg/pki`, the binaries,
`flaminio-ca`), Rust (`flaminio-net`, `flaminio-metaclient`, `flaminio-io`), and the scripts and
manifests, which need only `--life` and the paths. `make contracts` joins Rust and Go.
