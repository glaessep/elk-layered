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

// Mutation: leave out the `debug_assert!` in `nest`. A second graph then replaces the first
// one in the node, and the first one stays in the arena with no node that owns it.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a node owns at most one nested graph")]
fn nesting_a_second_graph_in_the_same_node_panics() {
    let mut graphs = GraphArena::new(graph("root", 1));
    graphs.nest(
        GraphId::ROOT,
        crate::graph::NodeId::new(0),
        graph("first", 0),
    );
    graphs.nest(
        GraphId::ROOT,
        crate::graph::NodeId::new(0),
        graph("second", 0),
    );
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

    assert_eq!(
        graphs.root().layerless_nodes[NodeId::new(0)].nested_graph,
        Some(a)
    );
    assert_eq!(
        graphs[a].layerless_nodes[NodeId::new(0)].nested_graph,
        Some(a0)
    );
    assert_eq!(
        graphs[b].layerless_nodes[NodeId::new(0)].nested_graph,
        Some(b0)
    );
    for id in [a, b, a0, b0] {
        assert_eq!(graphs[id].id, id, "{}", graphs[id].name);
    }
    assert_eq!(graphs[a0].name, "a0");
}

// Mutation: make `get` index the graphs (`Some(&self.graphs[id.0])`). An id past the last
// graph then panics instead of giving `None`.
#[test]
fn get_gives_none_for_an_id_the_arena_does_not_hold() {
    let (graphs, [a, ..]) = branched();

    assert_eq!(graphs.get(a).map(|graph| graph.name.as_str()), Some("a"));
    assert!(graphs.get(GraphId::new(graphs.len())).is_none());
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
        dummy.set_kind(crate::graph::LNodeKind::ExternalPort);
        dummy.external_port_mut().unwrap().side = side;
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
        graph.layerless_nodes[NodeId::new(1)].ports[port.port].port_dummy = Some(GraphNodeRef {
            graph_id: dummy_graph,
            node: NodeId::new(0),
        });
        graph.layerless_nodes[NodeId::new(1)].ports[port.port].side = PortSide::Undefined;
        crate::intermediate::port_side_processor::set_port_side(graph, others, port);
        graph.layerless_nodes[NodeId::new(1)].ports[port.port].side
    };

    // Node 1 holds the second graph. Both graphs have one name, so only the id tells them apart.
    assert_eq!(side_of(&mut graphs, sides[1]), PortSide::South);
    // The first graph lies in node 0, not in node 1, so no side is read from it.
    assert_eq!(side_of(&mut graphs, sides[0]), PortSide::Undefined);
}
