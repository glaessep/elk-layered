//! The ports of one node, and the id that names one of them.

use super::LPort;
use std::ops::{Index, IndexMut};

/// Names one port of one node. Two ports of the same node never share an id.
///
/// An id is not a position: `NodePorts::position` tells where the port stands in the node's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId(usize);

impl PortId {
    /// The id of the port that was added `index`th to its node.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// The ports of one node, in the node's port order (ELK's `LNode#getPorts()`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NodePorts {
    ports: Vec<LPort>,
}

impl NodePorts {
    /// How many ports the node lists.
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    /// Whether the node lists no port.
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// The ports in the node's order.
    pub fn iter(&self) -> std::slice::Iter<'_, LPort> {
        self.ports.iter()
    }

    /// One more than the largest id the node has given. Size a table keyed by `PortId::index` to
    /// this, not to `len`.
    pub fn slot_count(&self) -> usize {
        self.ports.len()
    }

    /// Every port with its id, for code whose result cannot depend on the order.
    pub fn slots_mut_with_ids(&mut self) -> impl Iterator<Item = (PortId, &mut LPort)> {
        self.ports
            .iter_mut()
            .enumerate()
            .map(|(index, port)| (PortId(index), port))
    }

    /// The first port of the node's order.
    pub fn first(&self) -> Option<&LPort> {
        self.ports.first()
    }

    /// The first port of the node's order.
    pub fn first_mut(&mut self) -> Option<&mut LPort> {
        self.ports.first_mut()
    }

    /// The last port of the node's order.
    pub fn last(&self) -> Option<&LPort> {
        self.ports.last()
    }

    /// Every port, for code whose result cannot depend on the order.
    pub fn slots_mut(&mut self) -> std::slice::IterMut<'_, LPort> {
        self.ports.iter_mut()
    }

    /// The ids of the listed ports in the node's order.
    pub fn ids(&self) -> impl DoubleEndedIterator<Item = PortId> + ExactSizeIterator + Clone {
        (0..self.ports.len()).map(PortId)
    }

    /// The listed ports with their ids, in the node's order.
    pub fn iter_with_ids(&self) -> impl DoubleEndedIterator<Item = (PortId, &LPort)> {
        self.ports
            .iter()
            .enumerate()
            .map(|(index, port)| (PortId(index), port))
    }

    /// The port that has this id, if the node has one.
    pub fn get(&self, id: PortId) -> Option<&LPort> {
        self.ports.get(id.0)
    }

    /// The port that has this id, if the node has one.
    pub fn get_mut(&mut self, id: PortId) -> Option<&mut LPort> {
        self.ports.get_mut(id.0)
    }

    /// The id of the port at this place in the node's order.
    pub fn id_at(&self, position: usize) -> Option<PortId> {
        (position < self.ports.len()).then_some(PortId(position))
    }

    /// The place of this port in the node's order.
    pub fn position(&self, id: PortId) -> Option<usize> {
        (id.0 < self.ports.len()).then_some(id.0)
    }

    /// Adds a port at the end of the node's order and returns its id.
    pub fn push(&mut self, port: LPort) -> PortId {
        self.ports.push(port);
        PortId(self.ports.len() - 1)
    }

    /// Takes the ports from place `at` on out of the list.
    pub(crate) fn split_off(&mut self, at: usize) -> Vec<LPort> {
        self.ports.split_off(at)
    }

    /// Adds ports at the end of the list.
    pub(crate) fn append(&mut self, ports: &mut Vec<LPort>) {
        self.ports.append(ports);
    }

    /// Removes every port.
    pub fn clear(&mut self) {
        self.ports.clear();
    }

    /// Puts the listed ports in this order. Returns `false` and changes nothing unless `order`
    /// names each listed port exactly once.
    pub fn set_order(&mut self, order: &[PortId]) -> bool {
        if order.len() != self.ports.len() {
            return false;
        }
        let mut seen = vec![false; self.ports.len()];
        for id in order {
            if id.0 >= seen.len() || seen[id.0] {
                return false;
            }
            seen[id.0] = true;
        }
        let mut old = std::mem::take(&mut self.ports)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        self.ports = order.iter().filter_map(|id| old[id.0].take()).collect();
        true
    }
}

impl Index<PortId> for NodePorts {
    type Output = LPort;

    fn index(&self, id: PortId) -> &LPort {
        &self.ports[id.0]
    }
}

impl IndexMut<PortId> for NodePorts {
    fn index_mut(&mut self, id: PortId) -> &mut LPort {
        &mut self.ports[id.0]
    }
}

impl<'a> IntoIterator for &'a NodePorts {
    type Item = &'a LPort;
    type IntoIter = std::slice::Iter<'a, LPort>;

    fn into_iter(self) -> Self::IntoIter {
        self.ports.iter()
    }
}

/// The ids with these numbers, for tests that name ports by the order they were added in.
#[cfg(test)]
pub(crate) fn port_ids<const N: usize>(numbers: [usize; N]) -> [PortId; N] {
    numbers.map(PortId::new)
}
