//! Linearizability checking of single-block histories (protocol spec §9).
//!
//! Every block is a register with unique written values, checked on its own: linearizability
//! is compositional, so a history is linearizable exactly when each block's sub-history is.
//! The search follows Wing & Gong, with memoization on (linearized set, current value).

use std::collections::HashSet;

use crate::types::Value;

/// One operation on a block, with real (simulator) times.
#[derive(Debug, Clone, Copy)]
pub struct COp {
    pub invoke: u64,
    /// Completion time. For an optional operation, the latest time it may take effect.
    pub ret: u64,
    pub kind: CKind,
    /// The operation may also never take effect (a write without an answer).
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CKind {
    Read(Value),
    Write(Value),
}

/// Most operations per block the checker accepts.
pub const MAX_OPS: usize = 128;

/// Checks one block's history, starting from value 0. Returns an explanation on failure.
pub fn check_register(ops: &[COp]) -> Result<(), String> {
    assert!(ops.len() <= MAX_OPS, "too many operations for one block");
    let mut ops = ops.to_vec();
    ops.sort_by_key(|o| (o.invoke, o.ret));
    let all: u128 = if ops.len() == 128 {
        u128::MAX
    } else {
        (1u128 << ops.len()) - 1
    };
    let mut seen = HashSet::new();
    let mut deepest = 0u128;
    if search(&ops, 0, 0, all, &mut seen, &mut deepest) {
        return Ok(());
    }
    // Explain from the longest prefix found: the first operation that could not be placed.
    let stuck: Vec<String> = (0..ops.len())
        .filter(|i| deepest & (1 << i) == 0)
        .take(6)
        .map(|i| format!("{:?}", ops[i]))
        .collect();
    Err(format!(
        "not linearizable; placed {} of {} operations; next unplaced: {}",
        deepest.count_ones(),
        ops.len(),
        stuck.join(", ")
    ))
}

fn search(
    ops: &[COp],
    done: u128,
    value: Value,
    all: u128,
    seen: &mut HashSet<(u128, Value)>,
    deepest: &mut u128,
) -> bool {
    if done == all {
        return true;
    }
    if !seen.insert((done, value)) {
        return false;
    }
    if done.count_ones() > deepest.count_ones() {
        *deepest = done;
    }
    // An operation can go next only if it was invoked before every other open operation
    // returned.
    let open = |i: usize| done & (1 << i) == 0;
    let mut first_ret = u64::MAX;
    let mut second_ret = u64::MAX;
    let mut first_idx = usize::MAX;
    for (i, o) in ops.iter().enumerate().filter(|(i, _)| open(*i)) {
        if o.ret < first_ret {
            second_ret = first_ret;
            first_ret = o.ret;
            first_idx = i;
        } else if o.ret < second_ret {
            second_ret = o.ret;
        }
    }
    for (i, o) in ops.iter().enumerate() {
        if !open(i) {
            continue;
        }
        let others_ret = if i == first_idx {
            second_ret
        } else {
            first_ret
        };
        if o.invoke > others_ret {
            // Sorted by invocation: no later operation can go first either.
            break;
        }
        let next = done | (1 << i);
        match o.kind {
            CKind::Read(v) if v == value => {
                if search(ops, next, value, all, seen, deepest) {
                    return true;
                }
            }
            CKind::Read(_) => {}
            CKind::Write(v) => {
                if search(ops, next, v, all, seen, deepest) {
                    return true;
                }
            }
        }
        if o.optional && search(ops, next, value, all, seen, deepest) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(invoke: u64, ret: u64, kind: CKind) -> COp {
        COp {
            invoke,
            ret,
            kind,
            optional: false,
        }
    }

    fn opt(invoke: u64, deadline: u64, v: Value) -> COp {
        COp {
            invoke,
            ret: deadline,
            kind: CKind::Write(v),
            optional: true,
        }
    }

    use CKind::{Read as R, Write as W};

    #[test]
    fn sequential_histories() {
        assert!(check_register(&[op(0, 1, W(1)), op(2, 3, R(1))]).is_ok());
        assert!(check_register(&[op(0, 1, R(0)), op(2, 3, W(1)), op(4, 5, R(1))]).is_ok());
        // A stale read after a completed write.
        assert!(check_register(&[op(0, 1, W(1)), op(2, 3, R(0))]).is_err());
        // Reading a value nobody wrote.
        assert!(check_register(&[op(0, 1, R(7))]).is_err());
    }

    #[test]
    fn concurrent_operations_may_go_either_way() {
        // A read overlapping a write may see either value.
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(0))]).is_ok());
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(1))]).is_ok());
        // But once a read saw the new value, a later read cannot see the old one.
        assert!(check_register(&[op(0, 10, W(1)), op(2, 3, R(1)), op(4, 5, R(0))]).is_err());
    }

    #[test]
    fn optional_writes() {
        // An unanswered write may take effect...
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(1))]).is_ok());
        // ...or not...
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(0))]).is_ok());
        // ...but not both, in that order.
        assert!(check_register(&[opt(0, u64::MAX, 1), op(5, 6, R(1)), op(7, 8, R(0))]).is_err());
        // And not after its deadline: here, after a newer write completed and was read.
        assert!(
            check_register(&[opt(0, 4, 1), op(2, 4, W(2)), op(5, 6, R(2)), op(7, 8, R(1))])
                .is_err()
        );
        assert!(check_register(&[opt(0, 4, 1), op(2, 4, W(2)), op(5, 6, R(2))]).is_ok());
    }
}
