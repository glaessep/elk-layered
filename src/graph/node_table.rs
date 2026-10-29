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
        // The rows before it were not written.
        assert_eq!(table.get(NodeId::new(0)), None);
        assert_eq!(table.get(NodeId::new(3)), None);
    }

    // Mutation: in `row_mut`, create a default row for every node before the one asked for.
    #[test]
    fn writing_a_row_leaves_the_rows_before_it_missing() {
        let mut table = NodeTable::<Vec<usize>>::default();
        table.row_mut(NodeId::new(1));

        assert_eq!(table.get(NodeId::new(1)), Some(&vec![]));
        assert_eq!(table.get(NodeId::new(0)), None);
    }

    // Mutation: in `get_or_default`, answer the default row also for a node that has a row.
    #[test]
    fn get_or_default_answers_the_row_or_the_default() {
        let mut table = NodeTable::<Vec<usize>>::default();
        table.row_mut(NodeId::new(1)).push(4);

        assert_eq!(table.get_or_default(NodeId::new(1)), vec![4]);
        assert_eq!(table.get_or_default(NodeId::new(0)), Vec::<usize>::new());
        assert_eq!(table.get_or_default(NodeId::new(9)), Vec::<usize>::new());
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

    // Mutation: in `subset`, give a node without a row a default row.
    #[test]
    fn subset_keeps_a_node_without_a_row_without_one() {
        let mut table = NodeTable::<Vec<usize>>::default();
        table.row_mut(NodeId::new(0)).push(5);

        let subset = table.subset(&nodes(&[0, 9, 0]));

        assert_eq!(subset.get(NodeId::new(0)), Some(&vec![5]));
        assert_eq!(subset.get(NodeId::new(1)), None);
        assert_eq!(subset.get(NodeId::new(2)), Some(&vec![5]));
    }

    // Mutation: in `absorb`, put each row at its own index, not at `node_map[local]`.
    #[test]
    fn absorb_moves_each_row_to_the_node_the_map_names() {
        let mut part = NodeTable::<Vec<usize>>::default();
        part.row_mut(NodeId::new(0)).push(10);
        part.row_mut(NodeId::new(2)).push(12);
        let mut table = NodeTable::<Vec<usize>>::default();
        table.row_mut(NodeId::new(5)).push(99);

        table.absorb(&nodes(&[3, 4, 1]), part);

        assert_eq!(table.get(NodeId::new(3)), Some(&vec![10]));
        assert_eq!(table.get(NodeId::new(1)), Some(&vec![12]));
        assert_eq!(table.get(NodeId::new(4)), None);
        // A row that is already here stays.
        assert_eq!(table.get(NodeId::new(5)), Some(&vec![99]));
    }

    // Mutation: in `absorb`, move a missing row too, as a default row.
    #[test]
    fn absorb_leaves_a_node_without_a_row_without_one() {
        let mut part = NodeTable::<Vec<usize>>::default();
        part.row_mut(NodeId::new(1)).push(1);
        let mut table = NodeTable::<Vec<usize>>::default();

        table.absorb(&nodes(&[7, 8]), part);

        assert_eq!(table.get(NodeId::new(7)), None);
        assert_eq!(table.get(NodeId::new(8)), Some(&vec![1]));
    }
}
