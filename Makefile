# Flaminio developer and lab entry points. `make help` lists everything.
#
# Lab targets take VM=a|b|all (default all) and drive scripts/lab.sh, which runs commands on
# the test servers over SSH. Settings live in scripts/lab.conf and can be overridden from the
# environment.

SHELL := $(shell command -v bash)
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

CARGO     ?= $(shell command -v cargo 2>/dev/null || echo $(HOME)/.cargo/bin/cargo)
DATAPLANE := dataplane
LAB       := scripts/lab.sh
VM        ?= all
CASES     ?= 50000
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
	@printf '\nLab targets take VM=a|b|all (default all). Example: make vm-status VM=b\n'

##@ Development (local)

.PHONY: build release test crash-test lint fmt disk-demo clean
build: ## Build the data plane (debug)
	cd $(DATAPLANE) && $(CARGO) build

release: ## Build the data plane (release)
	cd $(DATAPLANE) && $(CARGO) build --release

test: ## Run all tests
	cd $(DATAPLANE) && $(CARGO) test

crash-test: ## Long crash-consistency campaign (CASES=50000)
	cd $(DATAPLANE) && PROPTEST_CASES=$(CASES) $(CARGO) test --release -p flaminio-store --test crash_consistency

lint: ## clippy (warnings are errors), rustfmt check, shellcheck
	cd $(DATAPLANE) && $(CARGO) clippy --all-targets -- -D warnings && $(CARGO) fmt --check
	@if command -v shellcheck >/dev/null; then shellcheck -x scripts/*.sh scripts/host/*.sh scripts/guest/*.sh && echo "shellcheck: ok"; \
	 else echo "shellcheck not installed (brew install shellcheck); skipped"; fi

fmt: ## Format Rust code
	cd $(DATAPLANE) && $(CARGO) fmt

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

##@ Lab: network

.PHONY: net-setup net-teardown net-check
net-setup: ## Forward the replication port on rhythm-backup to fl-b (from rhythm only)
	$(LAB) net-setup

net-teardown: ## Remove the replication port forward
	$(LAB) net-teardown

net-check: ## End-to-end check: fl-a -> rhythm-backup -> fl-b
	$(LAB) net-check

##@ Lab: build, test, benchmark

.PHONY: deploy vm-test vm-crash-test bench-raw bench-store
deploy: ## Copy the working tree to the VMs and build (release)
	$(LAB) deploy $(VM)

vm-test: ## Run the test suite inside a VM on Linux (default VM a)
	$(LAB) test $(if $(filter all,$(VM)),a,$(VM))

vm-crash-test: ## Long crash-test campaign inside a VM (CASES)
	PROPTEST_CASES=$(CASES) $(LAB) test $(if $(filter all,$(VM)),a,$(VM)) -p flaminio-store --test crash_consistency

bench-raw: ## fio baseline on the raw data disks (RUNTIME, JOBS=regex; default reads, JOBS=. adds writes, which destroy data)
	RUNTIME=$(RUNTIME) JOBS='$(JOBS)' $(LAB) bench-raw $(VM)

bench-store: ## Replica-store benchmark (RUNTIME, JOBS=regex, default reads; FORMAT=0 reuses the disk, else reformats)
	RUNTIME=$(RUNTIME) JOBS='$(JOBS)' JOURNAL='$(JOURNAL)' $(LAB) bench-store $(VM)
