use super::*;

fn graph_with_nodes(count: usize) -> LGraph {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    for index in 0..count {
        graph.push_node_with(|_| LNode::new(format!("n{index}"), 10.0, 10.0, Some(index)));
    }
    graph
}

/// Every node that names a layer sits in it exactly once, and every layer lists only nodes
/// that name it.
fn assert_layers_agree_with_nodes(graph: &LGraph) {
    for (id, node) in graph.nodes_with_ids() {
        if let Some(layer) = node.layer_index {
            let places = graph.layers[layer]
                .nodes()
                .iter()
                .filter(|listed| **listed == id)
                .count();
            assert_eq!(places, 1, "{} is not listed once in layer {layer}", node.id);
        }
    }
    for (index, layer) in graph.layers.iter().enumerate() {
        for listed in layer.nodes() {
            assert_eq!(
                graph.layerless_nodes[*listed].layer_index,
                Some(index),
                "layer {index} lists {}",
                graph.layerless_nodes[*listed].id
            );
        }
    }
}

fn ids(graph: &LGraph, indexes: &[usize]) -> Vec<NodeId> {
    let all = graph.node_ids().collect::<Vec<_>>();
    indexes.iter().map(|index| all[*index]).collect()
}

// Mutation: in `set_layer_nodes`, skip clearing `layer_index` of the nodes that leave.
#[test]
fn set_layer_nodes_makes_a_node_that_no_longer_appears_layerless() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.set_layer_nodes(0, all.clone());

    graph.set_layer_nodes(0, vec![all[2], all[0]]);

    assert_eq!(graph.layers[0].nodes(), [all[2], all[0]]);
    assert_eq!(graph.layerless_nodes[all[1]].layer_index, None);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `set_layer_nodes`, skip removing a joining node from the layer it was in.
#[test]
fn set_layer_nodes_takes_a_node_out_of_its_old_layer() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.set_layer_nodes(0, vec![all[0], all[1]]);
    graph.set_layer_nodes(1, vec![all[2]]);

    graph.set_layer_nodes(1, vec![all[1], all[2]]);

    assert_eq!(graph.layers[0].nodes(), [all[0]]);
    assert_eq!(graph.layers[1].nodes(), [all[1], all[2]]);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `set_layer_nodes`, do not add the missing layers.
