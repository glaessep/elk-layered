use super::*;
use crate::graph::PortId;
use crate::graph::{GraphId, GraphNodeRef, LNode, LPoint, LayeredEdge, PortSide, PortType};
use crate::options::LayeredOptions;

// Mutation: leave the `origin_port` of a dummy's port out of `remap_references`. The second
// component keeps the node index it had in the whole graph, and that index is past its end.
#[test]
fn the_port_a_north_dummy_was_made_for_moves_with_its_component() {
    let mut graph = LGraph::new("g", LayeredOptions::default());
    // Node 0 is a component of its own. Nodes 1 and 2 are the second component: node 1 has a
    // north port, and node 2 is the dummy made for it.
    graph
        .layerless_nodes
        .push(LNode::new("alone", 10.0, 10.0, None));
    graph
        .layerless_nodes
        .push(LNode::new("a", 10.0, 10.0, None));
    let mut dummy = LNode::new("dummy", 0.0, 0.0, None);
    dummy.set_kind(crate::graph::LNodeKind::NorthSouthPort);
    graph.layerless_nodes.push(dummy);
    let north = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::North,
            LPoint::default(),
        )
        .unwrap();
    let west = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let out = graph
        .add_port(
            NodeId::new(2),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    graph.layerless_nodes[NodeId::new(2)].ports[out.port].origin_port = Some(north);
    graph.layerless_nodes[NodeId::new(1)].ports[north.port].port_dummy = Some(GraphNodeRef {
        graph_id: GraphId::ROOT,
        node: NodeId::new(2),
    });
    graph
        .add_edge({
            let mut built_edge = LayeredEdge::new("e".to_string(), out, west);
            built_edge.thickness = 1.0;
            built_edge
        })
        .unwrap();

    let components = split(&mut GraphArena::new(graph)).unwrap().unwrap();

    assert_eq!(components.len(), 2);
    let second = &components[1].graph;
    assert_eq!(second.layerless_nodes.len(), 2);
    // The dummy is node 1 here, and the node it was made for is node 0.
    assert_eq!(
        second.layerless_nodes[NodeId::new(1)].ports[PortId::new(0)].origin_port,
        Some(PortRef {
            node: NodeId::new(0),
            port: PortId::new(0)
        })
    );
    assert_eq!(
        second.layerless_nodes[NodeId::new(0)].ports[PortId::new(0)]
            .port_dummy
            .as_ref()
            .map(|dummy| dummy.node),
        Some(NodeId::new(1))
    );
}

fn edge_between(id: &str, source: PortRef, target: PortRef) -> LayeredEdge {
    let mut built_edge = LayeredEdge::new(id.to_string(), source, target);
    built_edge.thickness = 1.0;
    built_edge
}

