//! Metadata-service transitions (protocol spec §2).
//!
//! The metadata service is linearizable (etcd). Its transitions on the replication path:
//! - [`MetaRequest::Attach`] bumps the epoch;
//! - [`MetaRequest::ChangeIsr`] is applied only under the current epoch and the expected ISR
//!   versions. Only the target makes it. It may also change a chunk's replica set, to carry out
//!   a requested move;
//! - [`MetaRequest::MoveReplica`] only *requests* a move (an administrator: drain). It changes
//!   neither replicas nor ISR: the target does, under its epoch (spec §7, moves).
//!
//! The Go service implements the same table.

use std::collections::{BTreeMap, BTreeSet};

use crate::types::{Block, ChunkIdx, Epoch, NodeId, VolumeId};

/// Placement and ISR of one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMeta {
    /// Nodes that hold a replica of the chunk.
    pub replicas: Vec<NodeId>,
    /// In-sync replicas: those known to hold every acknowledged write.
    pub isr: BTreeSet<NodeId>,
    /// Incremented on every ISR change, and on every change of `replicas`.
    pub isr_ver: u64,
    /// A requested move of the replica on `.0` to `.1`, until the target completes it.
    pub moving: Option<(NodeId, NodeId)>,
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
    /// For a move only: the new replica set. Either the current one plus the move's
    /// destination, or the current one minus its source, once the destination is in the new
    /// ISR (which completes the move).
    pub new_replicas: Option<Vec<NodeId>>,
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
    /// Requests that the chunk's replica on `from` move to `to`. Refused unless `from` holds
    /// a replica, `to` does not, and no move is pending on the chunk.
    MoveReplica {
        vol: VolumeId,
        chunk: ChunkIdx,
        from: NodeId,
        to: NodeId,
    },
    /// Reads the volume (the target polls it, to learn of requested moves).
    Get { vol: VolumeId },
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
                moving: None,
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
                            let replicas = c.new_replicas.as_ref().unwrap_or(&m.replicas);
                            m.isr_ver == c.expected_ver
                                && c.new_isr.len() >= v.min_isr
                                && c.new_isr.iter().all(|n| replicas.contains(n))
                                && c.new_replicas
                                    .as_ref()
                                    .is_none_or(|r| valid_move(m, r, &c.new_isr))
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
                    if let Some(r) = &c.new_replicas {
                        if m.moving.is_some_and(|(from, _)| !r.contains(&from)) {
                            m.moving = None;
                        }
                        m.replicas = r.clone();
                    }
                }
                MetaReply::Applied {
                    vol: *vol,
                    view: v.clone(),
                }
            }
            MetaRequest::MoveReplica {
                vol,
                chunk,
                from,
                to,
            } => {
                let Some(v) = self.volumes.get_mut(vol) else {
                    return MetaReply::NotFound { vol: *vol };
                };
                let Some(m) = v.chunks.get_mut(*chunk as usize) else {
                    return MetaReply::Rejected {
                        vol: *vol,
                        view: v.clone(),
                    };
                };
                if !m.replicas.contains(from) || m.replicas.contains(to) || m.moving.is_some() {
                    return MetaReply::Rejected {
                        vol: *vol,
                        view: v.clone(),
                    };
                }
                m.moving = Some((*from, *to));
                MetaReply::Applied {
                    vol: *vol,
                    view: v.clone(),
                }
            }
            MetaRequest::Get { vol } => match self.volumes.get(vol) {
                Some(v) => MetaReply::Applied {
                    vol: *vol,
                    view: v.clone(),
                },
                None => MetaReply::NotFound { vol: *vol },
            },
        }
    }
}

/// A replica-set change carries out the chunk's requested move: it adds the destination, or
/// removes the source once the destination is in the new ISR.
fn valid_move(m: &ChunkMeta, new: &[NodeId], new_isr: &BTreeSet<NodeId>) -> bool {
    let Some((from, to)) = m.moving else {
        return false;
    };
    let mut add = m.replicas.clone();
    add.push(to);
    let remove: Vec<NodeId> = m.replicas.iter().copied().filter(|&n| n != from).collect();
    (!m.replicas.contains(&to) && new == add.as_slice())
        || (m.replicas.contains(&from) && new_isr.contains(&to) && new == remove.as_slice())
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
            new_replicas: None,
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

    #[test]
    fn moves_are_requested_then_carried_out_by_the_target() {
        let mut m = meta();
        m.handle(&MetaRequest::Attach { vol: 7, node: 1 });
        let mv = |from, to| MetaRequest::MoveReplica {
            vol: 7,
            chunk: 0,
            from,
            to,
        };
        // Only from a replica, to a node without one, one move at a time.
        assert!(matches!(m.handle(&mv(4, 5)), MetaReply::Rejected { .. }));
        assert!(matches!(m.handle(&mv(1, 2)), MetaReply::Rejected { .. }));
        assert!(matches!(m.handle(&mv(1, 4)), MetaReply::Applied { .. }));
        assert!(matches!(m.handle(&mv(2, 5)), MetaReply::Rejected { .. }));
        let c0 = m.volume(7).unwrap().chunks[0].clone();
        assert_eq!(
            (c0.moving, c0.isr_ver, c0.replicas.clone()),
            (Some((1, 4)), 0, vec![1, 2, 3])
        );

        let change = |ver, isr: &[NodeId], replicas: &[NodeId]| MetaRequest::ChangeIsr {
            vol: 7,
            epoch: 1,
            changes: vec![IsrChange {
                chunk: 0,
                expected_ver: ver,
                new_isr: set(isr),
                new_replicas: Some(replicas.to_vec()),
            }],
        };
        let applied = |r: MetaReply| matches!(r, MetaReply::Applied { .. });
        // Dropping the source before the destination is in sync, or another replica: refused.
        assert!(!applied(m.handle(&change(0, &[2, 3], &[2, 3]))));
        assert!(!applied(m.handle(&change(0, &[1, 2, 3], &[1, 2]))));
        // Adding the destination, outside the ISR.
        assert!(applied(m.handle(&change(0, &[1, 2, 3], &[1, 2, 3, 4]))));
        // Still refused: the destination is not in the new ISR.
        assert!(!applied(m.handle(&change(1, &[2, 3], &[2, 3, 4]))));
        // It joins (a plain ISR change), then the source goes, and the move is done.
        let mut join = change(1, &[1, 2, 3, 4], &[]);
        if let MetaRequest::ChangeIsr { changes, .. } = &mut join {
            changes[0].new_replicas = None;
        }
        assert!(applied(m.handle(&join)));
        assert!(applied(m.handle(&change(2, &[2, 3, 4], &[2, 3, 4]))));
        let c0 = &m.volume(7).unwrap().chunks[0];
        assert_eq!(
            (c0.moving, c0.replicas.clone(), c0.isr_ver),
            (None, vec![2, 3, 4], 3)
        );
        // No move pending: replica sets are fixed again.
        assert!(!applied(m.handle(&change(3, &[2, 3], &[2, 3]))));
    }
}