#[test]
fn set_layer_nodes_adds_the_layers_up_to_the_one_it_fills() {
    let mut graph = graph_with_nodes(1);
    let all = ids(&graph, &[0]);

    graph.set_layer_nodes(2, all.clone());

    assert_eq!(graph.layers.len(), 3);
    assert!(graph.layers[0].nodes().is_empty());
    assert_eq!(graph.layers[2].nodes(), all);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `insert_node_into_layer`, leave `layer_index` unset.
#[test]
fn insert_node_into_layer_places_the_node_at_the_position() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.set_layer_nodes(0, vec![all[0], all[1]]);

    graph.insert_node_into_layer(all[2], 0, 1);

    assert_eq!(graph.layers[0].nodes(), [all[0], all[2], all[1]]);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `insert_node_into_layer`, skip removing the node from its old layer.
#[test]
fn insert_node_into_layer_moves_a_node_that_is_in_another_layer() {
    let mut graph = graph_with_nodes(2);
    let all = ids(&graph, &[0, 1]);
    graph.set_layer_nodes(0, vec![all[0]]);
    graph.set_layer_nodes(1, vec![all[1]]);

    graph.insert_node_into_layer(all[0], 1, 0);

    assert!(graph.layers[0].nodes().is_empty());
    assert_eq!(graph.layers[1].nodes(), [all[0], all[1]]);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `insert_node_into_layer`, insert at `position` without capping it to the
// layer's length. The call then panics.
#[test]
fn insert_node_into_layer_appends_when_the_position_is_past_the_end() {
    let mut graph = graph_with_nodes(2);
    let all = ids(&graph, &[0, 1]);
    graph.set_layer_nodes(0, vec![all[0]]);

    graph.insert_node_into_layer(all[1], 0, 9);

    assert_eq!(graph.layers[0].nodes(), [all[0], all[1]]);
}

// Mutation: in `swap_layer_places`, swap the two list slots but not the two `layer_index`
// values.
#[test]
fn swap_layer_places_exchanges_layer_and_position() {
    let mut graph = graph_with_nodes(4);
    let all = ids(&graph, &[0, 1, 2, 3]);
    graph.set_layer_nodes(0, vec![all[0], all[1]]);
    graph.set_layer_nodes(1, vec![all[2], all[3]]);

    graph.swap_layer_places(all[1], all[2]);

    assert_eq!(graph.layers[0].nodes(), [all[0], all[2]]);
    assert_eq!(graph.layers[1].nodes(), [all[1], all[3]]);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `swap_layer_places`, read a missing layer or position as 0 instead of
// returning. The layerless node then swaps with the first node of layer 0.
#[test]
fn swap_layer_places_does_nothing_for_a_node_without_a_layer() {
    let mut graph = graph_with_nodes(2);
    let all = ids(&graph, &[0, 1]);
    graph.set_layer_nodes(0, vec![all[0]]);
    let before = graph.clone();

    graph.swap_layer_places(all[0], all[1]);

    assert_eq!(graph, before);
}

// Mutation: in `set_node_layer`, leave the node in its old layer when it joins the new one.
#[test]
fn set_node_layer_moves_the_node_to_the_end_of_the_new_layer() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.set_node_layer(all[0], 0);
    graph.set_node_layer(all[1], 1);
    graph.set_node_layer(all[2], 1);

    graph.set_node_layer(all[0], 1);

    assert!(graph.layers[0].nodes().is_empty());
    assert_eq!(graph.layers[1].nodes(), [all[1], all[2], all[0]]);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `remove_node_at`, keep `layer_index` set.
#[test]
fn remove_node_at_takes_out_the_node_at_the_position() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.set_layer_nodes(0, all.clone());

    assert_eq!(graph.remove_node_at(0, 1), Some(all[1]));

    assert_eq!(graph.layers()[0].nodes(), [all[0], all[2]]);
    assert_eq!(graph.layerless_nodes[all[1]].layer_index(), None);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `remove_node_at`, index the layer's list without checking the position. The
// call then panics.
#[test]
fn remove_node_at_returns_none_past_the_end_or_for_a_missing_layer() {
    let mut graph = graph_with_nodes(1);
    let all = ids(&graph, &[0]);
    graph.set_layer_nodes(0, all);
    let before = graph.clone();

    assert_eq!(graph.remove_node_at(0, 1), None);
    assert_eq!(graph.remove_node_at(5, 0), None);

    assert_eq!(graph, before);
}

// Mutation: in `layer_size_mut`, answer with the size of layer 0 for every layer.
#[test]
fn layer_size_mut_changes_the_size_of_that_layer_only() {
    let mut graph = graph_with_nodes(2);
    let all = ids(&graph, &[0, 1]);
    graph.set_layer_nodes(0, vec![all[0]]);
    graph.set_layer_nodes(1, vec![all[1]]);

    graph.layer_size_mut(1).width = 7.0;

    assert_eq!(graph.layers()[0].size.width, 0.0);
    assert_eq!(graph.layers()[1].size.width, 7.0);
    assert_layers_agree_with_nodes(&graph);
}

// Mutation: in `remove_node_from_layer`, keep `layer_index` set.
#[test]
fn remove_node_from_layer_makes_the_node_layerless() {
    let mut graph = graph_with_nodes(2);
    let all = ids(&graph, &[0, 1]);
    graph.set_layer_nodes(0, all.clone());

    graph.remove_node_from_layer(all[0]);
    graph.remove_node_from_layer(all[0]);

    assert_eq!(graph.layers[0].nodes(), [all[1]]);
    assert_eq!(graph.layerless_nodes[all[0]].layer_index, None);
    assert_layers_agree_with_nodes(&graph);
}

/// Two nodes, `a` with two output ports and `b` with two input ports.
struct EdgeFixture {
    graph: LGraph,
    a: [PortRef; 2],
    b: [PortRef; 2],
}

fn edge_fixture() -> EdgeFixture {
    let mut graph = graph_with_nodes(2);
    let nodes = ids(&graph, &[0, 1]);
    let mut port = |node: NodeId, port_type, side| {
        graph
            .add_port(node, port_type, side, LPoint::default())
            .unwrap()
    };
    let a = [
        port(nodes[0], PortType::Output, PortSide::East),
        port(nodes[0], PortType::Output, PortSide::East),
    ];
    let b = [
        port(nodes[1], PortType::Input, PortSide::West),
        port(nodes[1], PortType::Input, PortSide::West),
    ];
    EdgeFixture { graph, a, b }
}

/// Every edge sits once in the list of the port it names, and every port lists only edges
/// that name it.
fn assert_edges_agree_with_ports(graph: &LGraph) {
    for (id, edge) in graph.edges_with_ids() {
        let source = &graph.layerless_nodes[edge.source().node].ports[edge.source().port];
        let target = &graph.layerless_nodes[edge.target().node].ports[edge.target().port];
        let at_source = source.outgoing_edges().iter().filter(|e| **e == id).count();
        let at_target = target.incoming_edges().iter().filter(|e| **e == id).count();
        assert_eq!((at_source, at_target), (1, 1), "edge {}", edge.id);
    }
    for node in graph.node_ids() {
        for (port, data) in graph.layerless_nodes[node].ports.iter_with_ids() {
            let this = PortRef { node, port };
            for edge in data.outgoing_edges() {
                assert_eq!(graph.edges[*edge].source(), this);
            }
            for edge in data.incoming_edges() {
                assert_eq!(graph.edges[*edge].target(), this);
            }
        }
    }
}

fn outgoing(graph: &LGraph, port: PortRef) -> Vec<EdgeId> {
    graph.layerless_nodes[port.node].ports[port.port]
        .outgoing_edges()
        .to_vec()
}

fn incoming(graph: &LGraph, port: PortRef) -> Vec<EdgeId> {
    graph.layerless_nodes[port.node].ports[port.port]
        .incoming_edges()
        .to_vec()
}

// Mutation: in `add_edge`, skip pushing the edge to the incoming list of its target.
#[test]
fn add_edge_attaches_the_edge_to_both_ports() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();

    let first = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let second = graph.add_edge(LayeredEdge::new("e1", a[0], b[0])).unwrap();

    assert_eq!(outgoing(&graph, a[0]), [first, second]);
    assert_eq!(incoming(&graph, b[0]), [first, second]);
    assert_edges_agree_with_ports(&graph);
}

// Mutation: in `add_edge`, check only the source port and push to a target port that is
// missing. The call then panics.
#[test]
fn add_edge_refuses_an_edge_whose_port_is_missing() {
    let EdgeFixture { mut graph, a, .. } = edge_fixture();
    let missing = PortRef {
        node: a[0].node,
        port: PortId::new(99),
    };

    assert_eq!(graph.add_edge(LayeredEdge::new("e0", a[0], missing)), None);
    assert_eq!(graph.add_edge(LayeredEdge::new("e1", missing, a[0])), None);

    assert!(graph.edges.is_empty());
    assert!(outgoing(&graph, a[0]).is_empty());
    assert!(incoming(&graph, a[0]).is_empty());
}

// Mutation: in `set_edge_source`, skip removing the edge from the old source port.
#[test]
fn set_edge_source_moves_the_edge_to_the_end_of_the_new_port() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let moved = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let stays = graph.add_edge(LayeredEdge::new("e1", a[1], b[0])).unwrap();

    assert!(graph.set_edge_source(moved, a[1]));

    assert!(outgoing(&graph, a[0]).is_empty());
    assert_eq!(outgoing(&graph, a[1]), [stays, moved]);
    assert_eq!(graph.edges[moved].source(), a[1]);
    assert_edges_agree_with_ports(&graph);
}

// Mutation: in `set_edge_source`, skip the check that the new port exists.
#[test]
fn set_edge_source_changes_nothing_for_a_missing_port_or_edge() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let edge = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let missing = PortRef {
        node: a[0].node,
        port: PortId::new(99),
    };
    let before = graph.clone();

    assert!(!graph.set_edge_source(edge, missing));
    assert!(!graph.set_edge_source(EdgeId::new(99), a[1]));

    assert_eq!(graph, before);
}

