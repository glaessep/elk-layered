//! A side table: data about nodes that one processor owns, kept apart from the nodes.
//!
//! ELK hangs such values on the node as properties that only some nodes have. Here a processor
//! keeps them in a [`NodeTable`] of its own, typed by [`NodeId`]. A node that nobody wrote a row
//! for has no row, so a reader sees the difference between "nothing recorded" and a recorded
//! default.

use super::{IdVec, NodeId};

/// At most one row of type `T` per node. A row exists once something wrote it.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeTable<T> {
    rows: IdVec<NodeId, Option<T>>,
}

impl<T> Default for NodeTable<T> {
    fn default() -> Self {
        Self { rows: IdVec::new() }
    }
}

impl<T: Default + Clone> NodeTable<T> {
    /// The row of `node`, or `None` if nobody wrote one.
    pub fn get(&self, node: NodeId) -> Option<&T> {
        self.rows.get(node)?.as_ref()
    }

    /// A copy of the row of `node`, or the default row if nobody wrote one. Use it where "no
    /// row" and "the default row" mean the same.
    pub fn get_or_default(&self, node: NodeId) -> T {
        self.get(node).cloned().unwrap_or_default()
    }

    /// The row of `node` for change. It creates the row, as a default row, if it is missing. The
    /// rows of the nodes before it stay missing.
    pub fn row_mut(&mut self, node: NodeId) -> &mut T {
        while self.rows.len() <= node.index() {
            self.rows.push(None);
        }
        self.rows[node].get_or_insert_with(T::default)
    }

    /// The rows of the given nodes, in this order, as the table of a graph made from just those
    /// nodes. A node without a row stays without one.
    pub(crate) fn subset(&self, nodes: &IdVec<NodeId, NodeId>) -> Self {
        let rows = nodes
            .iter()
            .map(|node| self.rows.get(*node).cloned().flatten())
            .collect::<IdVec<NodeId, Option<T>>>();
        Self { rows }
    }

    /// Every row that exists, for a pass that renames the ids inside them.
    pub(crate) fn rows_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.rows.iter_mut().flatten()
    }

    /// Moves every row of `other` into this table: row `local` of `other` lands at
    /// `node_map[local]`. Rows already here stay unless a moved row takes their place. Used when
    /// the graphs of a component split merge again and their node ids change back.
    pub(crate) fn absorb(&mut self, node_map: &IdVec<NodeId, NodeId>, other: NodeTable<T>) {
        for (local, row) in other.rows.into_iter().enumerate() {
            let (Some(row), Some(&node)) = (row, node_map.get(NodeId::new(local))) else {
                continue;
            };
            while self.rows.len() <= node.index() {
                self.rows.push(None);
            }
            self.rows[node] = Some(row);
        }
    }
}

#[cfg(test)]
mod tests;
