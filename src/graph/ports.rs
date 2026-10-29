//! The ports of one node, and the id that names one of them.

use super::LPort;
use std::ops::{Index, IndexMut};

/// Names one port of one node. Two ports of the same node never share an id, and the id of a
/// port never changes: reordering the node's ports leaves every id as it was.
///
/// An id is not a place in the node's list. Ids order by when they were made, which is no order
/// ELK knows; only code that needs some fixed order, such as a sort for determinism, may use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PortId(usize);

impl PortId {
    /// The id of the port that was added `index`th to its node. The first port added is 0.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id. Use it to key a table that is sized by
    /// `NodePorts::slot_count`, never as a place in the node's list.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// The ports of one node.
///
/// The ports sit in `slots`, which never reorders, so a [`PortId`] keeps naming the same port.
/// `order` is the node's port list (ELK's `LNode#getPorts()`): the ids the node lists, in the
/// node's order. Reordering only rewrites `order`. A port taken off the list stays in its slot,
/// so edges and holders that name it still find it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodePorts {
    slots: Vec<LPort>,
    order: Vec<PortId>,
}

/// The listed ports in the node's order. See [`NodePorts::iter`].
#[derive(Debug, Clone)]
pub struct PortIter<'a> {
    slots: &'a [LPort],
    ids: std::slice::Iter<'a, PortId>,
}

impl<'a> Iterator for PortIter<'a> {
    type Item = &'a LPort;

    fn next(&mut self) -> Option<&'a LPort> {
        self.ids.next().map(|id| &self.slots[id.0])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.ids.size_hint()
    }
}

impl DoubleEndedIterator for PortIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.ids.next_back().map(|id| &self.slots[id.0])
    }
}

impl ExactSizeIterator for PortIter<'_> {}

impl NodePorts {
    /// How many ports the node lists.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether the node lists no port.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The listed ports in the node's order.
    pub fn iter(&self) -> PortIter<'_> {
        PortIter {
            slots: &self.slots,
            ids: self.order.iter(),
        }
    }

    /// The ids of the listed ports in the node's order.
    pub fn ids(&self) -> impl DoubleEndedIterator<Item = PortId> + ExactSizeIterator + Clone {
        self.order.iter().copied()
    }

    /// Calls `visit` on each listed port in the node's order, so a loop that only changes the
    /// ports needs no copy of the ids. `visit` cannot list, unlist or reorder ports.
    pub fn for_each_listed_mut(&mut self, mut visit: impl FnMut(&mut LPort)) {
        for id in &self.order {
            visit(&mut self.slots[id.0]);
        }
    }

    /// The listed ports with their ids, in the node's order.
    pub fn iter_with_ids(&self) -> impl DoubleEndedIterator<Item = (PortId, &LPort)> {
        self.order.iter().map(|id| (*id, &self.slots[id.0]))
    }

    /// The port that has this id, whether the node lists it or not.
    pub fn get(&self, id: PortId) -> Option<&LPort> {
        self.slots.get(id.0)
    }

    /// The port that has this id, whether the node lists it or not.
    pub fn get_mut(&mut self, id: PortId) -> Option<&mut LPort> {
        self.slots.get_mut(id.0)
    }

    /// The id of the port at this place in the node's order.
    pub fn id_at(&self, position: usize) -> Option<PortId> {
        self.order.get(position).copied()
    }

    /// The place of this port in the node's order, or `None` if the node does not list it.
    #[cfg(test)]
    pub fn position(&self, id: PortId) -> Option<usize> {
        self.order.iter().position(|listed| *listed == id)
    }

    /// One more than the largest id the node has given. Size a table keyed by `PortId::index` to
    /// this, not to `len`: a port off the list still has its slot.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Every port, listed or not, for code whose result cannot depend on the order.
    pub fn slots_mut(&mut self) -> std::slice::IterMut<'_, LPort> {
        self.slots.iter_mut()
    }

    /// Every port with its id, listed or not, for code whose result cannot depend on the order.
    pub fn slots_mut_with_ids(&mut self) -> impl Iterator<Item = (PortId, &mut LPort)> {
        self.slots
            .iter_mut()
            .enumerate()
            .map(|(index, port)| (PortId(index), port))
    }

    /// The first port of the node's order.
    pub fn first(&self) -> Option<&LPort> {
        self.order.first().map(|id| &self.slots[id.0])
    }

    /// The first port of the node's order.
    pub fn first_mut(&mut self) -> Option<&mut LPort> {
        let id = self.order.first()?;
        self.slots.get_mut(id.0)
    }

    /// The last port of the node's order.
    #[cfg(test)]
    pub fn last(&self) -> Option<&LPort> {
        self.order.last().map(|id| &self.slots[id.0])
    }

    /// Adds a port at the end of the node's order and returns its id.
    pub fn push(&mut self, port: LPort) -> PortId {
        let id = PortId(self.slots.len());
        self.slots.push(port);
        self.order.push(id);
        id
    }

    /// Takes these ports off the node's list. They keep their slots and their ids. A port the
    /// list does not hold is ignored.
    pub fn unlist(&mut self, ports: &[PortId]) {
        self.order.retain(|id| !ports.contains(id));
    }

    /// Puts ports that were taken off the list back at its end, in the given order. A port the
    /// list holds already, or that has no slot, is ignored.
    pub fn relist(&mut self, ports: &[PortId]) {
        for id in ports {
            if id.0 < self.slots.len() && !self.order.contains(id) {
                self.order.push(*id);
            }
        }
    }

    /// Removes every port. Only tests call it: the next `push` would give out `PortId::new(0)`
    /// again, and a port id must stay the name of one port for the life of its node.
    #[cfg(test)]
    pub fn clear(&mut self) {
        self.slots.clear();
        self.order.clear();
    }

    /// Puts the listed ports in this order. Returns `false` and changes nothing unless `order`
    /// names each listed port exactly once. No port changes its id.
    ///
    /// The list takes `order` as it is, with no copy. An order that is already the list's order
    /// costs one comparison, which is the common case when a sort leaves the ports where they were.
    pub fn set_order(&mut self, order: Vec<PortId>) -> bool {
        if order == self.order {
            return true;
        }
        if order.len() != self.order.len() {
            return false;
        }
        let mut listed = vec![false; self.slots.len()];
        for id in &self.order {
            listed[id.0] = true;
        }
        for id in &order {
            // A listed port is cleared as it is named, so a second naming of it fails.
            if !listed.get(id.0).copied().unwrap_or(false) {
                return false;
            }
            listed[id.0] = false;
        }
        self.order = order;
        true
    }
}

impl Index<PortId> for NodePorts {
    type Output = LPort;

    fn index(&self, id: PortId) -> &LPort {
        &self.slots[id.0]
    }
}

impl IndexMut<PortId> for NodePorts {
    fn index_mut(&mut self, id: PortId) -> &mut LPort {
        &mut self.slots[id.0]
    }
}

impl<'a> IntoIterator for &'a NodePorts {
    type Item = &'a LPort;
    type IntoIter = PortIter<'a>;

    fn into_iter(self) -> PortIter<'a> {
        self.iter()
    }
}

/// The ids with these numbers, for tests that name ports by the order they were added in.
#[cfg(test)]
pub(crate) fn port_ids<const N: usize>(numbers: [usize; N]) -> [PortId; N] {
    numbers.map(PortId::new)
}

#[cfg(test)]
mod tests;
