# Flaminio developer and lab entry points. `make help` lists everything.
#
# Lab targets take VM=a|b|all (default all) and drive scripts/lab.sh, which runs commands on
# the test servers over SSH. Settings live in scripts/lab.conf and can be overridden from the
# environment.

SHELL := $(shell command -v bash)
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

CARGO     ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
GO        ?= go
BUF       := $(GO) run github.com/bufbuild/buf/cmd/buf@v1.73.0
DATAPLANE := dataplane
LAB       := scripts/lab.sh
VM        ?= all
CASES     ?= 50000
SEEDS     ?= 20000
RUNTIME   ?= 30
# Benchmarks measure reads by default: SSD/NVMe write results are not stable enough to rely on.
# JOBS=. runs everything, writes included.
JOBS      ?= read
JOURNAL   ?=
IMAGE     ?= .lab/disk.img
IMAGE_SIZE ?= 10G

.PHONY: help
help: ## Show this help
	@awk 'BEGIN {FS = ":.*## "} \
	    /^##@/ {printf "\n\033[1m%s\033[0m\n", substr($$0, 5)} \
	    /^[a-zA-Z0-9_-]+:.*## / {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)
	@printf '\nLab targets take VM=a|b|w|all|cluster (default all: a and b). Example: make vm-status VM=b\n'

##@ Development (local)

.PHONY: build release test crash-test sim meta-interop lint fmt proto meta-vectors disk-demo clean
build: ## Build the data plane (debug) and the Go services
	cd $(DATAPLANE) && $(CARGO) build
	$(GO) build ./...

release: ## Build the data plane (release)
	cd $(DATAPLANE) && $(CARGO) build --release

test: ## Run all tests (Rust and Go)
	cd $(DATAPLANE) && $(CARGO) test
	$(GO) test ./...

crash-test: ## Long crash-consistency campaign (CASES=50000)
	cd $(DATAPLANE) && PROPTEST_CASES=$(CASES) $(CARGO) test --release -p flaminio-store --test crash_consistency

sim: ## Replication-protocol simulation: SEEDS random runs, then every injected bug must be caught
	cd $(DATAPLANE) && $(CARGO) run --release -q -p flaminio-repl --example sim -- 0 $(SEEDS)
	cd $(DATAPLANE) && $(CARGO) test --release -q -p flaminio-repl -- --include-ignored

meta-interop: ## The data plane's Rust metadata client against the Go service, over mutual TLS
	CARGO=$(CARGO) scripts/dev/meta-interop.sh

