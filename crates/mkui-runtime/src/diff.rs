//! Keyed-child diffing — minimal add/remove/reorder/update operations for
//! a shuffled keyed list (#268).
//!
//! The classic virtual-DOM-style problem, scoped here to `mkui-runtime`'s
//! own tree: given an old child list and a new one, each item tagged with
//! a stable key, compute the smallest set of operations that turns one
//! into the other. "Minimal" specifically means:
//!
//! - a key present in both lists never produces a [`DiffOp::Remove`] +
//!   [`DiffOp::Insert`] pair just because it moved — it produces exactly
//!   one [`DiffOp::Move`] (if its position needed to change) or
//!   [`DiffOp::Update`] (if it didn't);
//! - a key whose position *and* content are both unchanged produces no
//!   op at all — see `changed` below. Without that, an unrelated single
//!   insert into an N-item stable list would emit N-1 meaningless
//!   `Update`s, one per untouched sibling.
//!
//! [`diff_keyed`] is deliberately tree-agnostic: it compares `key`
//! sequences, not [`crate::tree::Node`]s. Keys never need to live on
//! `Node` itself, which matters because `Node` is `Serialize` and
//! `crate::snapshot` is the byte-identical Rust/C/Python parity gate — a
//! `key` field there would be one more thing three bindings would need to
//! agree on for no benefit, since only the caller (the component that
//! owns the keyed list) ever needs the key/[`crate::tree::NodeId`]
//! association.
//!
//! ## Applying the result
//!
//! This function does not mutate an [`crate::tree::AppTree`] itself. A
//! caller applies the diff by:
//!
//! 1. For every [`DiffOp::Insert`], build the new node (`push_*`) and
//!    remember its `NodeId` against the key.
//! 2. For every [`DiffOp::Remove`], call
//!    [`crate::tree::AppTree::remove_node`] (#83) on the node — it detaches
//!    the node from its parent, recurses into any children it still has,
//!    and unregisters any `ActionId` a removed `Button` held. Do this
//!    *before* step 4's `reorder_children` call: a removed node must not
//!    appear in the `order` slice that call receives, since
//!    `reorder_children` requires `order` to be a permutation of *all*
//!    current children and would reject a stale id outright (wrong
//!    length/unknown id).
//! 3. For every [`DiffOp::Move`] or [`DiffOp::Update`], resync props as
//!    needed (e.g. `AppTree::set_text_props`) — both of those setters
//!    no-op if nothing actually changed, so this is cheap even when
//!    `changed` fires on a false positive.
//! 4. Build the final `NodeId` sequence from `new` (using the ids
//!    collected in steps 1/3) and call
//!    [`crate::tree::AppTree::reorder_children`] once. That single call
//!    realizes every `Move` — `diff_keyed` does not need to emit
//!    per-item move operations for the parent's children `Vec` to end up
//!    correct; `Move` vs. `Update` only distinguishes "did this key's
//!    relative position among the *retained* keys change" for a caller
//!    that wants to know (e.g. to animate a move), and gates nothing
//!    about whether `reorder_children` itself runs.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use crate::tree::NodeId;

/// One diff operation [`diff_keyed`] emits. `at` is always the key's
/// index within the final (`new`) list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffOp<K> {
    /// `key` has no entry in the old list — the caller creates a node for
    /// it and places it at index `at` in the final order.
    Insert { key: K, at: usize },
    /// `key` has no entry in the new list — the caller removes `node`
    /// (see the module docs' "Applying the result" step 2 for today's
    /// `#83`-shaped caveat).
    Remove { key: K, node: NodeId },
    /// `key` survives in both lists, but its position relative to the
    /// other retained keys changed. The final `reorder_children` call
    /// (see the module docs) handles the actual repositioning; this
    /// variant just signals "this one moved" for a caller that cares
    /// (e.g. to animate it). Emitted unconditionally for a moved key,
    /// regardless of what the `changed` predicate says — a real position
    /// change is never "nothing happened".
    Move { key: K, node: NodeId, at: usize },
    /// `key` survives in both lists, already in a position consistent
    /// with the other retained keys, **and** the caller-supplied
    /// `changed` predicate said its content differs. A key that is both
    /// stationary and unchanged produces no op at all — see the module
    /// docs' minimality note.
    Update { key: K, node: NodeId, at: usize },
}