// Mutation: in `set_edge_target_at`, ignore `insert_at` and always push.
#[test]
fn set_edge_target_at_inserts_at_the_position_and_caps_it() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let first = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let second = graph.add_edge(LayeredEdge::new("e1", a[0], b[0])).unwrap();
    let moved = graph.add_edge(LayeredEdge::new("e2", a[1], b[1])).unwrap();
    let capped = graph.add_edge(LayeredEdge::new("e3", a[1], b[1])).unwrap();

    assert!(graph.set_edge_target_at(moved, b[0], Some(1)));
    assert!(graph.set_edge_target_at(capped, b[0], Some(99)));

    assert_eq!(incoming(&graph, b[0]), [first, moved, second, capped]);
    assert!(incoming(&graph, b[1]).is_empty());
    assert_edges_agree_with_ports(&graph);
}

// Mutation: in `set_edge_target`, skip removing the edge from the old target port.
#[test]
fn set_edge_target_moves_the_edge_to_the_end_of_the_new_port() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let moved = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let stays = graph.add_edge(LayeredEdge::new("e1", a[0], b[1])).unwrap();

    assert!(graph.set_edge_target(moved, b[1]));

    assert!(incoming(&graph, b[0]).is_empty());
    assert_eq!(incoming(&graph, b[1]), [stays, moved]);
    assert_edges_agree_with_ports(&graph);
}

// Mutation: in `detach_edge_target`, leave the edge in the incoming list of its target.
#[test]
fn detach_edge_removes_the_edge_from_both_ports_and_keeps_its_ends() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let edge = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();

    assert_eq!(graph.detach_edge(edge), Some((a[0], b[0])));

    assert!(outgoing(&graph, a[0]).is_empty());
    assert!(incoming(&graph, b[0]).is_empty());
    assert!(!graph.edge_source_attached(edge));
    assert!(!graph.edge_target_attached(edge));
    assert_eq!(graph.edges[edge].source(), a[0]);
    assert_eq!(graph.detach_edge(EdgeId::new(99)), None);
}

// Mutation: in `replace_incoming_edge_at`, leave the target of the new edge unchanged.
#[test]
fn replace_incoming_edge_at_gives_the_edge_the_exact_slot() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let first = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let replaced = graph.add_edge(LayeredEdge::new("e1", a[0], b[0])).unwrap();
    let last = graph.add_edge(LayeredEdge::new("e2", a[0], b[0])).unwrap();
    let newcomer = graph.add_edge(LayeredEdge::new("e3", a[1], b[1])).unwrap();
    graph.remove_leading_incoming_edges(b[1], 1);

    graph.replace_incoming_edge_at(b[0], 1, newcomer);

    assert_eq!(incoming(&graph, b[0]), [first, newcomer, last]);
    assert_eq!(graph.edges[newcomer].target(), b[0]);
    // The replaced edge is left for the caller to drop.
    assert_eq!(graph.edges[replaced].target(), b[0]);
    assert!(!graph.edge_target_attached(replaced));
}

// Mutation: in `remove_leading_incoming_edges`, drain no edge.
#[test]
fn remove_leading_edges_removes_the_first_ones_of_a_port() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let edges = (0..3)
        .map(|index| {
            graph
                .add_edge(LayeredEdge::new(format!("e{index}"), a[0], b[0]))
                .unwrap()
        })
        .collect::<Vec<_>>();

    graph.remove_leading_incoming_edges(b[0], 2);
    graph.remove_leading_outgoing_edges(a[0], 1);

    assert_eq!(incoming(&graph, b[0]), [edges[2]]);
    assert_eq!(outgoing(&graph, a[0]), [edges[1], edges[2]]);
}

// Mutation: in `reorder_incoming_edges`, skip the check that the order names each edge once.
#[test]
fn reorder_edges_accepts_only_a_permutation_of_the_ports_edges() {
    let EdgeFixture { mut graph, a, b } = edge_fixture();
    let first = graph.add_edge(LayeredEdge::new("e0", a[0], b[0])).unwrap();
    let second = graph.add_edge(LayeredEdge::new("e1", a[0], b[0])).unwrap();

    assert!(graph.reorder_incoming_edges(b[0], vec![second, first]));
    assert!(graph.reorder_outgoing_edges(a[0], vec![second, first]));
    assert_eq!(incoming(&graph, b[0]), [second, first]);
    assert_eq!(outgoing(&graph, a[0]), [second, first]);

    assert!(!graph.reorder_incoming_edges(b[0], vec![first]));
    assert!(!graph.reorder_incoming_edges(b[0], vec![first, first]));
    assert!(!graph.reorder_outgoing_edges(a[0], vec![first, EdgeId::new(99)]));
    assert_eq!(incoming(&graph, b[0]), [second, first]);
    assert_eq!(outgoing(&graph, a[0]), [second, first]);
}

