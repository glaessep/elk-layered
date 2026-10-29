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
