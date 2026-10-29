use super::*;

fn abc() -> IdVec<NodeId, &'static str> {
    ["a", "b", "c"].into_iter().collect()
}

// Mutation: make `Index<I>` read slot 0 whatever the id is.
#[test]
fn indexing_by_id_reads_that_item() {
    let list = abc();
    assert_eq!(list[NodeId::new(0)], "a");
    assert_eq!(list[NodeId::new(2)], "c");
}

// Mutation: make `IndexMut<I>` write slot `id + 1`.
#[test]
fn writing_by_id_changes_that_item_only() {
    let mut list = abc();
    list[NodeId::new(1)] = "x";
    assert_eq!(list.as_slice(), ["a", "x", "c"]);
}

// Mutation: make `fill` write `value` into every slot but the last, or into `len - 1` slots.
#[test]
fn fill_overwrites_every_item_and_keeps_the_length() {
    let mut list: IdVec<NodeId, u8> = [1, 2, 3].into_iter().collect();
    list.fill(9);
    assert_eq!(list.len(), 3);
    assert_eq!(list.as_slice(), [9, 9, 9]);
    let mut empty = IdVec::<EdgeId, u8>::new();
    empty.fill(9);
    assert!(empty.is_empty());
}

// Mutation: make `get` use `items.get(id + 1)`, or answer `Some` for an id one past the end.
#[test]
fn get_says_none_for_an_id_the_list_does_not_hold() {
    let list = abc();
    assert_eq!(list.get(NodeId::new(0)), Some(&"a"));
    assert_eq!(list.get(NodeId::new(2)), Some(&"c"));
    assert_eq!(list.get(NodeId::new(3)), None);
    assert_eq!(list.get(NodeId::new(usize::MAX)), None);
    assert_eq!(IdVec::<NodeId, u8>::new().get(NodeId::new(0)), None);
}

// Mutation: make `get_mut` use `items.get_mut(id + 1)`.
#[test]
fn get_mut_reaches_that_item_and_refuses_an_unknown_id() {
    let mut list = abc();
    *list.get_mut(NodeId::new(1)).unwrap() = "x";
    assert_eq!(list.as_slice(), ["a", "x", "c"]);
    assert_eq!(list.get_mut(NodeId::new(3)), None);
}

// Mutation: return the id of the previous item from `push`, or `len + 1`.
#[test]
fn push_returns_the_id_of_the_item_it_added() {
    let mut list = IdVec::<EdgeId, &str>::new();
    let first = list.push("p");
    let second = list.push("q");
    assert_eq!((first, second), (EdgeId::new(0), EdgeId::new(1)));
    assert_eq!(list[second], "q");
    assert_eq!(list.next_id(), EdgeId::new(2));
}

// Mutation: make `filled` build `len - 1` copies, or fill every slot but the last with `value`.
#[test]
fn filled_holds_one_copy_per_id() {
    let mut list = IdVec::<NodeId, u8>::filled(3, 7);
    assert_eq!(list.len(), 3);
    assert_eq!(list.as_slice(), [7, 7, 7]);
    list[NodeId::new(2)] = 1;
    assert_eq!(list[NodeId::new(0)], 7);
    assert!(IdVec::<EdgeId, u8>::filled(0, 7).is_empty());
}

// Mutation: make `len` return `items.len() - 1`, or `is_empty` always false.
#[test]
fn len_and_is_empty_follow_the_items() {
    let mut list = IdVec::<NodeId, u8>::new();
    assert!(list.is_empty());
    assert_eq!(list.len(), 0);
    list.push(1);
    assert!(!list.is_empty());
    assert_eq!(list.len(), 1);
}

// Mutation: make `iter` skip the first item.
#[test]
fn iter_and_iter_mut_keep_the_order_of_the_items() {
    let mut list: IdVec<NodeId, u32> = [1, 2, 3].into_iter().collect();
    for item in list.iter_mut() {
        *item *= 10;
    }
    assert_eq!(list.iter().copied().collect::<Vec<_>>(), [10, 20, 30]);
    assert_eq!((&list).into_iter().count(), 3);
    assert_eq!(list.into_iter().collect::<Vec<_>>(), [10, 20, 30]);
}

// Mutation: walk `ids` backwards or stop one short.
#[test]
fn ids_run_in_order_and_do_not_borrow_the_list() {
    let mut list = abc();
    let mut seen = Vec::new();
    for id in list.ids() {
        seen.push(id.index());
        // The iterator does not borrow the list. Items added in the loop are not visited.
        if id.index() == 0 {
            list.push("d");
        }
    }
    assert_eq!(seen, [0, 1, 2]);
    assert_eq!(list.ids().len(), 4);
    assert_eq!(list.ids().next_back(), Some(NodeId::new(3)));
}

// Mutation: pair each item with the id of its neighbour.
#[test]
fn iter_with_ids_pairs_each_item_with_its_own_id() {
    let pairs = abc()
        .iter_with_ids()
        .map(|(id, item)| (id.index(), *item))
        .collect::<Vec<_>>();
    assert_eq!(pairs, [(0, "a"), (1, "b"), (2, "c")]);
}

// Mutation: make `extend` or `From<Vec>` drop or reorder items.
#[test]
fn extend_and_from_vec_keep_every_item_in_order() {
    let mut list: IdVec<NodeId, u8> = vec![1, 2].into();
    list.extend([3, 4]);
    assert_eq!(list.as_slice(), [1, 2, 3, 4]);
    assert_eq!((list.first(), list.last()), (Some(&1), Some(&4)));
    list[NodeId::new(0)] = 9;
    assert_eq!(list[NodeId::new(0)], 9);
}
