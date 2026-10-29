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
mod tests;
