//! The graphs of one layout, and the id that names one of them.
//!
//! ELK hangs a nested graph on its node (`LNode#getNestedGraph`). Rust cannot mutate a child and
//! its parent through such a link at the same time, and a graph reached by a path of indices or
//! by the caller's node id is easy to get wrong. One [`GraphArena`] owns every graph of the
//! hierarchy instead. A node holds the [`GraphId`] of the graph inside it, and a graph never
//! leaves its slot.

use super::{LGraph, NodeId};
use std::collections::VecDeque;
use std::ops::{Index, IndexMut};

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
    pub fn add(&mut self, mut graph: LGraph) -> GraphId {
        let id = GraphId::new(self.graphs.len());
        graph.id = id;
        self.graphs.push(graph);
        id
    }

    /// Takes a graph into the arena as the graph inside `node` of `parent`, and returns its id.
    ///
    /// # Panics
    ///
    /// When `parent` has no such node.
    pub fn nest(&mut self, parent: GraphId, node: NodeId, graph: LGraph) -> GraphId {
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

    /// The root graph.
    pub fn root(&self) -> &LGraph {
        &self.graphs[GraphId::ROOT.0]
    }

    /// The root graph, mutable.
    pub fn root_mut(&mut self) -> &mut LGraph {
        &mut self.graphs[GraphId::ROOT.0]
    }

    /// Gives the root graph back and drops the rest.
    #[cfg(test)]
    pub fn into_root(mut self) -> LGraph {
        self.graphs.swap_remove(GraphId::ROOT.0)
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

    /// A read-only view of every graph but `except`.
    pub fn others(&self, except: GraphId) -> OtherGraphs<'_> {
        OtherGraphs {
            before: &self.graphs[..except.0],
            after: &self.graphs[except.0 + 1..],
            hole: except.0,
        }
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
mod tests {
    use super::*;
    use crate::graph::{GraphNodeRef, LNode, PortSide};
    use crate::options::LayeredOptions;

    fn graph(name: &str, node_count: usize) -> LGraph {
        let mut graph = LGraph::new(name, LayeredOptions::default());
        for index in 0..node_count {
            graph
                .layerless_nodes
                .push(LNode::new(format!("{name}-{index}"), 1.0, 1.0, None));
        }
        graph
    }

    /// root (2 nodes) -> a (in node 0, 2 nodes) -> a0 (in a's node 0), a1 (in a's node 1)
    ///                -> b (in node 1, 1 node)  -> b0 (in b's node 0)
    fn branched() -> (GraphArena, [GraphId; 5]) {
        let mut graphs = GraphArena::new(graph("root", 2));
        let a = graphs.nest(GraphId::ROOT, NodeId::new(0), graph("a", 2));
        let b = graphs.nest(GraphId::ROOT, NodeId::new(1), graph("b", 1));
        let a0 = graphs.nest(a, NodeId::new(0), graph("a0", 0));
        let a1 = graphs.nest(a, NodeId::new(1), graph("a1", 0));
        let b0 = graphs.nest(b, NodeId::new(0), graph("b0", 0));
        let _ = (a0, a1);
        (graphs, [a, b, a0, a1, b0])
    }

    fn names(graphs: &GraphArena, ids: &[GraphId]) -> Vec<String> {
        ids.iter().map(|&id| graphs[id].name.clone()).collect()
    }

    // Mutation: let `nest` leave the graph's own id as `GraphId::ROOT`.
    #[test]
    fn a_nested_graph_is_reached_by_its_id_from_every_level() {
        let (graphs, [a, b, a0, _, b0]) = branched();

        assert_eq!(graphs.root().layerless_nodes[0].nested_graph, Some(a));
        assert_eq!(graphs[a].layerless_nodes[0].nested_graph, Some(a0));
        assert_eq!(graphs[b].layerless_nodes[0].nested_graph, Some(b0));
        for id in [a, b, a0, b0] {
            assert_eq!(graphs[id].id, id, "{}", graphs[id].name);
        }
        assert_eq!(graphs[a0].name, "a0");
    }

    // Mutation: let `add` give the new graph the id of the previous one.
    #[test]
    fn two_graphs_with_one_name_have_two_ids() {
        let mut graphs = GraphArena::new(graph("root", 2));
        let first = graphs.nest(GraphId::ROOT, NodeId::new(0), graph("same", 0));
        let second = graphs.nest(GraphId::ROOT, NodeId::new(1), graph("same", 0));

        assert_ne!(first, second);
        assert_eq!(graphs[first].name, graphs[second].name);
        let ref_to = |graph_id| GraphNodeRef {
            graph_id,
            node: NodeId::new(0),
        };
        assert_ne!(ref_to(first), ref_to(second));
    }

    // Mutation: list the children of a graph in reverse node order.
    #[test]
    fn the_walk_orders_follow_the_node_order() {
        let (graphs, [a, b, a0, a1, b0]) = branched();

        assert_eq!(graphs.children(GraphId::ROOT), [a, b]);
        assert_eq!(
            names(&graphs, &graphs.preorder(GraphId::ROOT)),
            ["root", "a", "a0", "a1", "b", "b0"]
        );
        assert_eq!(
            names(&graphs, &graphs.postorder(GraphId::ROOT)),
            ["a0", "a1", "a", "b0", "b", "root"]
        );
        assert_eq!(
            names(&graphs, &graphs.breadth_first(GraphId::ROOT)),
            ["root", "a", "b", "a0", "a1", "b0"]
        );
        let _ = (a0, a1, b0);
    }

    // Mutation: search the graph found first before the one found last, as a queue would.
    #[test]
    fn the_discovery_order_takes_the_graph_found_last_first_like_elk() {
        let (graphs, _) = branched();

        // ELK pushes the children of a graph in node order and pops the last one: `b` is searched
        // before `a`, so `b0` is found before `a0` and `a1`.
        assert_eq!(
            names(&graphs, &graphs.discovery_order(GraphId::ROOT)),
            ["root", "a", "b", "b0", "a0", "a1"]
        );
    }

    // Mutation: return the graph with the lower slot first, whatever the arguments say.
    #[test]
    fn pair_mut_gives_two_different_graphs_in_the_order_asked() {
        let (mut graphs, [a, b, ..]) = branched();

        for (first, second, widths) in [(a, b, (1.0, 2.0)), (b, a, (3.0, 4.0))] {
            let (one, two) = graphs.pair_mut(first, second);
            assert_eq!(one.id, first);
            assert_eq!(two.id, second);
            one.size.width = widths.0;
            two.size.width = widths.1;
        }
        // The second call wrote last: `b` was first and got 3, `a` was second and got 4.
        assert_eq!(graphs[a].size.width, 4.0);
        assert_eq!(graphs[b].size.width, 3.0);
    }

    // Mutation: remove the assert in `pair_mut`.
    #[test]
    #[should_panic(expected = "borrowed twice")]
    fn pair_mut_refuses_one_graph_twice() {
        let (mut graphs, [a, ..]) = branched();
        graphs.pair_mut(a, a);
    }

    // Mutation: let `OtherGraphs::get` answer for the graph the view leaves out.
    #[test]
    fn split_mut_leaves_the_graph_out_of_the_view_of_the_others() {
        let (mut graphs, [a, b, a0, ..]) = branched();

        let (graph, others) = graphs.split_mut(a);

        assert_eq!(graph.id, a);
        assert!(others.get(a).is_none());
        for other in [GraphId::ROOT, b, a0] {
            assert_eq!(others.get(other).map(|g| g.id), Some(other));
        }
        assert!(graphs.others(a).get(a).is_none());
    }

    // Mutation: subtract from the id in `OtherGraphs::get` before the view is checked for being
    // empty.
    #[test]
    fn an_empty_view_answers_for_no_graph() {
        let none = OtherGraphs::none();

        assert!(none.get(GraphId::ROOT).is_none());
        assert!(none.get(GraphId::new(3)).is_none());
    }

    // Mutation: visit the graphs in slot order, not in pre-order.
    #[test]
    fn try_for_each_graph_mut_visits_in_pre_order_and_stops_at_the_first_error() {
        let (mut graphs, _) = branched();
        let mut seen = Vec::new();

        let result = graphs.try_for_each_graph_mut(|graph| {
            seen.push(graph.name.clone());
            if graph.name == "a1" {
                Err(graph.name.clone())
            } else {
                Ok(())
            }
        });

        assert_eq!(result, Err("a1".to_string()));
        assert_eq!(seen, ["root", "a", "a0", "a1"]);
    }

    // Mutation: read the side of the dummy from the first nested graph in the port's node, and
    // ignore the graph the dummy ref names.
    #[test]
    fn a_dummy_in_a_graph_that_is_not_inside_the_ports_node_is_not_read() {
        let mut graphs = GraphArena::new(graph("root", 2));
        let mut sides = Vec::new();
        for (position, side) in [PortSide::North, PortSide::South].into_iter().enumerate() {
            let mut nested = graph("same", 0);
            let mut dummy = LNode::new("external", 0.0, 0.0, None);
            dummy.external_port_side = side;
            nested.layerless_nodes.push(dummy);
            sides.push(graphs.nest(GraphId::ROOT, NodeId::new(position), nested));
        }
        let port = graphs
            .root_mut()
            .add_port(
                NodeId::new(1),
                crate::graph::PortType::Output,
                PortSide::Undefined,
                Default::default(),
            )
            .unwrap();
        let side_of = |graphs: &mut GraphArena, dummy_graph: GraphId| {
            let (graph, others) = graphs.split_mut(GraphId::ROOT);
            graph.layerless_nodes[1].ports[port.port].port_dummy = Some(GraphNodeRef {
                graph_id: dummy_graph,
                node: NodeId::new(0),
            });
            graph.layerless_nodes[1].ports[port.port].side = PortSide::Undefined;
            crate::p3order::set_port_side(graph, others, port);
            graph.layerless_nodes[1].ports[port.port].side
        };

        // Node 1 holds the second graph. Both graphs have one name, so only the id tells them apart.
        assert_eq!(side_of(&mut graphs, sides[1]), PortSide::South);
        // The first graph lies in node 0, not in node 1, so no side is read from it.
        assert_eq!(side_of(&mut graphs, sides[0]), PortSide::Undefined);
    }
}
