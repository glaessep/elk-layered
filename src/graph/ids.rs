//! The ids that name a node or an edge inside one graph.

/// Names one node of one graph: its place in `LGraph::layerless_nodes`. An id is valid until the
/// graph is split into components or combined again, which renumber every node. An id is not a
/// layer index and not a count.
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

#[cfg(test)]
mod tests;