fn dummy_of(kind: LNodeKind) -> LNode {
    let mut node = LNode::new("dummy", 0.0, 0.0, None);
    node.set_kind(kind);
    node
}

// Mutation: in `set_kind`, make a `Label` dummy hold the data of a long edge dummy, or make a
// `BreakingPoint` node a plain node again.
#[test]
fn set_kind_gives_the_node_the_data_of_its_kind() {
    for kind in [
        LNodeKind::Normal,
        LNodeKind::LongEdge,
        LNodeKind::Label,
        LNodeKind::ExternalPort,
        LNodeKind::NorthSouthPort,
        LNodeKind::BreakingPoint,
    ] {
        assert_eq!(dummy_of(kind).kind(), kind, "{kind:?}");
    }
}

// Mutation: in `origin_edge`, answer `Some(EdgeId::new(0))` for an external port dummy.
#[test]
fn dummy_data_of_another_kind_reads_as_none() {
    let normal = dummy_of(LNodeKind::Normal);
    let long_edge = dummy_of(LNodeKind::LongEdge);
    let label = dummy_of(LNodeKind::Label);
    let external = dummy_of(LNodeKind::ExternalPort);
    let north_south = dummy_of(LNodeKind::NorthSouthPort);

    for node in [&normal, &label, &external, &north_south] {
        assert!(node.long_edge().is_none(), "{:?}", node.kind());
    }
    for node in [&normal, &long_edge, &external, &north_south] {
        assert!(node.label_dummy().is_none(), "{:?}", node.kind());
    }
    for node in [&normal, &long_edge, &label, &north_south] {
        assert!(node.external_port().is_none(), "{:?}", node.kind());
    }
    for node in [&normal, &external, &north_south] {
        assert!(node.long_edge_ends().is_none(), "{:?}", node.kind());
        assert_eq!(node.origin_edge(), None, "{:?}", node.kind());
    }
    for node in [&normal, &long_edge, &external, &north_south] {
        assert!(node.represented_labels().is_empty(), "{:?}", node.kind());
    }
}

// Mutation: in `long_edge_ends`, answer for a long edge dummy only, not for a label dummy.
#[test]
fn long_edge_dummies_and_label_dummies_share_the_ends_of_their_long_edge() {
    let ends = LongEdgeEnds {
        source: Some(PortRef {
            node: NodeId::new(0),
            port: PortId::new(0),
        }),
        target: Some(PortRef {
            node: NodeId::new(1),
            port: PortId::new(0),
        }),
    };
    for kind in [LNodeKind::LongEdge, LNodeKind::Label] {
        let mut node = dummy_of(kind);
        assert_eq!(node.long_edge_ends(), Some(LongEdgeEnds::default()));

        node.long_edge_ends_mut().unwrap().source = ends.source;
        node.long_edge_ends_mut().unwrap().target = ends.target;

        assert_eq!(node.long_edge_ends(), Some(ends), "{kind:?}");
    }
}

// Mutation: in `origin_edge`, read the label dummy's edge from the long edge arm.
#[test]
fn origin_edge_is_the_edge_a_long_edge_dummy_or_label_dummy_was_made_from() {
    let mut long_edge = dummy_of(LNodeKind::LongEdge);
    long_edge.long_edge_mut().unwrap().origin_edge = Some(EdgeId::new(3));
    let mut label = dummy_of(LNodeKind::Label);
    label.label_dummy_mut().unwrap().origin_edge = Some(EdgeId::new(4));

    assert_eq!(long_edge.origin_edge(), Some(EdgeId::new(3)));
    assert_eq!(label.origin_edge(), Some(EdgeId::new(4)));
    assert_eq!(dummy_of(LNodeKind::LongEdge).origin_edge(), None);
}

// Mutation: in `set_kind`, keep the data of the old kind when the kind changes.
#[test]
fn set_kind_replaces_the_data_of_the_old_kind() {
    let mut node = dummy_of(LNodeKind::ExternalPort);
    node.external_port_mut().unwrap().side = PortSide::North;

    node.set_kind(LNodeKind::LongEdge);

    assert!(node.external_port().is_none());
    assert_eq!(node.long_edge_ends(), Some(LongEdgeEnds::default()));
    node.set_kind(LNodeKind::ExternalPort);
    assert_eq!(node.external_port().unwrap().side, PortSide::Undefined);
}

// Mutation: in `apply_port_index_to_dummy`, set the ratio on a node that is no external
// port dummy by making it one.
#[test]
fn apply_port_index_to_dummy_leaves_a_node_that_is_no_external_port_dummy_alone() {
    let mut node = dummy_of(LNodeKind::Normal);

    apply_port_index_to_dummy(&mut node, PortConstraints::FixedOrder, Some(2));

    assert_eq!(node.kind(), LNodeKind::Normal);
    assert!(node.external_port().is_none());
}

// Mutation: in `hidden_mask`, set the flag of the first node instead of the listed ones.
#[test]
fn hidden_mask_marks_the_listed_hidden_nodes() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    graph.hidden_nodes = vec![all[2]];

    let mask = graph.hidden_mask();

    assert_eq!(
        [mask[all[0]], mask[all[1]], mask[all[2]]],
        [false, false, true]
    );
    assert!(graph.layerless_nodes.iter().count() == mask.len());
}

fn external_dummy_on(side: PortSide) -> LNode {
    create_external_port_dummy(
        NodeId::new(0),
        "dummy",
        "port",
        PortType::Output,
        PortConstraints::FixedSide,
        side,
        0,
        LPoint::default(),
        LSize::default(),
        LSize::default(),
        0.0,
        ElkDirection::Right,
        ExternalPortData::default(),
    )
}

