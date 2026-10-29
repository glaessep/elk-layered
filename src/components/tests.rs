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
