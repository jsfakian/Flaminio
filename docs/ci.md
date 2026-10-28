# Continuous integration

Every check runs on self-hosted runners on centaur, a Linux server on the lab's network, so CI
uses none of the private repository's GitHub Actions minutes. Every job is a `make` target, so a
failure reproduces locally with the same command.

## What runs when

| Workflow | When | Jobs (each one `make` target) |
|---|---|---|
| [CI](../.github/workflows/ci.yml) | every pull request and push to `main` | `lint`; Rust tests (`cargo test --locked`); `race`; `hygiene`; `contracts`; `sim` |
| [Nightly](../.github/workflows/nightly.yml) | 01:00 UTC, and by hand | simulation of 200 000 seeds starting from a new one each day; `crash-test`; `race COUNT=20`; `sanitize`; `miri`; `hygiene` |
| [Fuzz](../.github/workflows/fuzz.yml) | Saturdays 02:00 UTC, and by hand | `fuzz`, 10 minutes per target; the corpus carries over between runs; crashing inputs are uploaded as an artifact |
| [Lab](../.github/workflows/lab.yml) | by hand only | one lab check on the lab VMs, from the lab runner |
| [Dependabot](../.github/dependabot.yml) | weekly | one pull request each for Cargo, Go modules and GitHub Actions |

What the targets check:

- **`lint`:** clippy with warnings as errors, rustfmt, `go vet`, gofmt, `buf lint`, shellcheck on
  every script, and actionlint on the workflows (runner labels in `.github/actionlint.yaml`).
- **`hygiene`:** cargo-deny (known vulnerabilities, unmaintained crates, licenses, duplicate
  versions, crate sources; policy in [`dataplane/deny.toml`](../dataplane/deny.toml)), unused
  crates (cargo-machete), rustdoc with warnings as errors, staticcheck, govulncheck, and that
  `go.mod` and `go.sum` are tidy and verified.
- **`race`:** every Go test under the race detector, in random order. Every Go test package checks
  at its end that no goroutine its tests started is still running (`goleak`, in its
  `main_test.go`).
- **`contracts`:** the generated protobuf code and the metadata transition vectors match their
  sources; the fuzz crate's lockfile is current; the data plane's Rust metadata client works
  against the Go service over mutual TLS.
- **`sim`:** the replication-protocol simulator, 20 000 random runs, then every injected bug
  must be caught.
- **`sanitize`** (nightly toolchain): the tests of the crates with threads, io_uring or unsafe code
  under AddressSanitizer, which also reports memory leaks, then ThreadSanitizer.
- **`miri`** (nightly toolchain): the unsafe code that runs without system calls, the aligned
  buffers of `flaminio-uring`, under Miri with strict provenance.
- **Store space:** `chunk_churn_leaves_no_space_behind` (in `cargo test`) creates, writes and
  deletes chunks round after round and requires every data cluster and metadata page back.

The lab VMs are not needed: centaur builds and tests the Linux-only code (io_uring) itself.

## Runners

| Runner | Label | Runs |
|---|---|---|
| `centaur-flaminio-1` … `-4` | `flaminio` | CI, Nightly, Fuzz |
| `centaur-flaminio-lab` | `flaminio-lab` | Lab, one check at a time |

Each runner is a systemd user service, `actions-runner-<name>`, in `~/actions-runners/<name>`,
running as `jsfakian`. Builds persist between runs in `~/.cache/flaminio-ci/<runner>/target`
(outside the workspace, which checkout cleans), and the fuzz corpus in
`~/.cache/flaminio-ci/fuzz-corpus`. Deleting `~/.cache/flaminio-ci` is safe; the next runs build
from scratch.

**Setting up a host, or adding a runner,** from a checkout on the host:

```sh
scripts/ci/runner-host.sh tools                      # packages, Rust toolchains, cargo tools
scripts/ci/runner-host.sh runner flaminio-5 flaminio # one more CI runner
scripts/ci/runner-host.sh runner flaminio-lab flaminio-lab
```

`gh` on the host must be logged in with admin rights on the repository (or set `RUNNER_TOKEN`).
To remove a runner: `systemctl --user disable --now actions-runner-<name>`, then
`./config.sh remove` in its directory with a removal token
(`gh api -X POST repos/jsfakian/Flaminio/actions/runners/remove-token --jq .token`).

**Pinned versions:** the Rust toolchain in `dataplane/rust-toolchain.toml` (rustup installs it
everywhere on first use), the Go toolchain in `go.mod`, and the check tools in the `Makefile` and
in `runner-host.sh`. After changing a pin, rerun `runner-host.sh tools` on centaur.

**Security.** The runners execute the repository's code as `jsfakian`, who can reach the lab and
holds its keys. That is acceptable only while the repository stays private and only trusted
people can push to it. Before making it public, remove the self-hosted runners: on a public
repository, a pull request from a fork would run its code here.

## The lab workflow

Actions → Lab → Run workflow, then pick a check:

| Check | What it does |
|---|---|
| `status` | VM, node and `vol1` state. Changes nothing. |
| `reconcile` | follow-ups §2: restarts every replica of `vol1`, attaches it on fl-a, fails if the first read waits more than 5 s ([script](../scripts/checks/reconcile.sh)) |
| `resync` | follow-ups §3: a 16 GiB volume with 4 GiB written moved from fl-a to fl-b, fails above 4.5 GiB moved ([script](../scripts/checks/resync.sh)) |
| `pvc-loop` | follow-ups §8: 100 PVC create/use/delete cycles, fails unless the stores end where they started ([script](../scripts/checks/pvc-loop.sh)) |
| `node-fault`, `partition-test`, `chaos` | the `make` targets of the same names |
| `csi-test` | `scripts/lab.sh csi test` |

Every check but `status` takes the lab over (it restarts nodes, attaches and moves volumes), so it
runs only with **take-over** checked. Do not run one during a demo.

The lab runner reaches rhythm and rhythm-backup directly through its `~/.ssh/config`. The VMs
authorize only the key that created them; on centaur that is `~/.ssh/id_rsa_mac`, which the
workflow passes as `SSH_IDENTITY` (override with the repository variable `LAB_SSH_IDENTITY`). The
runner has no `.lab/` state of its own, so the checks use the lab's Kubernetes on fl-w, not the
Mac demo control plane.