// Mutation: in `create_external_port_dummy`, leave out `..data`, or keep the side of `data`
// instead of the one the method works out.
#[test]
fn create_external_port_dummy_keeps_the_callers_data_and_sets_its_own() {
    let dummy = create_external_port_dummy(
        NodeId::new(0),
        "dummy",
        "port",
        PortType::Output,
        PortConstraints::FixedSide,
        PortSide::West,
        0,
        LPoint::default(),
        LSize::default(),
        LSize::default(),
        0.0,
        ElkDirection::Right,
        ExternalPortData {
            side: PortSide::North,
            scope_port: Some(3),
            parent_port_key: Some("key".to_string()),
            parent_port_type: Some(PortType::Input),
            ..ExternalPortData::default()
        },
    );

    let external = dummy.external_port().unwrap();
    assert_eq!(external.side, PortSide::West);
    assert_eq!(external.scope_port, Some(3));
    assert_eq!(external.parent_port_key.as_deref(), Some("key"));
    assert_eq!(external.parent_port_type, Some(PortType::Input));
}

// Mutation: in `external_port_side`, answer `North` for a node that is no external port
// dummy, or in `in_layer_constraint`, answer `Top`.
#[test]
fn a_node_that_is_no_external_port_dummy_has_no_side_and_no_in_layer_constraint() {
    let mut external = dummy_of(LNodeKind::ExternalPort);
    let data = external.external_port_mut().unwrap();
    data.side = PortSide::South;
    data.in_layer_constraint = InLayerConstraint::Bottom;

    assert_eq!(external.external_port_side(), PortSide::South);
    assert_eq!(external.in_layer_constraint(), InLayerConstraint::Bottom);
    for kind in [
        LNodeKind::Normal,
        LNodeKind::LongEdge,
        LNodeKind::NorthSouthPort,
    ] {
        let node = dummy_of(kind);
        assert_eq!(node.external_port_side(), PortSide::Undefined, "{kind:?}");
        assert_eq!(
            node.in_layer_constraint(),
            InLayerConstraint::None,
            "{kind:?}"
        );
    }
}

// Mutation: in `external_port_and_layer_constraint_mut`, answer `Some` for a long edge
// dummy too, or hand out the constraint of another node.
#[test]
fn the_external_port_data_and_the_layer_constraint_change_together() {
    let mut external = dummy_of(LNodeKind::ExternalPort);
    external.layer_constraint = LayerConstraint::FirstSeparate;

    let (data, constraint) = external.external_port_and_layer_constraint_mut().unwrap();
    data.side = PortSide::East;
    *constraint = LayerConstraint::LastSeparate;

    assert_eq!(external.external_port_side(), PortSide::East);
    assert_eq!(external.layer_constraint, LayerConstraint::LastSeparate);
    assert!(
        dummy_of(LNodeKind::LongEdge)
            .external_port_and_layer_constraint_mut()
            .is_none()
    );
}

// Mutation: in `segment_of`, leave out the copy of one field of the old edge, or keep its
// labels or bend points.
#[test]
fn segment_of_keeps_the_fields_of_the_old_edge_but_not_its_geometry() {
    let EdgeFixture { a, b, .. } = edge_fixture();
    let mut old = LayeredEdge::new("old", a[0], b[0]);
    old.source_node_id = "s".to_string();
    old.target_node_id = "t".to_string();
    old.minlen = 3;
    old.reversed = true;
    old.model_order = Some(4);
    old.priority_direction = 1;
    old.priority_shortness = 2;
    old.priority_straightness = 3;
    old.thickness = 5.0;
    old.original_opposite_port = Some(a[1]);
    old.compound_segment = Some(CompoundEdgeSegment::Output { depth: 2 });
    old.labels.push(LLabel::new("label", 1.0, 1.0));
    old.bend_points.push(LPoint { x: 1.0, y: 2.0 });
    old.junction_points.push(LPoint { x: 3.0, y: 4.0 });

    let segment = LayeredEdge::segment_of(&old, a[1], b[1]);

    let mut expected = old.clone();
    expected.labels.clear();
    expected.bend_points.clear();
    expected.junction_points.clear();
    expected.source = a[1];
    expected.target = b[1];
    assert_eq!(segment, expected);
}

// Mutation: in `create_external_port_dummy`, mark the layer constraint of every dummy
// explicit, or of none. In `is_layer_constraint_explicit`, read only the table.
#[test]
fn a_west_or_east_external_port_dummy_has_an_explicit_layer_constraint() {
    let mut graph = graph_with_nodes(0);
    let nodes = [
        (PortSide::West, true),
        (PortSide::East, true),
        (PortSide::North, false),
        (PortSide::South, false),
    ]
    .map(|(side, explicit)| {
        let dummy = graph.push_node_with(|_| external_dummy_on(side));
        (dummy, explicit, side)
    });

    for (dummy, explicit, side) in nodes {
        assert_eq!(
            graph.is_layer_constraint_explicit(dummy),
            explicit,
            "{side:?}"
        );
    }
}

// Mutation: in `port_alignment_of`, read the row of node 0 for every node.
#[test]
fn port_alignment_of_is_none_for_a_node_that_sets_none() {
    let mut graph = graph_with_nodes(3);
    let all = ids(&graph, &[0, 1, 2]);
    *graph.tables.port_alignment.row_mut(all[0]) = PortAlignment::End;

    assert_eq!(graph.port_alignment_of(all[0]), Some(PortAlignment::End));
    assert_eq!(graph.port_alignment_of(all[1]), None);
    assert_eq!(graph.port_alignment_of(all[2]), None);
}

