//! The graphs of one layout, and the id that names one of them.
//!
//! ELK hangs a nested graph on its node (`LNode#getNestedGraph`). Rust cannot mutate a child and
//! its parent through such a link at the same time, and a graph reached by a path of indices or
//! by the caller's node id is easy to get wrong. One [`GraphArena`] owns every graph of the
//! hierarchy instead. A node holds the [`GraphId`] of the graph inside it, and a graph never
//! leaves its slot.

use std::collections::VecDeque;
use std::ops::{Index, IndexMut};

use super::{LGraph, NodeId};

/// Names one graph of the hierarchy: the root or the graph inside a node.
///
/// Ids come from [`GraphArena`] and never change. A graph that is not in an arena yet is its own
/// root and has the id [`GraphId::ROOT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GraphId(usize);

impl GraphId {
    /// The root graph of a hierarchy.
    pub const ROOT: GraphId = GraphId(0);

    /// The id of the graph that joined the arena `index`th. The root is 0.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The number behind the id: the graph's slot in the arena.
    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

/// The node a graph lies in: ELK's `LGraph#getParentNode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphParent {
    /// The graph that holds the node.
    pub graph: GraphId,
    /// The node's index in that graph.
    pub node: NodeId,
}

/// Every graph of one layout. The root sits in slot [`GraphId::ROOT`].
///
/// Slot order is the order graphs joined. Never walk the slots for a result: ELK visits the
/// hierarchy in an order of its own, and [`preorder`](Self::preorder),
/// [`breadth_first`](Self::breadth_first) and [`children`](Self::children) give those orders.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphArena {
    graphs: Vec<LGraph>,
}

impl GraphArena {
    /// An arena whose root is `root`.
    pub fn new(mut root: LGraph) -> Self {
        root.id = GraphId::ROOT;
        Self { graphs: vec![root] }
    }

    /// Takes a graph into the arena and returns its id. The graph's own `id` is overwritten.
    fn add(&mut self, mut graph: LGraph) -> GraphId {
        let id = GraphId::new(self.graphs.len());
        graph.id = id;
        self.graphs.push(graph);
        id
    }

    /// Takes a graph into the arena as the graph inside `node` of `parent`, and returns its id.
    ///
    /// # Panics
    ///
    /// When `parent` has no such node. In a debug build, also when that node already holds a
    /// graph: a node owns at most one, and a second would orphan the first.
    pub fn nest(&mut self, parent: GraphId, node: NodeId, graph: LGraph) -> GraphId {
        debug_assert!(
            self.graphs[parent.0].layerless_nodes[node]
                .nested_graph
                .is_none(),
            "a node owns at most one nested graph"
        );
        let id = self.add(graph);
        self.graphs[id.0].parent = Some(GraphParent {
            graph: parent,
            node,
        });
        self.graphs[parent.0].layerless_nodes[node].nested_graph = Some(id);
        id
    }

    /// How many graphs the arena holds.
    pub fn len(&self) -> usize {
        self.graphs.len()
    }

    /// The graph `id` names, or `None` when this arena has no such graph. Use it for an id that
    /// came out of the data being processed, such as a record of a cross-hierarchy edge; indexing
    /// with a bad id panics.
    pub fn get(&self, id: GraphId) -> Option<&LGraph> {
        self.graphs.get(id.0)
    }

    /// The root graph.
    pub fn root(&self) -> &LGraph {
        &self.graphs[GraphId::ROOT.0]
    }

    /// The root graph, mutable.
    pub fn root_mut(&mut self) -> &mut LGraph {
        &mut self.graphs[GraphId::ROOT.0]
    }

    /// Every graph in slot order. Use it only where the order cannot reach a result.
    pub fn iter(&self) -> impl Iterator<Item = &LGraph> {
        self.graphs.iter()
    }