/// Diff an `old` keyed child list (`(key, NodeId)` pairs, in current tree
/// order) against the `new` desired key order, returning the minimal set
/// of [`DiffOp`]s needed to turn one into the other.
///
/// `changed(key, node)` is called once for every key present in *both*
/// lists (never for an `Insert` or `Remove`) and should report whether
/// that key's non-positional content differs — e.g. by comparing against
/// the caller's own source-of-truth data for `key` against whatever
/// `node` currently holds. A stationary key for which `changed` returns
/// `false` produces no op at all, which is what keeps a single unrelated
/// insert from spraying `Update`s across every untouched sibling. A key
/// whose position changed always gets a `Move`, independent of
/// `changed`'s answer.
///
/// Fast-pathed for the common unchanged-frame case (#177's per-frame
/// budget contract): if `old` and `new` already agree key-for-key in the
/// same order, this skips the `HashMap`/`HashSet` the general path needs
/// and only calls `changed` once per key — if every call returns `false`,
/// the returned `Vec` never allocates (nothing is ever pushed to it).
///
/// Debug builds assert that `old` and `new` each carry unique keys — a
/// duplicate key makes "the" matching old/new entry ambiguous, which is a
/// caller bug this function should surface loudly rather than paper over.
pub fn diff_keyed<K>(
    old: &[(K, NodeId)],
    new: &[K],
    mut changed: impl FnMut(&K, NodeId) -> bool,
) -> Vec<DiffOp<K>>
where
    K: Eq + Hash + Clone,
{
    let same_order = old.len() == new.len()
        && old
            .iter()
            .zip(new)
            .all(|((old_key, _), new_key)| old_key == new_key);
    if same_order {
        let mut ops = Vec::new();
        for (at, (key, id)) in old.iter().enumerate() {
            if changed(key, *id) {
                ops.push(DiffOp::Update {
                    key: key.clone(),
                    node: *id,
                    at,
                });
            }
        }
        return ops;
    }

    debug_assert!(
        {
            let mut seen: HashSet<&K> = HashSet::with_capacity(old.len());
            old.iter().all(|(key, _)| seen.insert(key))
        },
        "diff_keyed: `old` contains a duplicate key"
    );
    debug_assert!(
        {
            let mut seen: HashSet<&K> = HashSet::with_capacity(new.len());
            new.iter().all(|key| seen.insert(key))
        },
        "diff_keyed: `new` contains a duplicate key"
    );

    let old_index: HashMap<&K, (usize, NodeId)> = old
        .iter()
        .enumerate()
        .map(|(pos, (key, id))| (key, (pos, *id)))
        .collect();
    let new_set: HashSet<&K> = new.iter().collect();

    let mut ops = Vec::with_capacity(old.len().max(new.len()));

    for (key, id) in old {
        if !new_set.contains(key) {
            ops.push(DiffOp::Remove {
                key: key.clone(),
                node: *id,
            });
        }
    }

    // Old position of every `new` entry that also exists in `old`,
    // `None` for a brand-new key — used to find which retained keys are
    // already in relative order (the LIS) and so need no `Move`.
    let matched: Vec<Option<usize>> = new
        .iter()
        .map(|key| old_index.get(key).map(|(pos, _)| *pos))
        .collect();
    let stays_put = longest_increasing_subsequence(&matched);

    for (at, key) in new.iter().enumerate() {
        match old_index.get(key) {
            None => ops.push(DiffOp::Insert {
                key: key.clone(),
                at,
            }),
            Some((_, id)) => {
                if stays_put.contains(&at) {
                    if changed(key, *id) {
                        ops.push(DiffOp::Update {
                            key: key.clone(),
                            node: *id,
                            at,
                        });
                    }
                } else {
                    ops.push(DiffOp::Move {
                        key: key.clone(),
                        node: *id,
                        at,
                    });
                }
            }
        }
    }

    ops
}