// Mutation: swap `x` and `y`, or `width` and `height`, in `port_box`.
#[test]
fn port_box_reads_the_position_and_size_from_its_four_numbers() {
    let (position, size) = port_box(1.0, 2.0, 3.0, 4.0);
    assert_eq!(position, LPoint { x: 1.0, y: 2.0 });
    assert_eq!(
        size,
        LSize {
            width: 3.0,
            height: 4.0
        }
    );
}

// Mutation: swap the two arms of `is_across_flow`, or give an undefined direction the
// horizontal arm. A case of the table then fails.
#[test]
fn a_side_across_the_flow_is_a_north_or_south_side_of_a_horizontal_layout() {
    use ElkDirection::{Down, Left, Right, Undefined, Up};
    use PortSide::{East, North, South, West};
    for (direction, across) in [
        (Left, [North, South]),
        (Right, [North, South]),
        (Up, [East, West]),
        (Down, [East, West]),
    ] {
        for side in [North, East, South, West] {
            assert_eq!(
                is_across_flow(direction, side),
                across.contains(&side),
                "{direction:?} {side:?}"
            );
        }
    }
    // No flow, so nothing lies across it. A port without a side lies across no flow either.
    for side in [North, East, South, West, PortSide::Undefined] {
        assert!(!is_across_flow(Undefined, side), "{side:?}");
    }
    for direction in [Left, Right, Up, Down] {
        assert!(!is_across_flow(direction, PortSide::Undefined));
    }
}

#[test]
fn external_port_dummy_uses_direction_and_net_flow_for_free_ports() {
    let dummy = create_external_port_dummy(
        NodeId::new(0),
        "external:A",
        "external:A:0",
        PortType::Input,
        PortConstraints::Free,
        PortSide::Undefined,
        1,
        LPoint::default(),
        LSize {
            width: 4.0,
            height: 6.0,
        },
        LSize::default(),
        0.0,
        ElkDirection::Down,
        ExternalPortData::default(),
    );

    assert_eq!(dummy.kind(), LNodeKind::ExternalPort);
    assert_eq!(
        dummy.external_port().unwrap().in_layer_constraint,
        InLayerConstraint::Bottom
    );
    assert_eq!(dummy.port_constraints, PortConstraints::FixedPos);
    assert_eq!(dummy.ports[PortId::new(0)].side, PortSide::North);
    assert_eq!(
        dummy.ports[PortId::new(0)].position,
        LPoint { x: 2.0, y: 0.0 }
    );
}

// Mutation: drop the loop in `push_node_with` that sets the owner of each port, or
// set it to `NodeId::new(0)`.
#[test]
fn pushed_node_gets_the_id_its_port_names() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.layerless_nodes.push(LNode::new("a", 1.0, 1.0, None));
    graph.layerless_nodes.push(LNode::new("b", 1.0, 1.0, None));
    let id = graph.push_node_with(|_| {
        // A wrong owner on purpose: the method must repair it.
        create_external_port_dummy(
            NodeId::new(0),
            "external:A",
            "external:A:0",
            PortType::Output,
            PortConstraints::Free,
            PortSide::Undefined,
            -1,
            LPoint::default(),
            LSize::default(),
            LSize::default(),
            0.0,
            ElkDirection::Right,
            ExternalPortData::default(),
        )
    });
    assert_eq!(id, NodeId::new(2));
    assert_eq!(graph.layerless_nodes.len(), 3);
    assert_eq!(graph.layerless_nodes[id].kind(), LNodeKind::ExternalPort);
    assert_eq!(graph.layerless_nodes[id].ports[PortId::new(0)].node, id);
}

// Mutation: build the dummy's port with `NodeId::new(0)` instead of the `node` argument.
#[test]
fn external_port_dummy_port_names_the_given_node_as_owner() {
    let dummy = create_external_port_dummy(
        NodeId::new(5),
        "external:A",
        "external:A:0",
        PortType::Output,
        PortConstraints::Free,
        PortSide::Undefined,
        -1,
        LPoint::default(),
        LSize::default(),
        LSize::default(),
        0.0,
        ElkDirection::Right,
        ExternalPortData::default(),
    );

    assert_eq!(dummy.ports[PortId::new(0)].node, NodeId::new(5));
}

#[test]
fn external_port_dummy_keeps_fixed_side_and_order_metadata() {
    let dummy = create_external_port_dummy(
        NodeId::new(0),
        "external:A",
        "external:A:0",
        PortType::Output,
        PortConstraints::FixedRatio,
        PortSide::West,
        -1,
        LPoint { x: 0.0, y: 40.0 },
        LSize {
            width: 4.0,
            height: 6.0,
        },
        LSize {
            width: 20.0,
            height: 80.0,
        },
        -3.0,
        ElkDirection::Right,
        ExternalPortData::default(),
    );

    assert_eq!(dummy.layer_constraint, LayerConstraint::FirstSeparate);
    assert_eq!(dummy.size.width, 3.0);
    assert_eq!(dummy.size.height, 6.0);
    assert_eq!(dummy.ports[PortId::new(0)].side, PortSide::East);
    assert_eq!(
        dummy.ports[PortId::new(0)].position,
        LPoint { x: 0.0, y: 3.0 }
    );
    assert_eq!(
        dummy.ports[PortId::new(0)].anchor,
        LPoint { x: 0.0, y: 0.0 }
    );
    assert_eq!(dummy.ports[PortId::new(0)].size, LSize::default());
    assert_eq!(
        dummy.external_port().unwrap().size,
        LSize {
            width: 4.0,
            height: 6.0
        }
    );
    assert_eq!(dummy.ports[PortId::new(0)].border_offset, Some(-3.0));
    assert_eq!(dummy.external_port().unwrap().ratio_or_position, 0.5);
}