lint: ## clippy (warnings are errors), rustfmt, go vet, gofmt, buf lint, shellcheck
	cd $(DATAPLANE) && $(CARGO) clippy --all-targets -- -D warnings && $(CARGO) fmt --check
	$(GO) vet ./...
	@test -z "$$(gofmt -l api meta pkg control)" || { echo "gofmt needed:"; gofmt -l api meta pkg control; exit 1; }
	$(BUF) lint
	@if command -v shellcheck >/dev/null; then shellcheck -x scripts/*.sh scripts/host/*.sh scripts/guest/*.sh scripts/dev/*.sh && echo "shellcheck: ok"; \
	 else echo "shellcheck not installed (brew install shellcheck); skipped"; fi

fmt: ## Format Rust and Go code
	cd $(DATAPLANE) && $(CARGO) fmt
	gofmt -w meta pkg control

proto: ## Regenerate Go code from api/proto (buf and plugins run at pinned versions)
	$(BUF) lint
	$(BUF) generate

meta-vectors: ## Regenerate the metadata transition vectors from the Rust model
	cd $(DATAPLANE) && $(CARGO) run -q -p flaminio-repl --example meta_vectors > ../meta/testdata/isr-vectors.jsonl

disk-demo: release ## Format and inspect a sparse disk image (IMAGE, IMAGE_SIZE)
	@mkdir -p $(dir $(IMAGE))
	$(DATAPLANE)/target/release/flaminio-disk format $(IMAGE) --size $(IMAGE_SIZE) --label demo --force
	$(DATAPLANE)/target/release/flaminio-disk inspect $(IMAGE)

clean: ## Remove build output
	cd $(DATAPLANE) && $(CARGO) clean

##@ Lab: servers

.PHONY: lab-check host-prep lab-up
lab-check: ## Check SSH, sudo, KVM and libvirt on the servers
	$(LAB) check $(VM)

host-prep: ## Install KVM/libvirt, storage pool, network and ufw rules (idempotent)
	$(LAB) host-prep $(VM)

lab-up: host-prep vm-create net-setup provision deploy net-check ## Everything from scratch: hosts, VMs, network, toolchain, build

##@ Lab: VMs

.PHONY: vm-create vm-start vm-stop vm-restart vm-redefine vm-console-log vm-status vm-destroy vm-ssh vm-console provision
vm-create: ## Create and boot VMs (idempotent)
	$(LAB) create $(VM)

vm-start: ## Start VMs
	$(LAB) start $(VM)

vm-stop: ## Shut VMs down
	$(LAB) stop $(VM)

vm-restart: ## Reboot VMs (FORCE=1 for a hard reset of a hung guest)
	FORCE=$(FORCE) $(LAB) restart $(VM)

vm-redefine: ## Apply the current libvirt template to existing VMs (next boot)
	$(LAB) redefine $(VM)

vm-console-log: ## End of a VM's console log (VM=a or VM=b)
	$(LAB) console-log $(if $(filter all,$(VM)),a,$(VM))

vm-status: ## State, kernel, ublk, data disk and toolchain of each VM
	$(LAB) status $(VM)

vm-destroy: ## Delete VMs and their disks (needs CONFIRM=yes)
	CONFIRM=$(CONFIRM) $(LAB) destroy $(VM)

vm-ssh: ## Interactive shell in a VM (VM=a or VM=b)
	$(LAB) ssh $(if $(filter all,$(VM)),a,$(VM))

vm-console: ## Serial console of a VM (VM=a or VM=b; exit with Ctrl-])
	$(LAB) console $(if $(filter all,$(VM)),a,$(VM))

provision: ## Install the toolchain and load ublk in the VMs
	$(LAB) provision $(VM)

##@ Lab: ublk frontend (MVP step 4)

.PHONY: ublk-up ublk-down ublk-status ublk-smoke bench-ublk
UVM = $(if $(filter all,$(VM)),a,$(VM))
ublk-up: ## Start flaminio-io serving /dev/ublkb0 in a VM (default a; FORMAT=1 reformats the disk)
	FORMAT=$(FORMAT) $(LAB) ublk up $(UVM)

ublk-down: ## Stop flaminio-io and remove the ublk device
	$(LAB) ublk down $(UVM)

ublk-status: ## flaminio-io service state, device geometry, recent log
	$(LAB) ublk status $(UVM)

ublk-smoke: ## ext4 + files + daemon restart + checksum verification on /dev/ublkb0
	$(LAB) ublk smoke $(UVM)

bench-ublk: ## fio reads on /dev/ublkb0 (RUNTIME); fills 32 GiB first, every run
	RUNTIME=$(RUNTIME) $(LAB) ublk fio $(UVM)

##@ Lab: replication (MVP step 5)

.PHONY: repl-up repl-down repl-verify repl-fault
repl-up: ## RF=2: replica service on fl-b, fl-a's /dev/ublkb0 mirroring to it (FORMAT=1 reformats both)
	FORMAT=$(FORMAT) $(LAB) repl-up

repl-down: ## Stop the target on fl-a and the replica on fl-b
	$(LAB) repl-down

repl-verify: ## Stop both sides and check that the two replicas hold identical data
	$(LAB) repl-verify

repl-fault: ## Verified writes while the replica is stopped and restarted; then repl-verify
	$(LAB) repl-fault

##@ Lab: network

.PHONY: net-setup net-teardown net-check
net-setup: ## Port forwards on both hosts: replication to fl-b, overlay (UDP) to every VM
	$(LAB) net-setup

net-teardown: ## Remove the port forwards
	$(LAB) net-teardown

net-check: ## End-to-end check: fl-a -> rhythm-backup -> fl-b
	$(LAB) net-check

##@ Lab: metadata (phase 1; VM=w is the witness, VM=cluster all three)

.PHONY: node-up node-down node-status vol-create vol-attach vol-show vol-delete node-fault
node-up: ## Phase-1 nodes on the data VMs, replicas only (FORMAT=1 reformats the disks)
	FORMAT=$(or $(FORMAT),0) $(LAB) node up $(VM)

node-down: ## Stop the phase-1 nodes
	$(LAB) node down $(VM)

node-status: ## Phase-1 node state
	$(LAB) node status $(VM)

vol-create: ## Create the test volume (VOL, VOL_SIZE, VOL_CHUNK in lab.conf)
	$(LAB) vol create

vol-attach: ## Attach the test volume to a node, which serves it at /dev/ublkb1 (VM=a|b)
	$(LAB) vol attach $(if $(filter-out all,$(VM)),$(VM),a)

vol-show: ## The test volume: epoch, target, ISR per chunk
	$(LAB) vol show

vol-delete: ## Delete the test volume from the metadata service
	$(LAB) vol delete

node-fault: ## Verified writes while fl-b's node restarts; ISR restored; failover to fl-b verified
	$(LAB) node-fault

.PHONY: partition-test
partition-test: ## fl-a (the target) cut off the overlay, fl-b takes over: no stale reads, no lost writes (HOLD=30 s)
	HOLD=$(or $(HOLD),30) $(LAB) partition

.PHONY: pki-issue mesh-up mesh-down mesh-check etcd-up etcd-down etcd-status meta-up meta-down meta-status meta-smoke cluster-up
cluster-up: ## fl-w, overlay, identities, etcd and the metadata service on a, b and w
	$(LAB) create w
	$(LAB) provision w
	$(LAB) net-setup
	$(LAB) mesh up
	$(LAB) pki issue
	$(LAB) etcd up
	$(LAB) meta up

pki-issue: ## (Re)issue every VM's certificates; keys are made in the VMs (CAs in .lab/pki)
	$(LAB) pki issue $(if $(filter-out all,$(VM)),$(VM),cluster)

mesh-up: ## WireGuard overlay between fl-a, fl-b and fl-w (10.77.0.0/24)
	$(LAB) mesh up

mesh-down: ## Stop the overlay
	$(LAB) mesh down

mesh-check: ## Ping every VM pair over the overlay
	$(LAB) mesh check

etcd-up: ## etcd member on each VM (default all three)
	$(LAB) etcd up $(if $(filter-out all,$(VM)),$(VM),cluster)

etcd-down: ## Stop etcd members
	$(LAB) etcd down $(if $(filter-out all,$(VM)),$(VM),cluster)

etcd-status: ## etcd member status
	$(LAB) etcd status $(if $(filter-out all,$(VM)),$(VM),cluster)

meta-up: ## Build flaminio-meta for Linux and run it on each VM
	$(LAB) meta up $(if $(filter-out all,$(VM)),$(VM),cluster)

meta-down: ## Stop the metadata service
	$(LAB) meta down $(if $(filter-out all,$(VM)),$(VM),cluster)

meta-status: ## Metadata service state
	$(LAB) meta status $(if $(filter-out all,$(VM)),$(VM),cluster)

meta-smoke: ## End-to-end metadata check, including the loss of one and two etcd members
	$(LAB) meta-smoke

##@ Lab: Kubernetes (k3s server on fl-w, agents on fl-a and fl-b, over the overlay)

.PHONY: k8s-up k8s-down k8s-status k8s-smoke k8s-uninstall kubeconfig k8s-tunnel kubectl
k8s-up: ## Install and start k3s (K3S_VERSION in lab.conf); writes .lab/kubeconfig
	$(LAB) k8s up

k8s-down: ## Stop k3s and its pods on every VM (keeps its state)
	$(LAB) k8s down

k8s-status: ## k3s on each VM, nodes and pods
	$(LAB) k8s status

k8s-smoke: ## Pods on fl-a and fl-b through a service over the overlay; Pod Security enforced; the witness takes no pods
	$(LAB) k8s smoke

k8s-uninstall: ## Remove k3s and its state from every VM (CONFIRM=yes)
	CONFIRM=$(CONFIRM) $(LAB) k8s uninstall

kubeconfig: ## (Re)write .lab/kubeconfig for kubectl on this workstation, through k8s-tunnel
	$(LAB) k8s kubeconfig

k8s-tunnel: ## Forward 127.0.0.1:16443 to the API server (foreground; KUBECONFIG=.lab/kubeconfig)
	$(LAB) k8s tunnel

kubectl: ## kubectl on fl-w, no tunnel needed: make kubectl ARGS='get pods -A'
	$(LAB) kubectl $(ARGS)

.PHONY: csi-deploy csi-status csi-logs csi-test csi-failover csi-undeploy
csi-deploy: ## Build the CSI driver's image, import it on fl-a and fl-b, apply deploy/csi (needs node-up)
	$(LAB) csi deploy

csi-status: ## CSI pods, StorageClass, PVs, VolumeAttachments, the test pod
	$(LAB) csi status

csi-logs: ## Recent logs of the CSI controller and node plugins
	$(LAB) csi logs

csi-test: ## A pod writes to a Flaminio PVC, moves to the other node, verifies its data (KEEP=1 keeps it)
	KEEP=$(KEEP) $(LAB) csi test

csi-failover: ## Power off the test pod's data VM: the pod must come back on the other one with its data; the VM rejoins (KEEP=1)
	KEEP=$(KEEP) $(LAB) csi failover

csi-undeploy: ## Remove the CSI driver (and the test namespace)
	$(LAB) csi undeploy

##@ Test suites

.PHONY: check lab-test
check: lint test sim meta-interop ## Everything that runs on this workstation: lint, tests, simulation, Rust/Go interop

lab-test: ## Every lab test, in order: overlay, metadata, node fault, partition, Kubernetes, CSI (about 30 min)
	$(LAB) mesh check
	$(LAB) meta-smoke
	$(LAB) node-fault
	HOLD=$(or $(HOLD),30) $(LAB) partition
	$(LAB) k8s smoke
	$(LAB) csi test

##@ Lab: build, test, benchmark

.PHONY: deploy vm-lint vm-test vm-crash-test bench-raw bench-store
deploy: ## Copy the working tree to the VMs and build (release)
	$(LAB) deploy $(VM)

vm-lint: ## clippy inside a VM: also checks the Linux-only code (io_uring, ublk)
	$(LAB) lint $(if $(filter all,$(VM)),a,$(VM))

vm-test: ## Run the test suite inside a VM on Linux (default VM a)
	$(LAB) test $(if $(filter all,$(VM)),a,$(VM))

vm-crash-test: ## Long crash-test campaign inside a VM (CASES)
	PROPTEST_CASES=$(CASES) $(LAB) test $(if $(filter all,$(VM)),a,$(VM)) -p flaminio-store --test crash_consistency

bench-raw: ## fio baseline on the raw data disks (RUNTIME, JOBS=regex; default reads, JOBS=. adds writes, which destroy data)
	RUNTIME=$(RUNTIME) JOBS='$(JOBS)' $(LAB) bench-raw $(VM)

bench-store: ## Replica-store benchmark (RUNTIME, JOBS=regex, default reads; FORMAT=0 reuses the disk, else reformats)
	RUNTIME=$(RUNTIME) JOBS='$(JOBS)' JOURNAL='$(JOURNAL)' $(LAB) bench-store $(VM)
