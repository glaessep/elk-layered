//! The graphs of one layout, and the id that names one of them.

/// Names one graph of the hierarchy: the root or the graph inside a node.
///
/// Ids come from [`GraphArena`](super::GraphArena) and never change. A graph that is not in an
/// arena yet is its own root and has the id [`GraphId::ROOT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GraphId(usize);

impl GraphId {
    /// The root graph of a hierarchy.
    pub const ROOT: GraphId = GraphId(0);

    /// The id of the graph that joined the arena `index`th. The root is 0.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id: the graph's place in the arena.
    #[allow(dead_code)] // The arena uses it next.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}