// Mutation: leave out the `remap_nested_references` call in `split`. The nested graph then
// keeps the indices of the whole graph while its component has smaller ones. Leave out the
// call in `combine`: the nested graph keeps the component's indices after the nodes are back
// where they were.
#[test]
fn a_graph_inside_a_node_follows_the_node_through_split_and_combine() {
    use crate::graph::{CompoundEdgeSegment, CrossHierarchyEdge, GraphPortRef};

    let mut graph = LGraph::new("g", LayeredOptions::default());
    for id in ["a", "b", "holder", "c"] {
        graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
    }
    // The first component is nodes 0 and 1 with edge 0. The second is nodes 2 and 3 with
    // edge 1, so everything in it moves down: node 2 to 0, node 3 to 1, edge 1 to 0.
    let a_out = graph
        .add_port(
            crate::graph::NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let b_in = graph
        .add_port(
            crate::graph::NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    graph.add_edge(edge_between("e0", a_out, b_in)).unwrap();
    let holder_out = graph
        .add_port(
            crate::graph::NodeId::new(2),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let c_in = graph
        .add_port(
            crate::graph::NodeId::new(3),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    graph
        .add_edge(edge_between("e1", holder_out, c_in))
        .unwrap();

    let mut inside = LGraph::new("inside", LayeredOptions::default());
    inside
        .layerless_nodes
        .push(LNode::new("dummy", 1.0, 1.0, None));
    let port = inside
        .add_port(
            crate::graph::NodeId::new(0),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    // The dummy stands for the port of `holder`, and holds the dummy made for `c`'s port.
    inside.layerless_nodes[NodeId::new(0)].set_kind(crate::graph::LNodeKind::ExternalPort);
    inside.layerless_nodes[NodeId::new(0)]
        .external_port_mut()
        .unwrap()
        .origin_port = Some(GraphPortRef {
        graph_id: GraphId::ROOT,
        port: holder_out,
    });
    inside.layerless_nodes[NodeId::new(0)].ports[port.port].port_dummy = Some(GraphNodeRef {
        graph_id: GraphId::ROOT,
        node: crate::graph::NodeId::new(3),
    });
    let mut graphs = GraphArena::new(graph);
    let inside_id = graphs.nest(GraphId::ROOT, crate::graph::NodeId::new(2), inside);
    let record = |graph_id, edge| CrossHierarchyEdge {
        original_edge_id: "e1".to_string(),
        original_model_order: None,
        graph_id,
        edge,
        segment: CompoundEdgeSegment::Output { depth: 0 },
    };
    graphs[inside_id].cross_hierarchy_edges = vec![
        // A record of the root's edge 1, and one of the nested graph's own edge 5, which
        // belongs to no component.
        record(GraphId::ROOT, crate::graph::EdgeId::new(1)),
        record(inside_id, crate::graph::EdgeId::new(5)),
    ];
    let before = graphs[inside_id].clone();

    let components = split(&mut graphs).unwrap().unwrap();

    assert_eq!(components.len(), 2);
    let between = &graphs[inside_id];
    assert_eq!(
        between.layerless_nodes[NodeId::new(0)]
            .external_port()
            .unwrap()
            .origin_port
            .map(|o| o.port.node),
        Some(crate::graph::NodeId::new(0)),
        "the origin is node 0 of its component"
    );
    assert_eq!(
        between.layerless_nodes[NodeId::new(0)].ports[port.port]
            .port_dummy
            .map(|dummy| dummy.node),
        Some(crate::graph::NodeId::new(1))
    );
    assert_eq!(
        between
            .cross_hierarchy_edges
            .iter()
            .map(|record| (record.graph_id, record.edge))
            .collect::<Vec<_>>(),
        [
            (GraphId::ROOT, crate::graph::EdgeId::new(0)),
            (inside_id, crate::graph::EdgeId::new(5))
        ]
    );
    // The root still holds the node, so the parent link keeps its index.
    assert_eq!(between.parent, before.parent);

    combine(&mut graphs, components).unwrap();

    assert_eq!(graphs[inside_id], before);
}

// Mutation: in `split`, leave out `tables` of a component. In `remap_references`, leave out
// `map_node_ids`. In `combine`, leave the root's `tables` as they were. In `NodeTables`, leave
// one of the five tables out of `subset` or `absorb`.
#[test]
fn the_side_tables_of_nodes_follow_them_through_split_and_combine() {
    let mut graph = LGraph::new("g", LayeredOptions::default());
    for id in ["a", "b", "c", "d"] {
        graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
    }
    // The first component is nodes 0 and 1, the second is nodes 2 and 3. The second one
    // moves down: node 2 to 0, and node 3 to 1.
    for (source, target) in [(0, 1), (2, 3)] {
        let out = graph
            .add_port(
                NodeId::new(source),
                PortType::Output,
                PortSide::East,
                LPoint::default(),
            )
            .unwrap();
        let into = graph
            .add_port(
                NodeId::new(target),
                PortType::Input,
                PortSide::West,
                LPoint::default(),
            )
            .unwrap();
        graph
            .add_edge(edge_between(&format!("e{source}"), out, into))
            .unwrap();
    }
    let row = graph.tables.in_layer.row_mut(NodeId::new(3));
    row.layout_unit = Some(NodeId::new(2));
    row.successor_constraints = vec![NodeId::new(2)];
    row.barycenter_associates = vec![NodeId::new(2)];
    graph
        .tables
        .self_loop_options
        .row_mut(NodeId::new(3))
        .ordering = crate::options::SelfLoopOrderingStrategy::Sequenced;
    *graph.tables.port_alignment.row_mut(NodeId::new(3)) = crate::options::PortAlignment::End;
    graph
        .tables
        .placement_options
        .row_mut(NodeId::new(3))
        .flexibility = crate::options::NodeFlexibility::PortPosition;
    graph.mark_layer_constraint_explicit(NodeId::new(3));
    let mut graphs = GraphArena::new(graph);

    let components = split(&mut graphs).unwrap().unwrap();

    let second = &components[1].graph;
    let row = second.tables.in_layer.get(NodeId::new(1)).unwrap();
    assert_eq!(row.layout_unit, Some(NodeId::new(0)));
    assert_eq!(row.successor_constraints, [NodeId::new(0)]);
    assert_eq!(row.barycenter_associates, [NodeId::new(0)]);
    assert_eq!(
        second
            .tables
            .self_loop_options
            .get(NodeId::new(1))
            .unwrap()
            .ordering,
        crate::options::SelfLoopOrderingStrategy::Sequenced
    );
    assert_eq!(
        second.port_alignment_of(NodeId::new(1)),
        Some(crate::options::PortAlignment::End)
    );
    assert_eq!(
        second
            .tables
            .placement_options
            .get(NodeId::new(1))
            .unwrap()
            .flexibility,
        crate::options::NodeFlexibility::PortPosition
    );
    assert!(second.is_layer_constraint_explicit(NodeId::new(1)));
    assert!(
        !components[0]
            .graph
            .is_layer_constraint_explicit(NodeId::new(1))
    );
    assert_eq!(
        components[0].graph.in_layer_layout_unit(NodeId::new(1)),
        None
    );

    // A processor of the second component records a row and an option for its node 0.
    let mut components = components;
    components[1]
        .graph
        .tables
        .in_layer
        .row_mut(NodeId::new(0))
        .successor_constraints = vec![NodeId::new(1)];
    components[1]
        .graph
        .tables
        .self_loop_options
        .row_mut(NodeId::new(0))
        .distribution = crate::options::SelfLoopDistributionStrategy::Equally;
    *components[1]
        .graph
        .tables
        .port_alignment
        .row_mut(NodeId::new(0)) = crate::options::PortAlignment::Begin;
    components[1]
        .graph
        .tables
        .placement_options
        .row_mut(NodeId::new(0))
        .flexibility = crate::options::NodeFlexibility::PortPosition;
    components[1]
        .graph
        .mark_layer_constraint_explicit(NodeId::new(0));

    combine(&mut graphs, components).unwrap();

    let root = graphs.root();
    assert_eq!(
        root.in_layer_layout_unit(NodeId::new(3)),
        Some(NodeId::new(2))
    );
    assert_eq!(
        root.in_layer_successor_constraints(NodeId::new(3)),
        [NodeId::new(2)]
    );
    assert_eq!(root.barycenter_associates(NodeId::new(3)), [NodeId::new(2)]);
    assert_eq!(
        root.tables
            .self_loop_options
            .get(NodeId::new(3))
            .unwrap()
            .ordering,
        crate::options::SelfLoopOrderingStrategy::Sequenced
    );
    assert_eq!(root.in_layer_layout_unit(NodeId::new(1)), None);
    // The rows a processor wrote are back at the original node.
    assert_eq!(
        root.in_layer_successor_constraints(NodeId::new(2)),
        [NodeId::new(3)]
    );
    assert_eq!(
        root.tables
            .self_loop_options
            .get(NodeId::new(2))
            .unwrap()
            .distribution,
        crate::options::SelfLoopDistributionStrategy::Equally
    );
    assert_eq!(
        root.port_alignment_of(NodeId::new(2)),
        Some(crate::options::PortAlignment::Begin)
    );
    assert_eq!(
        root.port_alignment_of(NodeId::new(3)),
        Some(crate::options::PortAlignment::End)
    );
    assert_eq!(
        root.tables
            .placement_options
            .get(NodeId::new(2))
            .unwrap()
            .flexibility,
        crate::options::NodeFlexibility::PortPosition
    );
    assert!(root.is_layer_constraint_explicit(NodeId::new(2)));
    assert!(root.is_layer_constraint_explicit(NodeId::new(3)));
    assert!(!root.is_layer_constraint_explicit(NodeId::new(0)));
}

/// A graph for `drop_nodes`: `holder`, `a`, `r`, `b`, `c` as nodes 0 to 4, where `r` is the one
/// to drop. Edge 0 runs from `a` to `b`, edge 1 from `r` to `b` and is detached, edge 2 from `b`
/// to `c`. `a` and `b` sit in layers, and `b` has a row in a side table.
fn graph_with_a_node_to_drop() -> LGraph {
    let mut graph = LGraph::new("g", LayeredOptions::default());
    for id in ["holder", "a", "r", "b", "c"] {
        graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
    }
    let port = |graph: &mut LGraph, node: usize, port_type: PortType, side: PortSide| {
        graph
            .add_port(NodeId::new(node), port_type, side, LPoint::default())
            .unwrap()
    };
    let a_out = port(&mut graph, 1, PortType::Output, PortSide::East);
    let r_out = port(&mut graph, 2, PortType::Output, PortSide::East);
    let b_in = port(&mut graph, 3, PortType::Input, PortSide::West);
    let b_out = port(&mut graph, 3, PortType::Output, PortSide::East);
    let c_in = port(&mut graph, 4, PortType::Input, PortSide::West);
    graph.add_edge(edge_between("e0", a_out, b_in)).unwrap();
    graph.add_edge(edge_between("e1", r_out, b_in)).unwrap();
    graph.add_edge(edge_between("e2", b_out, c_in)).unwrap();
    // The routing detaches the edge of a replacement before the drop.
    graph.detach_edge(EdgeId::new(1));
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(3), 1);
    *graph.tables.port_alignment.row_mut(NodeId::new(3)) = crate::options::PortAlignment::End;
    graph
}

fn node_name(graph: &LGraph, node: NodeId) -> &str {
    &graph.layerless_nodes[node].id
}

// Mutation: leave out the `remap_references` call in `drop_nodes`. Edge `e2` then still names
// node 3, which is `c` now, and the layer of `b` names a node that is gone.
#[test]
fn dropping_a_node_moves_the_nodes_and_edges_after_it_down() {
    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());

    drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap();

    let graph = graphs.root();
    let names = graph
        .layerless_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["holder", "a", "b", "c"]);
    let edges = graph
        .edges
        .iter()
        .map(|edge| {
            (
                edge.id.as_str(),
                node_name(graph, edge.source().node),
                node_name(graph, edge.target().node),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(edges, [("e0", "a", "b"), ("e2", "b", "c")]);
    let b = graph.node_named("b");
    assert_eq!(
        graph.layerless_nodes[b].ports[PortId::new(0)].incoming_edges(),
        [EdgeId::new(0)]
    );
    assert_eq!(
        graph.layerless_nodes[b].ports[PortId::new(1)].outgoing_edges(),
        [EdgeId::new(1)]
    );
    assert_eq!(graph.layers()[1].nodes(), [b]);
    assert_eq!(
        graph.port_alignment_of(b),
        Some(crate::options::PortAlignment::End)
    );
}

// Mutation: in `drop_nodes`, keep every edge. Edge `e1` then names the dropped node, and the
// drop fails.
#[test]
fn an_edge_that_ends_at_a_dropped_node_goes_with_it() {
    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());

    drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap();

    assert!(graphs.root().edges.iter().all(|edge| edge.id != "e1"));
}

// Mutation: in `drop_nodes`, take each dropped node out of its layer before the drop. The node
// then goes silently, though a step still holds it in a layer.
#[test]
fn a_dropped_node_in_a_layer_is_an_error() {
    let mut graph = graph_with_a_node_to_drop();
    graph.set_node_layer(NodeId::new(2), 0);
    let mut graphs = GraphArena::new(graph);

    let error = drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap_err();

    assert_eq!(
        error,
        ComponentError::InvalidReference {
            kind: "layer node",
            index: 2
        }
    );
}

// Mutation: leave out the check for a node that holds a graph. Node `c` then moves from 4 to 3,
// and the graph inside it still names 4 as its parent node.
#[test]
fn a_node_that_holds_a_graph_after_a_dropped_node_is_an_error() {
    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());
    graphs.nest(
        GraphId::ROOT,
        NodeId::new(4),
        LGraph::new("inner", LayeredOptions::default()),
    );

    let error = drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap_err();

    assert_eq!(
        error,
        ComponentError::InvalidReference {
            kind: "graph owner after a dropped node",
            index: 4
        }
    );
}

// Mutation: in `drop_nodes`, leave out the graphs inside the nodes. The dummy in the inner graph
// then names node 4 as its origin, which no longer exists.
#[test]
fn a_graph_inside_a_node_sees_the_new_ids() {
    use crate::graph::GraphPortRef;

    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());
    let mut inner = LGraph::new("inner", LayeredOptions::default());
    let mut dummy = LNode::new("dummy", 0.0, 0.0, None);
    dummy.set_kind(crate::graph::LNodeKind::ExternalPort);
    dummy.external_port_mut().unwrap().origin_port = Some(GraphPortRef {
        graph_id: GraphId::ROOT,
        port: PortRef {
            node: NodeId::new(4),
            port: PortId::new(0),
        },
    });
    inner.layerless_nodes.push(dummy);
    let inner = graphs.nest(GraphId::ROOT, NodeId::new(0), inner);

    drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap();

    let origin = graphs[inner].layerless_nodes[NodeId::new(0)]
        .external_port()
        .unwrap()
        .origin_port
        .unwrap();
    assert_eq!(node_name(graphs.root(), origin.port.node), "c");
}

// Mutation: in `drop_nodes`, leave out the graphs above. The port of the outer node then names
// node 4 of the inner graph as its dummy, which no longer exists.
#[test]
fn the_graph_above_sees_the_new_ids() {
    let mut outer = LGraph::new("outer", LayeredOptions::default());
    outer
        .layerless_nodes
        .push(LNode::new("group", 10.0, 10.0, None));
    let group_port = outer
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let mut graphs = GraphArena::new(outer);
    let inner = graphs.nest(GraphId::ROOT, NodeId::new(0), graph_with_a_node_to_drop());
    graphs.root_mut().layerless_nodes[NodeId::new(0)].ports[group_port.port].port_dummy =
        Some(GraphNodeRef {
            graph_id: inner,
            node: NodeId::new(4),
        });

    drop_nodes(&mut graphs, inner, &[NodeId::new(2)]).unwrap();

    let dummy = graphs.root().layerless_nodes[NodeId::new(0)].ports[group_port.port]
        .port_dummy
        .unwrap();
    assert_eq!(node_name(&graphs[inner], dummy.node), "c");
}

// Mutation: in `drop_nodes`, renumber the ends of a long edge dummy instead of clearing an end
// at a dropped node. The drop then fails on the end at `r`.
#[test]
fn a_long_edge_end_at_a_dropped_node_is_cleared() {
    let mut graph = graph_with_a_node_to_drop();
    let c = NodeId::new(4);
    graph.layerless_nodes[c].set_kind(crate::graph::LNodeKind::LongEdge);
    let ends = graph.layerless_nodes[c].long_edge_ends_mut().unwrap();
    ends.source = Some(PortRef {
        node: NodeId::new(2),
        port: PortId::new(0),
    });
    ends.target = Some(PortRef {
        node: NodeId::new(3),
        port: PortId::new(1),
    });
    let mut graphs = GraphArena::new(graph);

    drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap();

    let graph = graphs.root();
    let ends = graph.layerless_nodes[graph.node_named("c")]
        .long_edge_ends()
        .unwrap();
    assert_eq!(ends.source, None);
    assert_eq!(
        ends.target.map(|port| node_name(graph, port.node)),
        Some("b")
    );
}

// Mutation: in `drop_nodes`, renumber `long_edge_target_node` instead of clearing it when it
// names a dropped node. The drop then fails on the port of `b`.
#[test]
fn a_long_edge_target_at_a_dropped_node_is_cleared() {
    let mut graph = graph_with_a_node_to_drop();
    let b = NodeId::new(3);
    graph.layerless_nodes[b].ports[PortId::new(0)].long_edge_target_node = Some(NodeId::new(2));
    graph.layerless_nodes[b].ports[PortId::new(1)].long_edge_target_node = Some(NodeId::new(4));
    let mut graphs = GraphArena::new(graph);

    drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap();

    let graph = graphs.root();
    let ports = &graph.layerless_nodes[graph.node_named("b")].ports;
    assert_eq!(ports[PortId::new(0)].long_edge_target_node, None);
    assert_eq!(
        ports[PortId::new(1)]
            .long_edge_target_node
            .map(|node| node_name(graph, node)),
        Some("c")
    );
}

// Mutation: in `drop_nodes`, skip a dropped id past the end of the node list. The call then
// succeeds and drops nothing.
#[test]
fn dropping_a_node_that_does_not_exist_is_an_error() {
    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());

    let error = drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(9)]).unwrap_err();

    assert_eq!(
        error,
        ComponentError::InvalidReference {
            kind: "dropped node",
            index: 9
        }
    );
}

