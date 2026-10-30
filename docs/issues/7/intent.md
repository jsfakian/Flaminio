# Issue #7: RegisterNode can lose labels when two registrations race — intent

[Issue #7](https://github.com/jsfakian/Flaminio/issues/7), follow-ups §4.

## Why

Two clients register the same node with the metadata service:
- the data plane registers its address, with no labels;
- the CSI node plugin registers the node's failure-domain labels (zone, rack), copied from the
  Kubernetes Node.

`RegisterNode` merges a registration's labels into the stored record: it reads the record, adds
the new labels, and writes it back. The write does not check that the record is still the one
it read. When two registrations interleave, the later write replaces the record that the
earlier one just wrote, and the earlier one's labels are lost. The plugin syncs topology only
when it starts, so a lost zone label stays lost: placement then treats the node as having no
zone, and can put two replicas of a chunk in one failure domain without knowing.

## What we want

1. A registration never loses a label written by another registration, however they
   interleave.
2. The rules of a registration stay as they are: its labels are added to the stored ones,
   overwriting a label of the same name, and its address replaces the stored address.
3. The CSI node plugin, which registers only to add labels, never puts back an old address.

## Out of scope

- **Removing labels.** A registration still cannot delete a label; nothing asks for that yet.
- **Other paths that read the node record.** The read-modify-write paths for volumes, chunks
  and tombstones already use the optimistic pattern this issue applies here. `NodeHeartbeat`
  and `DrainNode` only check that a node's record exists before they write their own keys,
  and nothing deletes a node record, so they need no compare.
- **The address the CSI node plugin sent** (now in scope, goal 3, from the code review). The
  plugin read the address with `ListNodes` and sent it back with the labels, in a separate call.
  If the data plane registered a new address in between, the plugin's registration put back the
  old one. A registration without an address now keeps the stored one, and the plugin sends
  none.