// Mutation: look a port up by its place in the node's list instead of by its slot.
#[test]
fn reordering_ports_leaves_the_origin_of_a_nested_external_dummy_alone() {
    let mut parent = LGraph::new("root", LayeredOptions::default());
    let mut compound = LNode::new("cluster", 10.0, 10.0, Some(0));
    compound
        .ports
        .push(LPort::new("p0", NodeId::new(0), PortType::Output));
    compound
        .ports
        .push(LPort::new("p1", NodeId::new(0), PortType::Output));

    let mut nested = LGraph::new("cluster", LayeredOptions::default());
    let mut dummy = LNode::new("external:cluster", 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::ExternalPort);
    let origin = GraphPortRef {
        graph_id: GraphId::ROOT,
        port: PortRef {
            node: NodeId::new(0),
            port: PortId::new(1),
        },
    };
    dummy.external_port_mut().unwrap().origin_port = Some(origin);
    nested.layerless_nodes.push(dummy);
    parent.layerless_nodes.push(compound);
    let mut graphs = GraphArena::new(parent);
    let nested_id = graphs.nest(GraphId::ROOT, NodeId::new(0), nested);

    assert!(
        graphs
            .root_mut()
            .reorder_node_ports(NodeId::new(0), port_ids([1, 0]))
    );

    let nested = &graphs[nested_id];
    let parent = graphs.root();
    assert_eq!(
        nested.layerless_nodes[NodeId::new(0)]
            .external_port()
            .unwrap()
            .origin_port,
        Some(origin)
    );
    // The origin still names the port "p1", which now comes first.
    let compound = &parent.layerless_nodes[NodeId::new(0)];
    assert_eq!(compound.ports[PortId::new(1)].id, "p1");
    assert_eq!(compound.ports.position(PortId::new(1)), Some(0));
}

// Mutation: keep the ports of every side, or list them in reverse.
#[test]
fn ports_on_side_lists_the_ports_of_one_side_in_list_order() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("A", 10.0, 10.0, Some(0)));
    for side in [
        PortSide::North,
        PortSide::East,
        PortSide::North,
        PortSide::South,
    ] {
        graph
            .add_port(NodeId::new(0), PortType::Output, side, LPoint::default())
            .unwrap();
    }

    assert_eq!(
        graph.ports_on_side(NodeId::new(0), PortSide::North),
        [
            PortRef {
                node: NodeId::new(0),
                port: PortId::new(0)
            },
            PortRef {
                node: NodeId::new(0),
                port: PortId::new(2)
            }
        ]
    );
    assert_eq!(
        graph.ports_on_side(NodeId::new(0), PortSide::South),
        [PortRef {
            node: NodeId::new(0),
            port: PortId::new(3)
        }]
    );
    assert!(
        graph
            .ports_on_side(NodeId::new(0), PortSide::West)
            .is_empty()
    );
}

// Mutation: return `true` without rewriting the node's list.
#[test]
fn reorder_node_ports_puts_the_ports_in_the_given_order() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let mut node = LNode::new("A", 10.0, 10.0, Some(0));
    node.ports
        .push(LPort::new("p0", NodeId::new(0), PortType::Output));
    node.ports
        .push(LPort::new("p1", NodeId::new(0), PortType::Output));
    node.ports
        .push(LPort::new("p2", NodeId::new(0), PortType::Output));
    graph.layerless_nodes.push(node);

    assert!(graph.reorder_node_ports(NodeId::new(0), port_ids([2, 0, 1])));

    let ports = &graph.layerless_nodes[NodeId::new(0)].ports;
    assert_eq!(
        ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["p2", "p0", "p1"]
    );
    assert_eq!(ports.ids().collect::<Vec<_>>(), port_ids([2, 0, 1]));
}

// Mutation: accept an order that leaves out a port, repeats one or names one the node lacks.
#[test]
fn reorder_node_ports_refuses_an_order_that_is_not_a_permutation() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let mut node = LNode::new("A", 10.0, 10.0, Some(0));
    for id in ["p0", "p1", "p2"] {
        node.ports
            .push(LPort::new(id, NodeId::new(0), PortType::Output));
    }
    graph.layerless_nodes.push(node);
    let before = graph.layerless_nodes[NodeId::new(0)].ports.clone();

    assert!(!graph.reorder_node_ports(NodeId::new(0), port_ids([0, 1])));
    assert!(!graph.reorder_node_ports(NodeId::new(0), port_ids([0, 1, 1])));
    assert!(!graph.reorder_node_ports(NodeId::new(0), port_ids([0, 1, 3])));
    assert!(!graph.reorder_node_ports(NodeId::new(0), port_ids([0, 1, 2, 2])));
    assert!(!graph.reorder_node_ports(NodeId::new(7), port_ids([0, 1, 2])));
    assert_eq!(graph.layerless_nodes[NodeId::new(0)].ports, before);
}