    /// Every graph, mutable, in slot order. Use it only where the order cannot reach a result.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut LGraph> {
        self.graphs.iter_mut()
    }

    /// The graphs inside the nodes of `graph`, in node order.
    pub fn children(&self, graph: GraphId) -> Vec<GraphId> {
        self.graphs[graph.0]
            .layerless_nodes
            .iter()
            .filter_map(|node| node.nested_graph)
            .collect()
    }

    /// `from` and all graphs below it, each graph before its children, children in node order.
    pub fn preorder(&self, from: GraphId) -> Vec<GraphId> {
        let mut order = Vec::new();
        let mut stack = vec![from];
        while let Some(graph) = stack.pop() {
            order.push(graph);
            stack.extend(self.children(graph).into_iter().rev());
        }
        order
    }

    /// `from` and all graphs below it, each graph after its children, children in node order.
    pub fn postorder(&self, from: GraphId) -> Vec<GraphId> {
        let mut order = Vec::new();
        let mut stack = vec![(from, false)];
        while let Some((graph, children_done)) = stack.pop() {
            if children_done {
                order.push(graph);
            } else {
                stack.push((graph, true));
                stack.extend(self.children(graph).into_iter().rev().map(|c| (c, false)));
            }
        }
        order
    }

    /// `from` and all graphs below it in the order ELK's `collectAllGraphsBottomUp` finds them:
    /// a stack takes the graph found last first, and a graph's children join the list in node
    /// order. Run in reverse, the list gives the order ELK lays the graphs out in. It is neither
    /// a pre-order nor a post-order walk: sibling subtrees interleave.
    pub fn discovery_order(&self, from: GraphId) -> Vec<GraphId> {
        let mut order = vec![from];
        let mut search = vec![from];
        while let Some(graph) = search.pop() {
            for child in self.children(graph) {
                order.push(child);
                search.push(child);
            }
        }
        order
    }

    /// `from` and all graphs below it, level by level, children in node order.
    pub fn breadth_first(&self, from: GraphId) -> Vec<GraphId> {
        let mut order = Vec::new();
        let mut queue = VecDeque::from([from]);
        while let Some(graph) = queue.pop_front() {
            order.push(graph);
            queue.extend(self.children(graph));
        }
        order
    }

    /// Calls `visit` on the root and then on every graph below it in [`preorder`](Self::preorder),
    /// and stops at the first error. The order is fixed before the first call.
    pub fn try_for_each_graph_mut<E>(
        &mut self,
        mut visit: impl FnMut(&mut LGraph) -> Result<(), E>,
    ) -> Result<(), E> {
        for graph in self.preorder(GraphId::ROOT) {
            visit(&mut self.graphs[graph.0])?;
        }
        Ok(())
    }

    /// Writes the graph properties found during import into each graph's options.
    pub fn sync_graph_properties_to_options(&mut self) {
        for graph in &mut self.graphs {
            graph.graph_properties.apply_to_options(&mut graph.options);
        }
    }

    /// Two different graphs, both mutable: a child and its parent, for instance.
    ///
    /// # Panics
    ///
    /// When `first` and `second` are the same graph.
    pub fn pair_mut(&mut self, first: GraphId, second: GraphId) -> (&mut LGraph, &mut LGraph) {
        assert_ne!(first, second, "a graph cannot be borrowed twice");
        if first.0 < second.0 {
            let (low, high) = self.graphs.split_at_mut(second.0);
            (&mut low[first.0], &mut high[0])
        } else {
            let (low, high) = self.graphs.split_at_mut(first.0);
            (&mut high[0], &mut low[second.0])
        }
    }

    /// One graph, mutable, with a read-only view of every other graph.
    pub fn split_mut(&mut self, id: GraphId) -> (&mut LGraph, OtherGraphs<'_>) {
        let (before, rest) = self.graphs.split_at_mut(id.0);
        let (graph, after) = rest
            .split_first_mut()
            .expect("a graph id names a slot of its arena");
        (
            graph,
            OtherGraphs {
                before,
                after,
                hole: id.0,
            },
        )
    }
}

impl Index<GraphId> for GraphArena {
    type Output = LGraph;

    fn index(&self, id: GraphId) -> &LGraph {
        &self.graphs[id.0]
    }
}

impl IndexMut<GraphId> for GraphArena {
    fn index_mut(&mut self, id: GraphId) -> &mut LGraph {
        &mut self.graphs[id.0]
    }
}

/// A read-only view of the graphs of an arena except one. Code that works on one graph reads the
/// graph inside one of its nodes through it.
#[derive(Debug, Clone, Copy)]
pub struct OtherGraphs<'a> {
    before: &'a [LGraph],
    after: &'a [LGraph],
    hole: usize,
}

impl<'a> OtherGraphs<'a> {
    /// A view of no graph, for a graph that has no nested graphs.
    #[cfg(test)]
    pub const fn none() -> OtherGraphs<'static> {
        OtherGraphs {
            before: &[],
            after: &[],
            hole: usize::MAX,
        }
    }

    /// The graph `id` names, unless it is the one the view leaves out.
    pub fn get(&self, id: GraphId) -> Option<&'a LGraph> {
        if id.0 < self.hole {
            self.before.get(id.0)
        } else if id.0 == self.hole {
            None
        } else {
            self.after.get(id.0 - self.hole - 1)
        }
    }
}

/// Tests read and change the root graph's fields on the arena directly. Production code names
/// the graph it means: `root()` or an id.
#[cfg(test)]
impl std::ops::Deref for GraphArena {
    type Target = LGraph;

    fn deref(&self) -> &LGraph {
        self.root()
    }
}

#[cfg(test)]
impl std::ops::DerefMut for GraphArena {
    fn deref_mut(&mut self) -> &mut LGraph {
        self.root_mut()
    }
}

#[cfg(test)]
mod tests;
