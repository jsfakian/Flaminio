#!/usr/bin/env bash
# Per-chunk checksums of the volume on the data disk, to compare replicas. The services must be
# stopped: two processes must never open the same store.
for unit in flaminio-io flaminio-replica; do
    systemctl is-active --quiet "$unit" && die "$unit is running; stop it first"
done
sudo "$HOME/flaminio/dataplane/target/release/flaminio-io" digest --device "$(data_disk)" --size "${VOLUME_SIZE:-64G}"