/// Indices (into `matched`, i.e. positions in the `new` list) belonging
/// to the longest run of matched entries whose old positions are already
/// strictly increasing — the standard keyed-diff trick (Vue 3/Inferno
/// style) for finding the minimal set of items that must move: everything
/// in this set can stay where it is: everything else needs a `Move`.
/// `None` entries (brand-new keys) are ignored entirely — they're never
/// part of the LIS, only the matched old positions are.
fn longest_increasing_subsequence(matched: &[Option<usize>]) -> HashSet<usize> {
    // `positions[i]` is the index into `new` (i.e. the `at` value) that
    // `values[i]` (an old position) came from.
    let mut positions = Vec::with_capacity(matched.len());
    let mut values = Vec::with_capacity(matched.len());
    for (at, slot) in matched.iter().enumerate() {
        if let Some(old_pos) = slot {
            positions.push(at);
            values.push(*old_pos);
        }
    }
    if values.is_empty() {
        return HashSet::new();
    }

    // Patience-sorting LIS: `tails[k]` holds the index (into `values`) of
    // the smallest tail value of any increasing subsequence of length
    // `k + 1` found so far. `predecessors[i]` is `values[i]`'s predecessor
    // in whatever subsequence it ultimately belongs to, for
    // reconstruction. Old positions are unique (each old key appears at
    // most once), so a strict `<` comparison is correct — no need to
    // special-case equal values.
    let mut tails: Vec<usize> = Vec::new();
    let mut predecessors: Vec<Option<usize>> = vec![None; values.len()];

    for i in 0..values.len() {
        let v = values[i];
        let insert_at = tails.partition_point(|&tail_idx| values[tail_idx] < v);
        if insert_at > 0 {
            predecessors[i] = Some(tails[insert_at - 1]);
        }
        if insert_at == tails.len() {
            tails.push(i);
        } else {
            tails[insert_at] = i;
        }
    }

    let mut lis_value_indices = HashSet::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(i) = cursor {
        lis_value_indices.insert(i);
        cursor = predecessors[i];
    }

    lis_value_indices
        .into_iter()
        .map(|value_idx| positions[value_idx])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(index: u32) -> NodeId {
        NodeId::from_raw(index, 0)
    }

    /// Every structural test below only cares about insert/remove/move
    /// counts, not content-change detection — so they all pass a
    /// predicate that always reports "changed" for a retained key. The
    /// predicate's own gating behavior gets its own dedicated tests.
    fn always_changed<K>(_key: &K, _node: NodeId) -> bool {
        true
    }

    #[test]
    fn identical_lists_with_nothing_changed_produce_no_ops() {
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["a", "b", "c"];
        assert_eq!(diff_keyed(&old, &new, |_, _| false), Vec::new());
    }

    #[test]
    fn same_order_but_one_changed_key_produces_exactly_one_update() {
        // The #1 fix this test locks in: an unrelated, content-unchanged
        // sibling must not get an Update op just because *some* key in
        // the list changed. Only "b" reports changed.
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["a", "b", "c"];
        let ops = diff_keyed(&old, &new, |key, _| *key == "b");
        assert_eq!(
            ops,
            vec![DiffOp::Update {
                key: "b",
                node: id(2),
                at: 1
            }]
        );
    }

    #[test]
    fn an_insert_does_not_spray_updates_across_untouched_siblings() {
        // Same fix, general path: inserting "d" must not also emit
        // Update for the untouched, stationary a/b/c.
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["a", "b", "c", "d"];
        let ops = diff_keyed(&old, &new, |_, _| false);
        assert_eq!(ops, vec![DiffOp::Insert { key: "d", at: 3 }]);
    }

    #[test]
    fn a_single_insert_with_always_changed_updates_every_retained_key() {
        let old = [("a", id(1)), ("b", id(2))];
        let new = ["a", "b", "c"];
        let ops = diff_keyed(&old, &new, always_changed);
        assert_eq!(
            ops,
            vec![
                DiffOp::Update {
                    key: "a",
                    node: id(1),
                    at: 0
                },
                DiffOp::Update {
                    key: "b",
                    node: id(2),
                    at: 1
                },
                DiffOp::Insert { key: "c", at: 2 },
            ]
        );
    }

    #[test]
    fn a_single_remove_produces_exactly_one_remove_op() {
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["a", "c"];
        let ops = diff_keyed(&old, &new, always_changed);
        assert_eq!(
            ops,
            vec![
                DiffOp::Remove {
                    key: "b",
                    node: id(2)
                },
                DiffOp::Update {
                    key: "a",
                    node: id(1),
                    at: 0
                },
                DiffOp::Update {
                    key: "c",
                    node: id(3),
                    at: 1
                },
            ]
        );
    }

    #[test]
    fn an_item_that_only_moved_never_produces_a_remove_and_insert_pair() {
        // Acceptance criterion 3's core claim: moving "a" from the front
        // to the back of [a, b, c] must be exactly one Move, never a
        // Remove{a} + Insert{a} pair.
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["b", "c", "a"];
        let ops = diff_keyed(&old, &new, |_, _| false);

        assert!(!ops
            .iter()
            .any(|op| matches!(op, DiffOp::Remove { key: "a", .. })));
        assert!(!ops
            .iter()
            .any(|op| matches!(op, DiffOp::Insert { key: "a", .. })));
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, DiffOp::Move { key: "a", .. }))
                .count(),
            1
        );
        // "b" and "c" keep their relative order and report unchanged, so
        // they produce no op at all; only "a" (which moved) does — a
        // single Move, nothing else.
        assert_eq!(
            ops,
            vec![DiffOp::Move {
                key: "a",
                node: id(1),
                at: 2
            },]
        );
    }

    #[test]
    fn representative_shuffle_with_insert_remove_and_reorder_is_minimal() {
        // Acceptance criterion 3: a representative mixed shuffle.
        // old: a b c d e
        // new: c a e f b   (d removed, f inserted, rest reordered)
        let old = [
            ("a", id(1)),
            ("b", id(2)),
            ("c", id(3)),
            ("d", id(4)),
            ("e", id(5)),
        ];
        let new = ["c", "a", "e", "f", "b"];
        let ops = diff_keyed(&old, &new, always_changed);

        // Exactly one Remove (d), exactly one Insert (f), and no key
        // appears in both a Remove and an Insert.
        let removes: Vec<_> = ops
            .iter()
            .filter(|o| matches!(o, DiffOp::Remove { .. }))
            .collect();
        let inserts: Vec<_> = ops
            .iter()
            .filter(|o| matches!(o, DiffOp::Insert { .. }))
            .collect();
        assert_eq!(removes.len(), 1);
        assert_eq!(inserts.len(), 1);
        assert!(matches!(removes[0], DiffOp::Remove { key: "d", node } if *node == id(4)));
        assert!(matches!(inserts[0], DiffOp::Insert { key: "f", at: 3 }));

        // Every retained key (a, b, c, e) shows up as exactly one Move or
        // Update — never duplicated, never dropped, never paired with a
        // Remove/Insert of the same key.
        for key in ["a", "b", "c", "e"] {
            let count = ops
                .iter()
                .filter(|op| {
                    matches!(op, DiffOp::Move { key: k, .. } | DiffOp::Update { key: k, .. } if *k == key)
                })
                .count();
            assert_eq!(
                count, 1,
                "key {key:?} must appear exactly once as Move/Update"
            );
            assert!(!removes
                .iter()
                .any(|o| matches!(o, DiffOp::Remove{key: k, ..} if *k == key)));
            assert!(!inserts
                .iter()
                .any(|o| matches!(o, DiffOp::Insert{key: k, ..} if *k == key)));
        }

        // Exactly 2 of the 4 retained keys can stay in relative order
        // (the longest increasing run among their old positions
        // [0,2,4,1] — e.g. a,b or c,e — has length 2), so exactly 2 must
        // move. Which specific pair the algorithm picks is an
        // implementation detail when multiple maximal runs tie; the
        // *count* is what "minimal" means here.
        let move_count = ops
            .iter()
            .filter(|op| matches!(op, DiffOp::Move { .. }))
            .count();
        let update_count = ops
            .iter()
            .filter(|op| matches!(op, DiffOp::Update { .. }))
            .count();
        assert_eq!(move_count, 2);
        assert_eq!(update_count, 2);
    }

    #[test]
    fn every_op_at_index_matches_its_position_in_new() {
        let old = [("a", id(1)), ("b", id(2)), ("c", id(3))];
        let new = ["c", "x", "a", "b"];
        let ops = diff_keyed(&old, &new, always_changed);
        for op in &ops {
            let at = match op {
                DiffOp::Insert { at, .. } | DiffOp::Move { at, .. } | DiffOp::Update { at, .. } => {
                    Some(*at)
                }
                DiffOp::Remove { .. } => None,
            };
            if let Some(at) = at {
                let key = match op {
                    DiffOp::Insert { key, .. }
                    | DiffOp::Move { key, .. }
                    | DiffOp::Update { key, .. } => key,
                    DiffOp::Remove { .. } => unreachable!(),
                };
                assert_eq!(new[at], *key);
            }
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "duplicate key")]
    fn duplicate_key_in_old_panics_in_debug_builds() {
        let old = [("a", id(1)), ("a", id(2))];
        let new = ["a"];
        let _ = diff_keyed(&old, &new, always_changed);
    }

    // ---- end-to-end: diff_keyed's ops actually applied to a real
    // AppTree via reorder_children (acceptance criterion 1 says "across
    // a diff", not just "across a reorder_children call") ----

    use crate::props::TextVariant;
    use crate::tree::AppTree;
    use std::collections::HashMap;

    #[test]
    fn diff_keyed_plus_reorder_children_applies_a_pure_shuffle_preserving_identity() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let mut old: Vec<(&str, NodeId)> = Vec::new();
        for key in ["a", "b", "c", "d"] {
            let node = tree.push_text(root, key, TextVariant::Body, "").unwrap();
            old.push((key, node));
        }
        let len_before = tree.len();
        let key_to_id: HashMap<&str, NodeId> = old.iter().cloned().collect();

        let new_keys = ["c", "a", "d", "b"];
        let ops = diff_keyed(&old, &new_keys, |_, _| false);

        // A pure shuffle must contain zero Insert/Remove — every key
        // survives, only positions change.
        assert!(!ops.iter().any(|op| matches!(op, DiffOp::Insert { .. })));
        assert!(!ops.iter().any(|op| matches!(op, DiffOp::Remove { .. })));

        let final_order: Vec<NodeId> = new_keys.iter().map(|k| key_to_id[k]).collect();
        assert!(tree.reorder_children(root, &final_order));

        assert_eq!(tree.get(root).unwrap().children, final_order);
        assert_eq!(
            tree.len(),
            len_before,
            "no node was removed or re-added by the diff + reorder"
        );
        for key in new_keys {
            let id = key_to_id[key];
            assert_eq!(
                tree.get(id).unwrap().id,
                id,
                "key {key:?}'s NodeId must survive the diff unchanged"
            );
        }
    }

    #[test]
    fn diff_keyed_plus_reorder_children_applies_a_shuffle_with_an_insert() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let mut old: Vec<(&str, NodeId)> = Vec::new();
        let mut key_to_id: HashMap<&str, NodeId> = HashMap::new();
        for key in ["a", "b", "c"] {
            let node = tree.push_text(root, key, TextVariant::Body, "").unwrap();
            old.push((key, node));
            key_to_id.insert(key, node);
        }

        // "d" is brand new; the rest shuffle.
        let new_keys = ["c", "d", "a", "b"];
        let ops = diff_keyed(&old, &new_keys, |_, _| false);
        assert_eq!(
            ops.iter()
                .filter(|op| matches!(op, DiffOp::Insert { .. }))
                .count(),
            1
        );
        assert!(!ops.iter().any(|op| matches!(op, DiffOp::Remove { .. })));

        for op in &ops {
            if let DiffOp::Insert { key, .. } = op {
                let node = tree.push_text(root, *key, TextVariant::Body, "").unwrap();
                key_to_id.insert(key, node);
            }
        }

        let final_order: Vec<NodeId> = new_keys.iter().map(|k| key_to_id[k]).collect();
        assert!(tree.reorder_children(root, &final_order));

        assert_eq!(tree.get(root).unwrap().children, final_order);
        for key in ["a", "b", "c"] {
            let id = key_to_id[key];
            assert_eq!(
                tree.get(id).unwrap().id,
                id,
                "key {key:?}'s NodeId must survive the diff unchanged"
            );
        }
    }
}
