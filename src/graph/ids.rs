//! The ids that name a node or an edge inside one graph.

use super::{LNode, LayeredEdge};
use std::ops::{Index, IndexMut};

/// Names one node of one graph: its place in `LGraph::layerless_nodes`. A node keeps its id while
/// the graph lives. An id is not a layer index and not a count.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct NodeId(usize);

impl NodeId {
    /// The id of the node that stands `index`th in `LGraph::layerless_nodes`.
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id. Use it to size or key a table, or to step through the nodes in
    /// creation order.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Names one edge of one graph: its place in `LGraph::edges`. An id is not a count.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EdgeId(usize);

impl EdgeId {
    /// The id of the edge that stands `index`th in `LGraph::edges`.
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id. Use it to size or key a table, or to step through the edges in
    /// creation order.
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Looks a node up by id and says `None` for an id the graph does not hold. `Vec::get` takes a
/// number, so this stands in for it.
pub trait NodeSlice {
    /// The node with this id, if the graph holds one.
    fn get_node(&self, id: NodeId) -> Option<&LNode>;
    /// The node with this id, for changing, if the graph holds one.
    fn get_node_mut(&mut self, id: NodeId) -> Option<&mut LNode>;
}

impl NodeSlice for [LNode] {
    fn get_node(&self, id: NodeId) -> Option<&LNode> {
        self.get(id.0)
    }

    fn get_node_mut(&mut self, id: NodeId) -> Option<&mut LNode> {
        self.get_mut(id.0)
    }
}

/// Looks an edge up by id and says `None` for an id the graph does not hold.
pub trait EdgeSlice {
    /// The edge with this id, if the graph holds one.
    fn get_edge(&self, id: EdgeId) -> Option<&LayeredEdge>;
    /// The edge with this id, for changing, if the graph holds one.
    fn get_edge_mut(&mut self, id: EdgeId) -> Option<&mut LayeredEdge>;
}

impl EdgeSlice for [LayeredEdge] {
    fn get_edge(&self, id: EdgeId) -> Option<&LayeredEdge> {
        self.get(id.0)
    }

    fn get_edge_mut(&mut self, id: EdgeId) -> Option<&mut LayeredEdge> {
        self.get_mut(id.0)
    }
}

impl Index<EdgeId> for Vec<LayeredEdge> {
    type Output = LayeredEdge;

    fn index(&self, id: EdgeId) -> &LayeredEdge {
        &self[id.0]
    }
}

impl IndexMut<EdgeId> for Vec<LayeredEdge> {
    fn index_mut(&mut self, id: EdgeId) -> &mut LayeredEdge {
        &mut self[id.0]
    }
}

impl Index<NodeId> for Vec<LNode> {
    type Output = LNode;

    fn index(&self, id: NodeId) -> &LNode {
        &self[id.0]
    }
}

impl IndexMut<NodeId> for Vec<LNode> {
    fn index_mut(&mut self, id: NodeId) -> &mut LNode {
        &mut self[id.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::LGraph;
    use crate::options::LayeredOptions;

    fn three_nodes() -> LGraph {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        for id in ["a", "b", "c"] {
            graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
        }
        graph
    }

    // Mutation: return `self.0 + 1` from `NodeId::index`.
    #[test]
    fn an_id_gives_back_the_number_it_was_made_from() {
        assert_eq!(NodeId::new(0).index(), 0);
        assert_eq!(NodeId::new(7).index(), 7);
        assert_eq!(NodeId::new(usize::MAX).index(), usize::MAX);
    }

    // Mutation: make `Vec<LNode>[NodeId]` read slot 0 whatever the id is.
    #[test]
    fn indexing_by_id_reads_that_node() {
        let graph = three_nodes();
        assert_eq!(graph.layerless_nodes[NodeId::new(0)].id, "a");
        assert_eq!(graph.layerless_nodes[NodeId::new(2)].id, "c");
    }

    // Mutation: make `IndexMut<NodeId>` write slot `id + 1`.
    #[test]
    fn writing_by_id_changes_that_node_only() {
        let mut graph = three_nodes();
        graph.layerless_nodes[NodeId::new(1)].size.width = 99.0;
        let widths = graph
            .layerless_nodes
            .iter()
            .map(|node| node.size.width)
            .collect::<Vec<_>>();
        assert_eq!(widths, [10.0, 99.0, 10.0]);
    }

    // Mutation: make `get_node` answer `Some` for an id one past the end, or `get(id.0 + 1)`.
    #[test]
    fn get_node_says_none_for_an_id_the_graph_does_not_hold() {
        let graph = three_nodes();
        assert_eq!(
            graph.layerless_nodes.get_node(NodeId::new(0)).unwrap().id,
            "a"
        );
        assert_eq!(
            graph.layerless_nodes.get_node(NodeId::new(2)).unwrap().id,
            "c"
        );
        assert!(graph.layerless_nodes.get_node(NodeId::new(3)).is_none());
        assert!(
            graph
                .layerless_nodes
                .get_node(NodeId::new(usize::MAX))
                .is_none()
        );
        assert!(
            LGraph::new("empty", LayeredOptions::default())
                .layerless_nodes
                .get_node(NodeId::new(0))
                .is_none()
        );
    }

    // Mutation: make `get_node_mut` hand out slot `id + 1`.
    #[test]
    fn get_node_mut_reaches_that_node_and_refuses_an_unknown_id() {
        let mut graph = three_nodes();
        graph
            .layerless_nodes
            .get_node_mut(NodeId::new(1))
            .unwrap()
            .size
            .height = 5.0;
        assert_eq!(graph.layerless_nodes[NodeId::new(1)].size.height, 5.0);
        assert_eq!(graph.layerless_nodes[NodeId::new(0)].size.height, 10.0);
        assert!(graph.layerless_nodes.get_node_mut(NodeId::new(3)).is_none());
    }

    // Mutation: return `len + 1` from `next_node_id`, or `len - 1`.
    #[test]
    fn the_next_id_is_the_id_the_next_pushed_node_gets() {
        let mut graph = three_nodes();
        let next = graph.next_node_id();
        assert_eq!(next, NodeId::new(3));
        graph.layerless_nodes.push(LNode::new("d", 1.0, 1.0, None));
        assert_eq!(graph.layerless_nodes[next].id, "d");
        assert_eq!(
            LGraph::new("empty", LayeredOptions::default()).next_node_id(),
            NodeId::new(0)
        );
    }

    // Mutation: walk `node_ids` backwards, or stop one short.
    #[test]
    fn node_ids_run_in_creation_order_and_let_the_loop_change_the_graph() {
        let mut graph = three_nodes();
        let mut seen = Vec::new();
        for id in graph.node_ids() {
            seen.push(id.index());
            // The iterator does not borrow the graph. Nodes added in the loop are not visited.
            if id.index() == 0 {
                graph.layerless_nodes.push(LNode::new("d", 1.0, 1.0, None));
            }
        }
        assert_eq!(seen, [0, 1, 2]);
        assert_eq!(graph.node_ids().len(), 4);
    }

    // Mutation: pair each node with the id of its neighbour.
    #[test]
    fn nodes_with_ids_pair_each_node_with_its_own_id() {
        let graph = three_nodes();
        let pairs = graph
            .nodes_with_ids()
            .map(|(id, node)| (id.index(), node.id.clone()))
            .collect::<Vec<_>>();
        assert_eq!(
            pairs,
            [
                (0, "a".to_string()),
                (1, "b".to_string()),
                (2, "c".to_string())
            ]
        );
    }
}
