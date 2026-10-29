//! A list that a typed id indexes, and a plain number does not.

use super::{EdgeId, NodeId};
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

/// An id type that names a place in an [`IdVec`].
pub trait VecId: Copy {
    /// The id of the item that stands `index`th.
    fn from_index(index: usize) -> Self;
    /// The place the id names.
    fn to_index(self) -> usize;
}

impl VecId for NodeId {
    fn from_index(index: usize) -> Self {
        NodeId::new(index)
    }

    fn to_index(self) -> usize {
        self.index()
    }
}

impl VecId for EdgeId {
    fn from_index(index: usize) -> Self {
        EdgeId::new(index)
    }

    fn to_index(self) -> usize {
        self.index()
    }
}

/// A `Vec<T>` that only an id of type `I` indexes. It has no `Index<usize>` and does not deref to
/// `Vec`, so a stray number cannot name an item. Library code has no way to the plain list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdVec<I, T> {
    items: Vec<T>,
    _id: PhantomData<fn(I) -> I>,
}

impl<I, T> Default for IdVec<I, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            _id: PhantomData,
        }
    }
}

impl<I: VecId, T> IdVec<I, T> {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// A list of `len` copies of `value`, one per id from 0 up to `len - 1`. Side tables use it:
    /// `IdVec::filled(graph.layerless_nodes.len(), false)` has one flag for each node.
    pub fn filled(len: usize, value: T) -> Self
    where
        T: Clone,
    {
        vec![value; len].into()
    }

    /// Sets every item to a copy of `value`. The ids stay as they are: `visited.fill(false)` clears
    /// a flag table between two passes without a loop over numbers.
    pub fn fill(&mut self, value: T)
    where
        T: Clone,
    {
        self.items.fill(value);
    }

    /// The item with this id, if the list holds one.
    pub fn get(&self, id: I) -> Option<&T> {
        self.items.get(id.to_index())
    }

    /// The item with this id, for changing, if the list holds one.
    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        self.items.get_mut(id.to_index())
    }

    /// Adds an item at the end and returns the id it got.
    pub fn push(&mut self, item: T) -> I {
        let id = self.next_id();
        self.items.push(item);
        id
    }

    /// The id the next pushed item gets.
    pub fn next_id(&self) -> I {
        I::from_index(self.items.len())
    }

    /// How many items the list holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the list holds no item.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The items in order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }

    /// The items in order, for changing.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.items.iter_mut()
    }

    /// The ids of all items, in order. The iterator does not borrow the list, so a loop over it may
    /// change the list. Items added in the loop are not visited.
    pub fn ids(&self) -> impl DoubleEndedIterator<Item = I> + ExactSizeIterator + use<I, T> {
        (0..self.items.len()).map(I::from_index)
    }

    /// Every item with its id, in order.
    pub fn iter_with_ids(&self) -> impl DoubleEndedIterator<Item = (I, &T)> + ExactSizeIterator {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| (I::from_index(index), item))
    }

    /// The first item, if any.
    pub fn first(&self) -> Option<&T> {
        self.items.first()
    }

    /// The last item, if any.
    pub fn last(&self) -> Option<&T> {
        self.items.last()
    }

    /// The items as a plain slice, for tests that compare a whole list. Library code has no way
    /// out of the typed ids: a place in a slice is a number, not an id.
    #[cfg(test)]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

impl<I: VecId, T> From<Vec<T>> for IdVec<I, T> {
    fn from(items: Vec<T>) -> Self {
        Self {
            items,
            _id: PhantomData,
        }
    }
}

impl<I: VecId, T> FromIterator<T> for IdVec<I, T> {
    fn from_iter<It: IntoIterator<Item = T>>(iter: It) -> Self {
        Vec::from_iter(iter).into()
    }
}

impl<I: VecId, T> Extend<T> for IdVec<I, T> {
    fn extend<It: IntoIterator<Item = T>>(&mut self, iter: It) {
        self.items.extend(iter);
    }
}

impl<I: VecId, T> Index<I> for IdVec<I, T> {
    type Output = T;

    fn index(&self, id: I) -> &T {
        &self.items[id.to_index()]
    }
}

impl<I: VecId, T> IndexMut<I> for IdVec<I, T> {
    fn index_mut(&mut self, id: I) -> &mut T {
        &mut self.items[id.to_index()]
    }
}

impl<I: VecId, T> IntoIterator for IdVec<I, T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, I: VecId, T> IntoIterator for &'a IdVec<I, T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a, I: VecId, T> IntoIterator for &'a mut IdVec<I, T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

#[cfg(test)]
mod tests {
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
}