// Mutation: look a port up by its place in the node's list instead of by its slot. The edges
// then name the port that stands in the old place.
// Mutation: in `reorder_node_ports`, clear `origin_port` of every external port dummy, or the
// `source` of every long edge dummy's ends.
#[test]
fn reordering_ports_changes_no_edge_and_no_other_reference() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("A", 10.0, 10.0, Some(0)));
    graph
        .layerless_nodes
        .push(LNode::new("B", 10.0, 10.0, Some(1)));
    graph
        .layerless_nodes
        .push(LNode::new("C", 10.0, 10.0, Some(2)));
    let sides = [PortSide::East, PortSide::North, PortSide::South];
    let a = sides.map(|side| {
        graph
            .add_port(NodeId::new(0), PortType::Output, side, LPoint::default())
            .unwrap()
    });
    let b = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            LPoint::default(),
        )
        .unwrap();
    let edge = |id: &str, source: PortRef, target: PortRef| LayeredEdge {
        id: id.to_string(),
        source,
        target,
        source_node_id: String::new(),
        target_node_id: String::new(),
        labels: Vec::new(),
        minlen: 1,
        reversed: false,
        bend_points: Vec::new(),
        junction_points: Vec::new(),
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
        thickness: 0.0,
        original_opposite_port: Some(a[1]),
        compound_segment: None,
        inside_self_loop: false,
    };
    graph.add_edge(edge("e0", a[0], b));
    graph.add_edge(edge("e1", a[2], b));
    graph.add_edge(edge("e2", a[1], a[2]));
    graph.layerless_nodes[NodeId::new(1)].set_kind(LNodeKind::LongEdge);
    graph.layerless_nodes[NodeId::new(1)]
        .long_edge_ends_mut()
        .unwrap()
        .source = Some(a[0]);
    graph.layerless_nodes[NodeId::new(1)]
        .long_edge_ends_mut()
        .unwrap()
        .target = Some(a[2]);
    graph.layerless_nodes[NodeId::new(2)].set_kind(LNodeKind::ExternalPort);
    graph.layerless_nodes[NodeId::new(2)]
        .external_port_mut()
        .unwrap()
        .origin_port = Some(GraphPortRef {
        graph_id: GraphId::ROOT,
        port: a[1],
    });
    let edges_before = graph.edges.clone();
    let nodes_before = [
        graph.layerless_nodes[NodeId::new(1)].clone(),
        graph.layerless_nodes[NodeId::new(2)].clone(),
    ];

    assert!(graph.reorder_node_ports(NodeId::new(0), [a[2].port, a[0].port, a[1].port]));

    // No edge and no reference of the other node changed.
    assert_eq!(graph.edges, edges_before);
    assert_eq!(graph.layerless_nodes[NodeId::new(1)], nodes_before[0]);
    assert_eq!(graph.layerless_nodes[NodeId::new(2)], nodes_before[1]);
    // Each port still holds the edges it held, under the same id.
    for (port, outgoing) in [(a[0], "e0"), (a[2], "e1"), (a[1], "e2")] {
        let data = &graph.layerless_nodes[NodeId::new(0)].ports[port.port];
        assert_eq!(data.outgoing_edges.len(), 1, "{outgoing}");
        assert_eq!(graph.edges[data.outgoing_edges[0]].id, outgoing);
    }
    // Only the list changed: the sides follow the ports, not the places.
    let listed = graph.layerless_nodes[NodeId::new(0)]
        .ports
        .iter()
        .map(|port| port.side)
        .collect::<Vec<_>>();
    assert_eq!(listed, [PortSide::South, PortSide::East, PortSide::North]);
}

// --- `compute_inside_part`: ElkUtil.computeInsidePart. ---

fn inside_part(side: PortSide, position: (f64, f64), offset: f64) -> f64 {
    compute_inside_part(
        LPoint {
            x: position.0,
            y: position.1,
        },
        LSize {
            width: 30.0,
            height: 14.0,
        },
        LSize {
            width: 6.0,
            height: 4.0,
        },
        offset,
        side,
    )
}

// Mutation: in the north arm, read the label width, or leave out the port height.
#[test]
fn the_inside_part_of_a_label_on_a_north_port_is_what_reaches_below_the_port() {
    // 14 + 1 - (4 + 0) = 11.
    assert_eq!(inside_part(PortSide::North, (0.0, 1.0), 0.0), 11.0);
    // A label above the port's bottom edge has no inside part: 14 - 20 - 4 < 0.
    assert_eq!(inside_part(PortSide::North, (0.0, -20.0), 0.0), 0.0);
    // The border offset moves the node's border: 14 + 1 - (4 + 2) = 9.
    assert_eq!(inside_part(PortSide::North, (0.0, 1.0), 2.0), 9.0);
}

// Mutation: in the south arm, add the label height, as the north arm does.
#[test]
fn the_inside_part_of_a_label_on_a_south_port_is_what_reaches_above_the_port() {
    // -(-3) - 0 = 3.
    assert_eq!(inside_part(PortSide::South, (0.0, -3.0), 0.0), 3.0);
    assert_eq!(inside_part(PortSide::South, (0.0, 3.0), 0.0), 0.0);
    assert_eq!(inside_part(PortSide::South, (0.0, -3.0), 1.5), 1.5);
}

// Mutation: in the east arm, read the y position.
#[test]
fn the_inside_part_of_a_label_on_an_east_port_is_what_reaches_left_of_the_port() {
    assert_eq!(inside_part(PortSide::East, (-10.0, 0.0), 0.0), 10.0);
    assert_eq!(inside_part(PortSide::East, (4.0, 0.0), 0.0), 0.0);
    assert_eq!(inside_part(PortSide::East, (-10.0, 0.0), 4.0), 6.0);
}

// Mutation: in the west arm, read the label height, or leave out the port width.
#[test]
fn the_inside_part_of_a_label_on_a_west_port_is_what_reaches_right_of_the_port() {
    // 30 + 1 - (6 + 0) = 25.
    assert_eq!(inside_part(PortSide::West, (1.0, 0.0), 0.0), 25.0);
    assert_eq!(inside_part(PortSide::West, (-40.0, 0.0), 0.0), 0.0);
}

// Mutation: let an undefined side count as north.
#[test]
fn a_port_without_a_side_has_no_inside_part() {
    assert_eq!(inside_part(PortSide::Undefined, (-10.0, 1.0), 0.0), 0.0);
}
