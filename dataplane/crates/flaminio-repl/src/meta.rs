//! Metadata-service transitions (protocol spec §2).
//!
//! The metadata service is linearizable (etcd). Its only transitions on the replication path
//! are [`MetaRequest::Attach`], which bumps the epoch, and [`MetaRequest::ChangeIsr`], which
//! is applied only under the current epoch and the expected ISR versions. The Go service
//! implements the same table.

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{Block, ChunkIdx, Epoch, NodeId, VolumeId};

/// Placement and ISR of one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMeta {
    /// Nodes that hold a replica of the chunk.
    pub replicas: Vec<NodeId>,
    /// In-sync replicas: those known to hold every acknowledged write.
    pub isr: BTreeSet<NodeId>,
    /// Incremented on every ISR change.
    pub isr_ver: u64,
}

/// A volume as the metadata service records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeMeta {
    pub epoch: Epoch,
    /// Node of the current target, if attached.
    pub target: Option<NodeId>,
    /// Writes block rather than proceed with fewer in-sync copies.
    pub min_isr: usize,
    pub blocks_per_chunk: Block,
    pub chunks: Vec<ChunkMeta>,
}

/// One chunk's part of a [`MetaRequest::ChangeIsr`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsrChange {
    pub chunk: ChunkIdx,
    pub expected_ver: u64,
    pub new_isr: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaRequest {
    /// Make `node` the volume's target under a new epoch.
    Attach { vol: VolumeId, node: NodeId },
    /// Change ISRs atomically, conditional on `epoch` and every `expected_ver`.
    ChangeIsr {
        vol: VolumeId,
        epoch: Epoch,
        changes: Vec<IsrChange>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaReply {
    /// The request was applied. `view` is the volume afterwards.
    Applied { vol: VolumeId, view: VolumeMeta },
    /// The request was refused; nothing changed. `view` is the current state.
    Rejected { vol: VolumeId, view: VolumeMeta },
    /// No such volume.
    NotFound { vol: VolumeId },
}

/// The metadata service's state.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    volumes: BTreeMap<VolumeId, VolumeMeta>,
}

impl Metadata {
    /// Creates a volume, unattached, with every replica in the ISR.
    pub fn create_volume(
        &mut self,
        vol: VolumeId,
        min_isr: usize,
        blocks_per_chunk: Block,
        placement: Vec<Vec<NodeId>>,
    ) {
        let chunks = placement
            .into_iter()
            .map(|replicas| ChunkMeta {
                isr: replicas.iter().copied().collect(),
                replicas,
                isr_ver: 0,
            })
            .collect();
        self.volumes.insert(
            vol,
            VolumeMeta {
                epoch: 0,
                target: None,
                min_isr,
                blocks_per_chunk,
                chunks,
            },
        );
    }

    pub fn volume(&self, vol: VolumeId) -> Option<&VolumeMeta> {
        self.volumes.get(&vol)
    }

    /// Applies one request (spec §2).
    pub fn handle(&mut self, req: &MetaRequest) -> MetaReply {
        match req {
            MetaRequest::Attach { vol, node } => {
                let Some(v) = self.volumes.get_mut(vol) else {
                    return MetaReply::NotFound { vol: *vol };
                };
                v.epoch += 1;
                v.target = Some(*node);
                MetaReply::Applied {
                    vol: *vol,
                    view: v.clone(),
                }
            }
            MetaRequest::ChangeIsr {
                vol,
                epoch,
                changes,
            } => {
                let Some(v) = self.volumes.get_mut(vol) else {
                    return MetaReply::NotFound { vol: *vol };
                };
                let distinct = changes
                    .iter()
                    .map(|c| c.chunk)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == changes.len();
                let valid = *epoch == v.epoch
                    && distinct
                    && changes.iter().all(|c| {
                        v.chunks.get(c.chunk as usize).is_some_and(|m| {
                            m.isr_ver == c.expected_ver
                                && c.new_isr.len() >= v.min_isr
                                && c.new_isr.iter().all(|n| m.replicas.contains(n))
                        })
                    });
                if !valid {
                    return MetaReply::Rejected {
                        vol: *vol,
                        view: v.clone(),
                    };
                }
                for c in changes {
                    let m = &mut v.chunks[c.chunk as usize];
                    m.isr = c.new_isr.clone();
                    m.isr_ver += 1;
                }
                MetaReply::Applied {
                    vol: *vol,
                    view: v.clone(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(n: &[NodeId]) -> BTreeSet<NodeId> {
        n.iter().copied().collect()
    }

    fn meta() -> Metadata {
        let mut m = Metadata::default();
        m.create_volume(7, 2, 4, vec![vec![1, 2, 3], vec![2, 3, 4]]);
        m
    }

    fn change(chunk: ChunkIdx, ver: u64, isr: &[NodeId]) -> IsrChange {
        IsrChange {
            chunk,
            expected_ver: ver,
            new_isr: set(isr),
        }
    }

    #[test]
    fn attach_bumps_the_epoch() {
        let mut m = meta();
        for e in 1..=3 {
            let r = m.handle(&MetaRequest::Attach { vol: 7, node: e });
            let MetaReply::Applied { view, .. } = r else {
                panic!("{r:?}")
            };
            assert_eq!(view.epoch, e as u64);
            assert_eq!(view.target, Some(e));
        }
    }

    #[test]
    fn isr_change_is_conditional_and_atomic() {
        let mut m = meta();
        m.handle(&MetaRequest::Attach { vol: 7, node: 1 });
        let req = |epoch, changes| MetaRequest::ChangeIsr {
            vol: 7,
            epoch,
            changes,
        };
        // Wrong epoch, wrong version, below the minimum, not a replica: all refused.
        for bad in [
            req(0, vec![change(0, 0, &[1, 2])]),
            req(1, vec![change(0, 1, &[1, 2])]),
            req(1, vec![change(0, 0, &[1])]),
            req(1, vec![change(0, 0, &[1, 4])]),
            // Atomic: the valid first change is not applied either.
            req(1, vec![change(0, 0, &[1, 2]), change(1, 0, &[2])]),
            // The same chunk twice.
            req(1, vec![change(0, 0, &[1, 2]), change(0, 0, &[1, 3])]),
        ] {
            assert!(
                matches!(m.handle(&bad), MetaReply::Rejected { .. }),
                "{bad:?}"
            );
        }
        assert_eq!(m.volume(7).unwrap().chunks[0].isr_ver, 0);

        let r = m.handle(&req(1, vec![change(0, 0, &[1, 2]), change(1, 0, &[3, 4])]));
        let MetaReply::Applied { view, .. } = r else {
            panic!("{r:?}")
        };
        assert_eq!(view.chunks[0].isr, set(&[1, 2]));
        assert_eq!(view.chunks[1].isr, set(&[3, 4]));
        assert_eq!((view.chunks[0].isr_ver, view.chunks[1].isr_ver), (1, 1));

        // A retry of an applied change no longer matches its version.
        let r = m.handle(&req(1, vec![change(0, 0, &[1, 2])]));
        assert!(matches!(r, MetaReply::Rejected { .. }));
    }
}
