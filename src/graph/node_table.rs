//! A side table: data about nodes that one processor owns, kept apart from the nodes.
//!
//! ELK hangs such values on the node as properties that only some nodes have. Here a processor
//! keeps them in a [`NodeTable`] of its own, typed by [`NodeId`]. A node that nobody wrote a row
//! for has no row, so a reader sees the difference between "nothing recorded" and a recorded
//! default.

use super::{IdVec, NodeId};

/// One row of type `T` per node, written when first needed.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeTable<T> {
    rows: IdVec<NodeId, T>,
}

impl<T> Default for NodeTable<T> {
    fn default() -> Self {
        Self { rows: IdVec::new() }
    }
}

impl<T: Default + Clone> NodeTable<T> {
    /// The row of `node`, or `None` if nobody wrote one.
    pub fn get(&self, node: NodeId) -> Option<&T> {
        self.rows.get(node)
    }

    /// The row of `node` for change. It creates the row, and the rows before it, when they are
    /// missing.
    pub fn row_mut(&mut self, node: NodeId) -> &mut T {
        while self.rows.len() <= node.index() {
            self.rows.push(T::default());
        }
        &mut self.rows[node]
    }

    /// The rows of the given nodes, in this order, as the table of a graph made from just those
    /// nodes. A node without a row gets a default row, as every row of a table before the last
    /// written one does.
    pub(crate) fn subset(&self, nodes: &IdVec<NodeId, NodeId>) -> Self {
        let rows = nodes
            .iter()
            .map(|node| self.rows.get(*node).cloned().unwrap_or_default())
            .collect::<IdVec<NodeId, T>>();
        Self { rows }
    }

    /// Every row, for a pass that renames the ids inside them.
    pub(crate) fn rows_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.rows.iter_mut()
    }

    /// The table of `len` nodes that puts each row of `parts` at the node it names. A node that
    /// no row names gets a default row.
    pub(crate) fn scattered(len: usize, parts: impl IntoIterator<Item = (NodeId, T)>) -> Self {
        let mut table = Self {
            rows: IdVec::filled(len, T::default()),
        };
        for (node, row) in parts {
            table.rows[node] = row;
        }
        table
    }

    /// The rows, one per node from 0 up, for [`Self::scattered`] to place again.
    pub(crate) fn into_rows(self) -> Vec<T> {
        self.rows.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(indexes: &[usize]) -> IdVec<NodeId, NodeId> {
        indexes.iter().map(|index| NodeId::new(*index)).collect()
    }

    // Mutation: in `row_mut`, return the row of node 0 for every node.
    #[test]
    fn a_row_exists_only_after_it_was_written() {
        let mut table = NodeTable::<Vec<usize>>::default();
        assert_eq!(table.get(NodeId::new(2)), None);

        table.row_mut(NodeId::new(2)).push(7);

        assert_eq!(table.get(NodeId::new(2)), Some(&vec![7]));
        // The rows before it exist and are empty.
        assert_eq!(table.get(NodeId::new(0)), Some(&vec![]));
        assert_eq!(table.get(NodeId::new(3)), None);
    }

    // Mutation: in `subset`, copy the rows in the order of the table, not in the order of `nodes`.
    #[test]
    fn subset_takes_the_rows_of_the_nodes_in_their_order() {
        let mut table = NodeTable::<Vec<usize>>::default();
        for index in 0..4 {
            table.row_mut(NodeId::new(index)).push(index * 10);
        }

        let subset = table.subset(&nodes(&[3, 1]));

        assert_eq!(subset.get(NodeId::new(0)), Some(&vec![30]));
        assert_eq!(subset.get(NodeId::new(1)), Some(&vec![10]));
        assert_eq!(subset.get(NodeId::new(2)), None);
    }

    // Mutation: in `subset`, answer a node past the end of the table with a panic.
    #[test]
    fn subset_gives_a_node_without_a_row_a_default_row() {
        let mut table = NodeTable::<Vec<usize>>::default();
        table.row_mut(NodeId::new(0)).push(5);

        let subset = table.subset(&nodes(&[0, 9]));

        assert_eq!(subset.get(NodeId::new(1)), Some(&vec![]));
    }

    // Mutation: in `scattered`, put the rows in the order they come, not at the node they name.
    #[test]
    fn scattered_puts_each_row_at_its_node() {
        let table = NodeTable::scattered(4, [(NodeId::new(3), vec![3]), (NodeId::new(1), vec![1])]);

        assert_eq!(table.into_rows(), vec![vec![], vec![1], vec![], vec![3]]);
    }
}