// Mutation: in `drop_nodes`, return an `InvalidReference` error when `dropped` is empty. The
// routing of a graph without replacements then fails.
#[test]
fn dropping_no_node_leaves_the_graph_as_it_is() {
    let mut graphs = GraphArena::new(graph_with_a_node_to_drop());
    let before = graphs.root().clone();

    drop_nodes(&mut graphs, GraphId::ROOT, &[]).unwrap();

    assert_eq!(graphs.root(), &before);
}

// Mutation: in `drop_nodes`, index the keep flags with an edge's node id directly. An edge that
// names a node past the end then panics instead of giving an error.
#[test]
fn an_edge_that_names_a_missing_node_is_an_error_not_a_panic() {
    let mut graph = graph_with_a_node_to_drop();
    graph.edges[EdgeId::new(2)]
        .map_ports(|port| {
            port.node = NodeId::new(9);
            Ok::<_, ()>(())
        })
        .unwrap();
    let mut graphs = GraphArena::new(graph);

    let error = drop_nodes(&mut graphs, GraphId::ROOT, &[NodeId::new(2)]).unwrap_err();

    assert_eq!(
        error,
        ComponentError::InvalidReference {
            kind: "port node",
            index: 9
        }
    );
}

// Mutation: in `drop_nodes`, close the gap of a dropped node with the last node, as a
// swap-remove does, instead of moving every later node down in order. `combine` then puts the
// dummies `d2` and `d1` in the wrong order.
#[test]
fn a_component_drops_a_node_between_split_and_combine() {
    let mut graph = LGraph::new("g", LayeredOptions::default());
    for id in ["alone", "x", "y"] {
        graph.layerless_nodes.push(LNode::new(id, 10.0, 10.0, None));
    }
    let x_out = graph
        .add_port(
            NodeId::new(1),
            PortType::Output,
            PortSide::East,
            LPoint::default(),
        )
        .unwrap();
    let y_in = graph
        .add_port(
            NodeId::new(2),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    graph.add_edge(edge_between("e", x_out, y_in)).unwrap();
    let mut graphs = GraphArena::new(graph);
    let mut components = split(&mut graphs).unwrap().unwrap();
    // The second component is `x` and `y`. A processor then adds `r`, `d1` and `d2`, and `r`
    // goes again, as a replacement does.
    let component = &mut components[1];
    for id in ["r", "d1", "d2"] {
        component
            .graph
            .layerless_nodes
            .push(LNode::new(id, 1.0, 1.0, None));
    }
    std::mem::swap(&mut graphs[GraphId::ROOT], &mut component.graph);
    let r = graphs.root().node_named("r");
    drop_nodes(&mut graphs, GraphId::ROOT, &[r]).unwrap();
    std::mem::swap(&mut graphs[GraphId::ROOT], &mut component.graph);

    combine(&mut graphs, components).unwrap();

    let names = graphs
        .root()
        .layerless_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["alone", "x", "y", "d1", "d2"]);
    let edge = &graphs.root().edges[EdgeId::new(0)];
    assert_eq!(node_name(graphs.root(), edge.source().node), "x");
    assert_eq!(node_name(graphs.root(), edge.target().node), "y");
}
