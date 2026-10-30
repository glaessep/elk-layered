use super::*;
use crate::compound::preprocess_source_ported_compound_graph;
use crate::graph::LNodeKind;
use crate::options::OrderingStrategy;
use crate::options::SpacingOptions;

fn node(id: &str) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width: 80.0,
        height: 40.0,
        parent: None,
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_alignment: None,
        port_constraints: None,
        node_label_placement: NodeLabelPlacement::FIXED,
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: None,
        compound_node: false,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
        port_label_placement: Default::default(),
        port_labels_treat_as_group: true,
    }
}

fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
    ElkInputEdge {
        source_port: None,
        target_port: None,
        id: id.to_string(),
        source: source.to_string(),
        target: target.to_string(),
        labels: Vec::new(),
        minlen: 1,
        inside_self_loop: false,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
        container: None,
    }
}

fn graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> ElkInputGraph {
    ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    }
}

#[test]
fn parent_cycle_reports_the_revisited_cycle_member_after_a_stem() {
    let mut stem = node("stem");
    stem.parent = Some("A".to_string());
    let mut a = node("A");
    a.parent = Some("B".to_string());
    let mut b = node("B");
    b.parent = Some("A".to_string());
    let input = graph(vec![stem, a, b], vec![]);
    let node_order = input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();

    assert!(matches!(
        detect_parent_cycles(&input, &node_order),
        Err(ImportError::ParentCycle { node_id }) if node_id == "A"
    ));
}

// Mutation: in `transform_node`, write the self loop options of every node to the row of node 0.
#[test]
fn import_records_the_self_loop_options_of_each_node() {
    let mut first = node("A");
    first.self_loop_distribution = crate::options::SelfLoopDistributionStrategy::Equally;
    first.self_loop_ordering = crate::options::SelfLoopOrderingStrategy::ReverseStacked;
    let mut second = node("B");
    second.self_loop_distribution = crate::options::SelfLoopDistributionStrategy::North;
    second.self_loop_ordering = crate::options::SelfLoopOrderingStrategy::Sequenced;
    let imported = import_graph(&graph(vec![first, second], vec![])).unwrap();

    let root = imported.root();
    let options = |name: &str| {
        root.tables
            .self_loop_options
            .get(root.node_named(name))
            .unwrap()
    };

    assert_eq!(
        options("A").distribution,
        crate::options::SelfLoopDistributionStrategy::Equally
    );
    assert_eq!(
        options("A").ordering,
        crate::options::SelfLoopOrderingStrategy::ReverseStacked
    );
    assert_eq!(
        options("B").distribution,
        crate::options::SelfLoopDistributionStrategy::North
    );
    assert_eq!(
        options("B").ordering,
        crate::options::SelfLoopOrderingStrategy::Sequenced
    );
}

// Mutation: in `transform_node`, write the port alignment or the placement options of every node
// to the row of node 0, or skip `set_layer_constraint`, or call it for every node.
#[test]
fn import_records_the_options_the_processors_read() {
    let mut first = node("A");
    first.port_alignment = Some(crate::options::PortAlignment::End);
    first.node_flexibility = crate::options::NodeFlexibility::PortPosition;
    first.ports_surrounding = Some(crate::options::SpacingMargin {
        top: 1.0,
        right: 2.0,
        bottom: 3.0,
        left: 4.0,
    });
    first.layer_constraint = Some(crate::options::LayerConstraint::First);
    let second = node("B");
    let imported = import_graph(&graph(vec![first, second], vec![])).unwrap();

    let root = imported.root();
    let (a, b) = (root.node_named("A"), root.node_named("B"));

    assert_eq!(
        root.port_alignment_of(a),
        Some(crate::options::PortAlignment::End)
    );
    assert_eq!(root.port_alignment_of(b), None);
    let placement = root.tables.placement_options.get(a).unwrap();
    assert_eq!(
        placement.flexibility,
        crate::options::NodeFlexibility::PortPosition
    );
    assert_eq!(
        placement.ports_surrounding.map(|margin| margin.left),
        Some(4.0)
    );
    assert_eq!(
        root.tables.placement_options.get(b).unwrap().flexibility,
        crate::options::NodeFlexibility::None
    );
    assert!(root.layerless_nodes[a].is_layer_constraint_explicit());
    assert!(!root.layerless_nodes[b].is_layer_constraint_explicit());
}

#[test]
fn import_rejects_duplicate_input_edge_ids_before_endpoint_validation() {
    let input = graph(
        vec![node("A"), node("B")],
        vec![
            edge("duplicate", "A", "B"),
            edge("duplicate", "missing", "B"),
        ],
    );

    let error = import_graph(&input).unwrap_err();

    assert_eq!(
        error,
        ImportError::DuplicateEdge {
            id: "duplicate".to_string(),
        }
    );
}

// Mutation: in `import_hierarchical_graph`, queue the children of a node with the graph slot of
// the node itself (`parent_slot`), not the slot of its nested graph. The chain then stays in
// one graph.
#[test]
fn hierarchical_import_uses_linear_graph_slots_for_deep_chains() {
    for node_count in [1usize, 4, 16, 64, 256] {
        let mut nodes = Vec::with_capacity(node_count);
        for index in 0..node_count {
            let mut current = node(format!("node-{index}").as_str());
            current.parent = index.checked_sub(1).map(|parent| format!("node-{parent}"));
            current.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            nodes.push(current);
        }
        let mut input = graph(nodes, Vec::new());
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let imported = import_graph(&input).unwrap();

        let mut graph: &LGraph = &imported;
        for index in 0..node_count {
            assert_eq!(graph.layerless_nodes.len(), 1);
            assert_eq!(
                graph.layerless_nodes[NodeId::new(0)].id,
                format!("node-{index}")
            );
            if index + 1 < node_count {
                graph = graph.layerless_nodes[NodeId::new(0)]
                    .nested_graph
                    .map(|id| &imported[id])
                    .expect("every non-leaf chain node owns the next graph slot");
            } else {
                assert!(graph.layerless_nodes[NodeId::new(0)].nested_graph.is_none());
            }
        }
    }
}

#[test]
fn hierarchical_import_keeps_the_full_path_of_each_cross_edge() {
    for node_count in [2usize, 8, 32] {
        for edge_count in [1usize, 4, 16] {
            let mut nodes = Vec::with_capacity(node_count);
            for index in 0..node_count {
                let mut current = node(format!("node-{index}").as_str());
                current.parent = index.checked_sub(1).map(|parent| format!("node-{parent}"));
                current.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                nodes.push(current);
            }
            let edges = (0..edge_count)
                .map(|index| {
                    edge(
                        format!("edge-{index}").as_str(),
                        "node-0",
                        format!("node-{}", node_count - 1).as_str(),
                    )
                })
                .collect::<Vec<_>>();
            let mut input = graph(nodes, edges);
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

            let imported = import_graph(&input).unwrap();

            assert_eq!(imported.hierarchy_edges.len(), edge_count);
            assert!(imported.hierarchy_edges.iter().all(|edge| {
                edge.source_path.is_empty() && edge.target_path.len() == node_count - 1
            }));
            assert_eq!(
                imported
                    .hierarchy_edges
                    .iter()
                    .map(|edge| edge.id.clone())
                    .collect::<Vec<_>>(),
                (0..edge_count)
                    .map(|index| format!("edge-{index}"))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn hierarchy_query_index_answers_ancestry_across_branches_and_forests() {
    let mut left = node("left");
    left.parent = Some("root-a".to_string());
    let mut left_leaf = node("left-leaf");
    left_leaf.parent = Some("left".to_string());
    let mut right = node("right");
    right.parent = Some("root-a".to_string());
    let mut right_leaf = node("right-leaf");
    right_leaf.parent = Some("right".to_string());
    let input = graph(
        vec![
            node("root-a"),
            left,
            left_leaf,
            right,
            right_leaf,
            node("root-b"),
        ],
        Vec::new(),
    );
    let index = InputIndex::new(&input, &[]).unwrap();
    let root_a = index.node_index("root-a").unwrap();
    let left_leaf = index.node_index("left-leaf").unwrap();
    let right_leaf = index.node_index("right-leaf").unwrap();
    let root_b = index.node_index("root-b").unwrap();

    let queries = &index.hierarchy_queries;

    assert!(queries.is_ancestor(root_a, left_leaf));
    assert!(queries.is_ancestor(root_a, right_leaf));
    assert!(queries.is_ancestor(root_a, root_a));
    assert!(!queries.is_ancestor(left_leaf, root_a));
    assert!(!queries.is_ancestor(root_a, root_b));
    assert!(!queries.is_ancestor(root_b, left_leaf));
}

/// A node whose own options set `insideSelfLoops.activate`, as `layout.rs` passes them in
/// `nested_options`. The importer does not read the option for a loop: the edge carries that
/// decision in `inside_self_loop`.
fn activating(mut node: ElkInputNode) -> ElkInputNode {
    node.nested_options = Some(Box::new(LayeredOptions {
        inside_self_loops_activate: true,
        ..Default::default()
    }));
    node
}

// Mutation: in `import_hierarchical_graph`, queue the children of every node that gets a nested
// graph (`if true`). The dummy-only graph of `group` then holds `child`.
#[test]
fn separate_children_self_loop_graph_does_not_materialize_regular_children() {
    let mut group = activating(node("group"));
    group.hierarchy_handling = Some(HierarchyHandling::SeparateChildren);
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let loop_edge = ElkInputEdge {
        source_port: None,
        target_port: None,
        inside_self_loop: true,
        ..edge("group-loop", "group", "group")
    };
    let mut input = graph(vec![group, child], vec![loop_edge]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

    let imported = import_graph(&input).unwrap();
    let nested = imported.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .map(|id| &imported[id])
        .expect("inside self-loop creates a nested graph");
    assert!(nested.layerless_nodes.iter().all(|node| node.id != "child"));
}

// Mutation: build the map whatever `with_edge_containers` says. The import for a caller that
// does not read the map then still pays for it.
#[test]
fn the_edge_containers_are_built_only_when_asked_for() {
    let input = graph(vec![node("a"), node("b")], vec![edge("e", "a", "b")]);
    let scope = || (ElkPadding::uniform(0.0), ScopePorts::none());

    let (padding, ports) = scope();
    let without = import_scope_with(&input, &[], padding, ports, false).unwrap();
    let (padding, ports) = scope();
    let with = import_scope_with(&input, &[], padding, ports, true).unwrap();

    assert!(without.edge_containers.is_empty());
    assert_eq!(with.edge_containers["e"], GraphId::ROOT);
}

// Mutation: put every edge in `GraphId::ROOT`, or look the container up by the scope's id
// first. The edge between the children of `group` then maps to the root, not to the graph
// inside `group`. The scope is named "group" too, as a Mermaid adapter root may be.
#[test]
fn an_edge_maps_to_the_graph_of_its_container_even_when_the_scope_has_that_name() {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut a = node("a");
    a.parent = Some("group".to_string());
    let mut b = node("b");
    b.parent = Some("group".to_string());
    let mut input = graph(
        vec![group, a, b, node("outside")],
        vec![edge("inner", "a", "b"), edge("top", "group", "outside")],
    );
    input.id = "group".to_string();
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

    let imported = import_scope(&input, &[], ElkPadding::uniform(0.0), ScopePorts::none()).unwrap();

    let group_graph = imported.graphs.root().layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("group holds a graph");
    assert_ne!(group_graph, GraphId::ROOT);
    assert_eq!(imported.edge_containers["inner"], group_graph);
    assert_eq!(imported.edge_containers["top"], GraphId::ROOT);
}

// Mutation: read a named container without a graph as `GraphId::ROOT` again, as
// `graph_of` does. The import then succeeds and the edge gets the root's coordinates.
#[test]
fn an_edge_whose_container_holds_no_graph_is_refused() {
    let mut contained = edge("e", "a", "b");
    contained.container = Some("a".to_string());
    // Flat hierarchy handling: no node gets a graph.
    let input = graph(vec![node("a"), node("b")], vec![contained]);

    let error = import_scope(&input, &[], ElkPadding::uniform(0.0), ScopePorts::none())
        .err()
        .expect("an error");

    assert_eq!(
        error,
        ImportError::ContainerWithoutGraph {
            edge_id: "e".to_string(),
            container: "a".to_string(),
        }
    );
}

#[test]
fn scoped_pieces_may_reference_the_same_logical_edge_id() {
    let input = graph(vec![node("A"), node("B")], Vec::new());
    let segment = |edge_order| ElkInputEdgeSegment {
        edge: edge("logical-edge", "A", "B"),
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "A".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "B".to_string(),
        },
        segment: CompoundEdgeSegment::Output { depth: 0 },
        edge_order,
    };

    let imported = import_graph_with_segments(&input, &[segment(0), segment(1)]).unwrap();

    assert_eq!(imported.edges.len(), 2);
    assert_eq!(imported.cross_hierarchy_edges.len(), 2);
    assert!(
        imported
            .cross_hierarchy_edges
            .iter()
            .all(|segment| segment.original_edge_id == "logical-edge")
    );
}

fn cross_edge_import_input(merge_edges: bool, edge_count: usize) -> ElkInputGraph {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    group.nested_options = Some(Box::new(LayeredOptions {
        merge_edges,
        ..Default::default()
    }));
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let edges = (0..edge_count)
        .map(|index| edge(format!("edge-{index}").as_str(), "child", "outer"))
        .collect::<Vec<_>>();
    let mut input = graph(vec![group, child, node("outer")], edges);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    input.options.merge_edges = merge_edges;
    input
}

// Mutation: in `ensure_hierarchy_endpoint_port`, skip the shared-port branch (`if false && …`).
// Each of the 32 edges then gets its own port.
#[test]
fn merged_cross_edges_share_one_port_per_endpoint() {
    let edge_count = 32usize;

    let imported = import_graph(&cross_edge_import_input(true, edge_count)).unwrap();

    let outer = imported
        .layerless_nodes
        .iter()
        .find(|node| node.id == "outer")
        .unwrap();
    assert_eq!(outer.ports.len(), 1);
    let nested = &imported[imported.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .unwrap()];
    let child = nested
        .layerless_nodes
        .iter()
        .find(|node| node.id == "child")
        .unwrap();
    assert_eq!(child.ports.len(), 1);
    assert_eq!(imported.hierarchy_edges.len(), edge_count);
}

#[test]
fn dedicated_cross_edge_ports_are_not_reused() {
    let edge_count = 32usize;

    let imported = import_graph(&cross_edge_import_input(false, edge_count)).unwrap();

    let outer = imported
        .layerless_nodes
        .iter()
        .find(|node| node.id == "outer")
        .unwrap();
    assert_eq!(outer.ports.len(), edge_count);
    assert_eq!(imported.hierarchy_edges.len(), edge_count);
}

// Mutation: where the importer transforms an edge, test the node's `insideSelfLoops.activate`
// again. The node here sets no option, because the caller already decided, so its loop would then
// be an ordinary self loop of the outer graph.
#[test]
fn importer_trusts_the_edge_and_does_not_read_the_node_options() {
    let graph = graph(
        vec![node("A")],
        vec![ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loop: true,
            ..edge("A-A", "A", "A")
        }],
    );

    let lgraph = import_graph(&graph).unwrap();
    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "A")
        .and_then(|node| node.nested_graph.map(|id| &lgraph[id]))
        .expect("an inside self loop creates a nested graph");
    assert!(nested.graph_properties.self_loops);
}

// Mutation: drop the `debug_assert!` in `is_inside_self_loop`. An edge between two nodes then
// becomes a fake loop in the nested graph of its source, with no error.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "joins two nodes")]
fn importer_refuses_an_inside_self_loop_flag_on_an_edge_between_two_nodes() {
    let graph = graph(
        vec![node("A"), node("B")],
        vec![ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loop: true,
            ..edge("A-B", "A", "B")
        }],
    );

    let _ = import_graph(&graph);
}

// Mutation: reuse the dummies of the first loop of a node for its later loops in
// `transform_inside_self_loop`. The loops then share one pair of dummies and the nested graph has
// 2 nodes.
#[test]
fn each_inside_self_loop_gets_its_own_pair_of_dummies() {
    let edge_count = 32usize;
    let edges = (0..edge_count)
        .map(|index| ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loop: true,
            ..edge(format!("loop-{index}").as_str(), "A", "A")
        })
        .collect::<Vec<_>>();
    let mut input = graph(vec![activating(node("A"))], edges);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

    let imported = import_graph(&input).unwrap();

    let nested = &imported[imported.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .unwrap()];
    assert_eq!(nested.layerless_nodes.len(), 2 * edge_count);
    assert_eq!(nested.edges.len(), edge_count);
}

#[test]
fn edge_labels_are_imported_with_their_edge() {
    let mut labeled_edge = edge("ab", "A", "B");
    labeled_edge.labels = (0..128)
        .map(|_| ElkInputLabel::center("terminal", 18.0, 16.5))
        .collect();
    let input = graph(vec![node("A"), node("B")], vec![labeled_edge]);

    let imported = import_graph(&input).unwrap();

    assert_eq!(imported.edges[EdgeId::new(0)].labels.len(), 128);
}

#[test]
fn scoped_segment_depth_that_overflows_when_rebased_is_an_error() {
    let mut nested_input = graph(vec![node("outer"), node("inner")], Vec::new());
    nested_input.nodes[1].parent = Some("outer".to_string());
    nested_input.nodes[0].hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    nested_input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let deep = ElkInputEdgeSegment {
        edge: edge("deep", "inner", "outer"),
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "inner".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "outer".to_string(),
        },
        segment: CompoundEdgeSegment::Output { depth: usize::MAX },
        edge_order: 0,
    };

    assert_eq!(
        import_graph_with_segments(&nested_input, &[deep]).unwrap_err(),
        ImportError::SegmentDepthOverflow {
            edge_id: "deep".to_string(),
        }
    );
}

#[test]
fn imports_mermaid_flowchart_nodes_edges_labels_and_model_order() {
    let mut a = node("A");
    a.labels = vec![ElkInputLabel::center("Alpha", 42.0, 18.0)];
    let mut ab = edge("A-B", "A", "B");
    ab.labels = vec![ElkInputLabel::center("go", 20.0, 12.0)];

    let lgraph = import_graph(&graph(vec![a, node("B")], vec![ab])).unwrap();

    assert_eq!(lgraph.layerless_nodes.len(), 2);
    assert_eq!(lgraph.layerless_nodes[NodeId::new(0)].id, "A");
    assert_eq!(lgraph.layerless_nodes[NodeId::new(0)].model_order, Some(0));
    assert_eq!(
        lgraph.layerless_nodes[NodeId::new(0)].labels[0].text,
        "Alpha"
    );
    assert_eq!(lgraph.edges.len(), 1);
    assert_eq!(lgraph.edges[EdgeId::new(0)].model_order, Some(0));
    assert_eq!(lgraph.edges[EdgeId::new(0)].thickness, 1.0);
    assert_eq!(
        lgraph.edges[EdgeId::new(0)].labels[0].placement,
        EdgeLabelPlacement::Center
    );
    assert!(lgraph.graph_properties.center_labels);
    assert!(lgraph.options.graph_has_center_labels);
}

// Mutation: make the importer ignore `inside_self_loop`.
#[test]
fn importer_creates_nested_graph_for_inside_self_loop() {
    let graph = graph(
        vec![activating(node("A"))],
        vec![ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loop: true,
            ..edge("A-A", "A", "A")
        }],
    );

    let lgraph = import_graph(&graph).unwrap();
    let node = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "A")
        .unwrap();
    assert!(node.nested_graph.is_some());
}

// Mutation: reuse the dummies of the first loop of a node for its later loops in
// `transform_inside_self_loop`. The two loops then share one pair of dummies.
#[test]
fn importer_gives_each_inside_self_loop_its_own_dummies() {
    let graph = graph(
        vec![activating(node("A"))],
        vec![
            ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loop: true,
                ..edge("A-A-1", "A", "A")
            },
            ElkInputEdge {
                source_port: None,
                target_port: None,
                inside_self_loop: true,
                ..edge("A-A-2", "A", "A")
            },
        ],
    );

    let lgraph = import_graph(&graph).unwrap();
    let node = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "A")
        .unwrap();
    let nested = &lgraph[node.nested_graph.unwrap()];
    let external_dummies = nested
        .layerless_nodes
        .iter()
        .filter(|candidate| candidate.kind() == LNodeKind::ExternalPort)
        .collect::<Vec<_>>();

    // `ElkGraphImporter.transformEdge` makes a new port for each loop, so each loop gets an input
    // dummy and an output dummy of its own.
    assert_eq!(external_dummies.len(), 4);
    for dummy in external_dummies {
        let port = &dummy.ports[PortId::new(0)];
        assert_eq!(port.incoming_edges().len() + port.outgoing_edges().len(), 1);
    }
}

// Mutation: in `transform_inside_self_loop`, drop `graph_properties.self_loops = true`.
#[test]
fn importer_marks_inside_self_loops_as_graph_self_loops() {
    let graph = graph(
        vec![activating(node("A"))],
        vec![ElkInputEdge {
            source_port: None,
            target_port: None,
            inside_self_loop: true,
            ..edge("A-A", "A", "A")
        }],
    );

    let lgraph = import_graph(&graph).unwrap();
    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "A")
        .and_then(|node| node.nested_graph.map(|id| &lgraph[id]))
        .expect("inside self-loop should create a nested graph");

    assert!(nested.graph_properties.self_loops);
    assert!(nested.options.graph_has_self_loops);
}

#[test]
fn importer_applies_layered_padding_option_to_lgraph_padding() {
    let mut input = graph(vec![node("A")], vec![]);
    input.options.padding = ElkPadding {
        top: 7.0,
        right: 8.0,
        bottom: 9.0,
        left: 10.0,
    };

    let lgraph = import_graph(&input).unwrap();

    assert_eq!(lgraph.padding.top, 7.0);
    assert_eq!(lgraph.padding.right, 8.0);
    assert_eq!(lgraph.padding.bottom, 9.0);
    assert_eq!(lgraph.padding.left, 10.0);
}

// Mutation: in `import_hierarchical_graph`, drop the `apply_graph_padding_from_options` call
// for the nested graph.
#[test]
fn importer_applies_layered_padding_option_to_nested_graphs() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
    let nested = &lgraph[lgraph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];

    assert_eq!(lgraph.padding.top, 12.0);
    assert_eq!(lgraph.padding.left, 12.0);
    assert_eq!(nested.padding.top, 12.0);
    assert_eq!(nested.padding.left, 12.0);
}

// Mutation: in `import_hierarchical_graph`, drop the `add_padding` call with
// `inside_node_label_padding`.
#[test]
fn importer_adds_inside_top_node_label_padding_to_nested_graphs() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    cluster.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopCenter);
    cluster.labels = vec![ElkInputLabel::center("Cluster", 64.0, 22.0)];
    cluster.nested_options = Some(Box::new(LayeredOptions {
        spacing: SpacingOptions::layered_base_value(30.0),
        ..Default::default()
    }));
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
    let nested = &lgraph[lgraph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];

    assert_eq!(nested.options.spacing.node_node, 30.0);
    assert_eq!(nested.padding.top, 39.0);
    assert_eq!(nested.padding.right, 12.0);
    assert_eq!(nested.padding.bottom, 12.0);
    assert_eq!(nested.padding.left, 12.0);
}

// Mutation: in `nested_graph_options`, ignore `node.nested_options` and start from the default
// options.
#[test]
fn importer_applies_nested_hierarchy_merge_without_cloning_root_options() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    cluster.nested_options = Some(Box::new(LayeredOptions {
        spacing: SpacingOptions::layered_base_value(30.0),
        layering_strategy: crate::options::LayeringStrategy::LongestPath,
        random_seed: 17,
        ..Default::default()
    }));
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut input = graph(vec![cluster, child], vec![]);
    input.options.merge_hierarchy_edges = false;
    input.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
    input.options.spacing = SpacingOptions::layered_base_value(40.0);
    input.options.layering_strategy = crate::options::LayeringStrategy::CoffmanGraham;
    input.options.random_seed = 42;

    let lgraph = import_graph(&input).unwrap();
    let nested = &lgraph[lgraph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];

    assert_eq!(lgraph.layerless_nodes[NodeId::new(0)].model_order, Some(0));
    assert_eq!(nested.layerless_nodes[NodeId::new(0)].model_order, None);
    assert!(nested.options.merge_hierarchy_edges);
    assert_eq!(
        nested.options.consider_model_order_strategy,
        OrderingStrategy::None
    );
    assert_eq!(nested.options.spacing.node_node, 30.0);
    assert_eq!(
        nested.options.layering_strategy,
        crate::options::LayeringStrategy::LongestPath
    );
    assert_eq!(nested.options.random_seed, 17);
}

// Mutation: in `nested_graph_options`, set `port_constraints` to `Free` whatever the node says.
// The nested options then lose `FixedSide`.
#[test]
fn importer_copies_node_port_constraints_to_nested_graph_options() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    cluster.port_constraints = Some(PortConstraints::FixedSide);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let lgraph = import_graph(&graph(vec![cluster, child], vec![])).unwrap();
    let nested = &lgraph[lgraph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];

    assert_eq!(nested.options.port_constraints, PortConstraints::FixedSide);
}

// Mutation: in `nested_graph_options`, drop the line that sets `options.direction` from the
// parent.
#[test]
fn importer_forces_nested_direction_to_parent_direction() {
    let mut cluster = node("cluster");
    cluster.direction = Some(ElkDirection::Left);
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut input = graph(vec![cluster, child], vec![]);
    input.options.direction = ElkDirection::Down;

    let lgraph = import_graph(&input).unwrap();
    let nested = &lgraph[lgraph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];

    assert_eq!(nested.options.direction, ElkDirection::Down);
}

// Mutation: in `resolve_root_hierarchy_handling`, resolve `Inherit` to `IncludeChildren`.
#[test]
fn importer_resolves_root_hierarchy_inherit_to_separate_children() {
    let mut input = graph(vec![node("cluster"), node("A")], vec![]);
    input.options.hierarchy_handling = HierarchyHandling::Inherit;
    input.nodes[1].parent = Some("cluster".to_string());

    let lgraph = import_graph(&input).unwrap();

    assert_eq!(
        lgraph.options.hierarchy_handling,
        HierarchyHandling::SeparateChildren
    );
    assert!(
        lgraph
            .layerless_nodes
            .iter()
            .all(|node| node.nested_graph.is_none())
    );
}

// Mutation: let `GraphArena::nest` record the node after the holder (`node + 1`) as the parent.
// `nested.parent` then no longer names the cluster.
#[test]
fn imports_include_children_hierarchy_into_nested_graphs() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let lgraph = import_graph(&graph(vec![cluster, child, node("B")], vec![])).unwrap();

    let cluster = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    assert_eq!(
        nested.parent,
        Some(crate::graph::GraphParent {
            graph: GraphId::ROOT,
            node: NodeId::new(
                lgraph
                    .layerless_nodes
                    .iter()
                    .position(|n| n.id == "cluster")
                    .unwrap()
            ),
        })
    );
    assert_eq!(nested.layerless_nodes[NodeId::new(0)].id, "A");
}

// Mutation: give every `HierarchyStep` the root as its graph in `transform_cross_hierarchy_edge`.
// The step then does not name the graph inside the cluster.
#[test]
fn importer_preserves_descendant_edge_for_compound_preprocessor() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let lgraph = import_graph(&graph(
        vec![cluster, child],
        vec![edge("cluster-A", "cluster", "A")],
    ))
    .unwrap();
    let cluster = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    assert!(nested.edges.is_empty());
    assert_eq!(lgraph.hierarchy_edges.len(), 1);
    assert_eq!(lgraph.hierarchy_edges[0].id, "cluster-A");
    assert_eq!(lgraph.hierarchy_edges[0].source_node_id, "cluster");
    assert_eq!(lgraph.hierarchy_edges[0].target_node_id, "A");
    assert_eq!(lgraph.hierarchy_edges[0].source_path, Vec::new());
    assert_eq!(
        lgraph.hierarchy_edges[0].target_path,
        vec![HierarchyStep {
            node_id: "cluster".to_string(),
            graph: nested.id,
        }]
    );
}

// Mutation: in `hierarchy_port_key`, write the key as `node:role:order` instead of
// `node:order:role`. The port then reads `A:source:0`, not `A:0:source`.
#[test]
fn cross_hierarchy_import_creates_endpoint_ports_in_input_order() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut sibling = node("C");
    sibling.parent = Some("cluster".to_string());

    let lgraph = import_graph(&graph(
        vec![cluster, child, sibling, node("B")],
        vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
    ))
    .unwrap();

    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap()
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap();
    let child = nested
        .layerless_nodes
        .iter()
        .find(|node| node.id == "A")
        .unwrap();

    assert_eq!(
        child
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["A:0:source", "A:1"]
    );
    assert_eq!(nested.edges[EdgeId::new(0)].source().port, PortId::new(1));
    assert_eq!(lgraph.hierarchy_edges[0].source_port_key, "A:0:source");
}

// Mutation: in `source_ported_cross_hierarchy_segments`, set `connects_parent_node` to `true`
// for the output steps.
#[test]
fn source_ported_compound_metadata_links_parent_port_and_external_dummy() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![cluster, child, node("B")],
        vec![edge("A-B", "A", "B")],
    ))
    .unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let cluster_index = lgraph.node_named("cluster");
    let cluster = &lgraph.layerless_nodes[cluster_index];
    let parent_port = &cluster.ports[PortId::new(0)];
    let port_dummy = parent_port
        .port_dummy
        .as_ref()
        .expect("compound port should point to nested external dummy");
    assert!(parent_port.inside_connections);
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    assert_eq!(port_dummy.graph_id, nested.id);
    let external = &nested.layerless_nodes[port_dummy.node];
    assert_eq!(external.external_port().unwrap().side, PortSide::South);
    assert_eq!(
        parent_port.border_offset,
        external.ports[PortId::new(0)].border_offset
    );
    assert_eq!(
        parent_port.border_offset,
        Some(nested.options.spacing.edge_edge / 2.0)
    );
    let origin = external
        .external_port()
        .unwrap()
        .origin_port
        .as_ref()
        .expect("external dummy should point back to parent port");
    assert_eq!(origin.graph_id, GraphId::ROOT);
    assert_eq!(origin.port.node, cluster_index);
    assert_eq!(origin.port.port, PortId::new(0));
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn source_ported_compound_does_not_duplicate_existing_parent_dummy() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![node("B"), cluster, child],
        vec![edge("B-A", "B", "A")],
    ))
    .unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let cluster = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    let external_dummies = nested
        .nodes_with_ids()
        .filter(|(_, node)| node.kind() == LNodeKind::ExternalPort && node.id == "external:cluster")
        .collect::<Vec<_>>();

    assert_eq!(external_dummies.len(), 1);
    let (dummy_node, external_dummy) = external_dummies[0];
    let dummy_port = external_dummy.ports.first().unwrap();
    assert_eq!(
        dummy_port.incoming_edges().len() + dummy_port.outgoing_edges().len(),
        1
    );
    assert_eq!(cluster.ports.len(), 1);
    assert_eq!(
        cluster.ports[PortId::new(0)]
            .port_dummy
            .as_ref()
            .unwrap()
            .node,
        dummy_node
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn source_ported_compound_metadata_links_parent_to_child_external_dummy() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![cluster, child],
        vec![edge("cluster-A", "cluster", "A")],
    ))
    .unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let cluster_index = lgraph.node_named("cluster");
    let cluster = &lgraph.layerless_nodes[cluster_index];
    let parent_port = cluster
        .ports
        .iter()
        .find(|port| port.port_dummy.is_some())
        .expect("parent-to-child edge should create a parent external port");
    assert_eq!(cluster.ports.len(), 1);
    assert_eq!(parent_port.id, "cluster:0:source");
    assert_eq!(parent_port.port_type, PortType::Output);
    let port_dummy = parent_port.port_dummy.as_ref().unwrap();
    assert!(parent_port.inside_connections);
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    assert_eq!(port_dummy.graph_id, nested.id);
    let external = &nested.layerless_nodes[port_dummy.node];
    let origin = external
        .external_port()
        .unwrap()
        .origin_port
        .as_ref()
        .expect("external dummy should point back to parent port");
    assert_eq!(origin.graph_id, GraphId::ROOT);
    assert_eq!(origin.port.node, cluster_index);
    assert_eq!(origin.port.port, PortId::new(0));
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn source_ported_compound_parent_boundary_segments_use_external_port_dummies() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![cluster, child],
        vec![edge("cluster-A", "cluster", "A")],
    ))
    .unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);
    let cluster = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    let nested = &lgraph[cluster.nested_graph.unwrap()];
    let segment = nested
        .edges
        .iter()
        .find(|edge| edge.id == "cluster-A")
        .unwrap();

    assert_eq!(
        nested.layerless_nodes[segment.source().node].kind(),
        LNodeKind::ExternalPort
    );
    assert_eq!(nested.layerless_nodes[segment.target().node].id, "A");
}

// Mutation: in `source_ported_cross_hierarchy_segments`, give the segment between the two paths
// the depth `common_depth + 1`.
#[test]
fn source_ported_compound_import_records_cross_hierarchy_segments() {
    let mut outer = node("outer");
    outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut inner = node("inner");
    inner.parent = Some("outer".to_string());
    inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("inner".to_string());

    let mut lgraph = import_graph(&graph(
        vec![outer, inner, child, node("B")],
        vec![edge("A-B", "A", "B")],
    ))
    .unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let outer = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "outer")
        .unwrap();
    let inner_graph = outer
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap()
        .layerless_nodes
        .iter()
        .find(|node| node.id == "inner")
        .unwrap()
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap();
    let outer_graph = &lgraph[outer.nested_graph.unwrap()];
    assert_eq!(
        inner_graph.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Output { depth: 2 }
    );
    assert_eq!(
        outer_graph.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Output { depth: 1 }
    );
    assert_eq!(
        lgraph.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Output { depth: 0 }
    );
}

// Mutation: in `hierarchy_port_key`, never use the shared `node:collector:type` key (`if
// false`).
#[test]
fn source_ported_compound_reuses_exported_external_port_when_hierarchy_edges_merge() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut first = edge("A-B", "A", "B");
    let mut first_label = ElkInputLabel::center("first", 12.0, 6.0);
    first_label.placement = EdgeLabelPlacement::Tail;
    first.labels = vec![first_label];
    let mut second = edge("A-C", "A", "C");
    let mut second_label = ElkInputLabel::center("second", 18.0, 6.0);
    second_label.placement = EdgeLabelPlacement::Tail;
    second.labels = vec![second_label];
    let mut input = graph(
        vec![cluster, child, node("B"), node("C")],
        vec![first, second],
    );
    input.options.merge_edges = true;
    input.options.merge_hierarchy_edges = true;

    let mut lgraph = import_graph(&input).unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap()
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap();
    assert_eq!(nested.edges.len(), 1);
    assert_eq!(nested.cross_hierarchy_edges.len(), 2);
    assert!(
        nested
            .cross_hierarchy_edges
            .iter()
            .all(|segment| segment.edge == EdgeId::new(0))
    );
    assert_eq!(nested.edges[EdgeId::new(0)].labels.len(), 2);
    assert_eq!(
        nested.edges[EdgeId::new(0)]
            .labels
            .iter()
            .filter_map(|label| label.original_label_edge.as_deref())
            .collect::<Vec<_>>(),
        vec!["A-B", "A-C"]
    );
    assert!(nested.graph_properties.end_labels);
}

// Mutation: in `hierarchy_port_key`, never use the shared `node:collector:type` key (`if
// false`).
#[test]
fn source_ported_compound_keeps_nested_hierarchy_merge_default_when_root_disables_merge() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut input = graph(
        vec![cluster, child, node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
    );
    input.options.merge_edges = true;
    input.options.merge_hierarchy_edges = false;

    let mut lgraph = import_graph(&input).unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap()
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap();
    assert_eq!(nested.edges.len(), 1);
    assert_eq!(
        nested
            .cross_hierarchy_edges
            .iter()
            .map(|segment| segment.edge)
            .collect::<Vec<_>>(),
        vec![EdgeId::new(0), EdgeId::new(0)]
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn source_ported_compound_parent_end_segments_are_not_reused() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut input = graph(
        vec![cluster, child],
        vec![
            edge("cluster-A-1", "cluster", "A"),
            edge("cluster-A-2", "cluster", "A"),
        ],
    );
    input.options.merge_edges = true;
    input.options.merge_hierarchy_edges = true;

    let mut lgraph = import_graph(&input).unwrap();
    preprocess_source_ported_compound_graph(&mut lgraph);

    let nested = lgraph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap()
        .nested_graph
        .map(|id| &lgraph[id])
        .unwrap();
    assert_eq!(nested.edges.len(), 2);
    assert_eq!(
        nested
            .cross_hierarchy_edges
            .iter()
            .map(|segment| segment.edge)
            .collect::<Vec<_>>(),
        vec![EdgeId::new(0), EdgeId::new(1)]
    );
    assert!(
        nested.edges.iter().all(
            |edge| nested.layerless_nodes[edge.source().node].kind() == LNodeKind::ExternalPort
        )
    );
}

#[test]
fn importer_reuses_collector_ports_when_edges_are_merged() {
    let mut input = graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
    );
    input.options.merge_edges = true;

    let lgraph = import_graph(&input).unwrap();

    let a = lgraph.node_named("A");
    assert_eq!(lgraph.layerless_nodes[a].ports.len(), 1);
    assert_eq!(
        lgraph.layerless_nodes[a].ports[PortId::new(0)].collector_type,
        Some(PortType::Output)
    );
    assert_eq!(
        lgraph.layerless_nodes[a].ports[PortId::new(0)].outgoing_edges(),
        vec![EdgeId::new(0), EdgeId::new(1)]
    );
    assert!(lgraph.graph_properties.hyperedges);
    assert!(lgraph.options.graph_has_hyperedges);
}

#[test]
fn importer_keeps_dedicated_ports_when_edge_merge_is_disabled() {
    let lgraph = import_graph(&graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
    ))
    .unwrap();

    let a = lgraph.node_named("A");
    assert_eq!(lgraph.layerless_nodes[a].ports.len(), 2);
    assert!(
        lgraph.layerless_nodes[a]
            .ports
            .iter()
            .all(|port| port.collector_type.is_none())
    );
    assert!(!lgraph.graph_properties.hyperedges);
}

// Mutation: name a made-up port after `ports.len()` instead of `ports.slot_count()`.
#[test]
fn a_made_up_port_gets_a_name_no_unlisted_port_has() {
    let mut lgraph = LGraph::new("root", LayeredOptions::default());
    lgraph.options.merge_edges = false;
    lgraph
        .layerless_nodes
        .push(LNode::new("A", 10.0, 10.0, None));
    let first = lgraph.add_port(
        NodeId::new(0),
        PortType::Output,
        PortSide::East,
        Default::default(),
    );
    lgraph.add_port(
        NodeId::new(0),
        PortType::Output,
        PortSide::East,
        Default::default(),
    );
    lgraph.layerless_nodes[NodeId::new(0)]
        .ports
        .unlist(&[first.unwrap().port]);

    let made_up = ensure_port_at_node(
        &mut lgraph,
        None,
        NodeId::new(0),
        "A",
        PortType::Output,
        &mut ImportPortIndex::new(),
    )
    .unwrap();

    let ports = &lgraph.layerless_nodes[NodeId::new(0)].ports;
    assert_eq!(ports.slot_count(), 3);
    let made_up_name = &ports[made_up.port].id;
    let same_name = (0..ports.slot_count())
        .filter(|index| ports[PortId::new(*index)].id == *made_up_name)
        .count();
    assert_eq!(same_name, 1);
}

#[test]
fn implicit_ports_have_directional_sides_before_preprocessing() {
    for (direction, output_side, input_side) in [
        (ElkDirection::Right, PortSide::East, PortSide::West),
        (ElkDirection::Left, PortSide::West, PortSide::East),
        (ElkDirection::Down, PortSide::South, PortSide::North),
        (ElkDirection::Up, PortSide::North, PortSide::South),
    ] {
        let mut input = graph(
            vec![node("A"), node("B")],
            vec![edge("first", "A", "B"), edge("second", "A", "B")],
        );
        input.options.direction = direction;
        let imported = import_graph(&input).unwrap();
        let source = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "A")
            .unwrap();
        let target = imported
            .layerless_nodes
            .iter()
            .find(|node| node.id == "B")
            .unwrap();
        assert_eq!(source.ports.len(), 2);
        assert_eq!(target.ports.len(), 2);
        for index in 0..2 {
            let port = PortId::new(index);
            assert_eq!(source.ports[port].side, output_side, "{direction:?}");
            assert_eq!(target.ports[port].side, input_side, "{direction:?}");
            assert_eq!(
                source.ports[port].outgoing_edges(),
                vec![EdgeId::new(index)]
            );
            assert_eq!(
                target.ports[port].incoming_edges(),
                vec![EdgeId::new(index)]
            );
        }
    }
}

#[test]
fn importer_keeps_dedicated_ports_when_node_port_constraints_are_side_fixed() {
    let mut a = node("A");
    a.port_constraints = Some(PortConstraints::FixedSide);
    let mut input = graph(
        vec![a, node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("A-C", "A", "C")],
    );
    input.options.merge_edges = true;

    let lgraph = import_graph(&input).unwrap();

    let a = lgraph.node_named("A");
    assert_eq!(lgraph.layerless_nodes[a].ports.len(), 2);
    assert!(
        lgraph.layerless_nodes[a]
            .ports
            .iter()
            .all(|port| port.collector_type.is_none())
    );
}

#[test]
fn import_rejects_invalid_parent_and_endpoints() {
    let mut child = node("A");
    child.parent = Some("missing".to_string());
    assert!(matches!(
        import_graph(&graph(vec![child], vec![])),
        Err(ImportError::MissingParent { .. })
    ));

    assert!(matches!(
        import_graph(&graph(vec![node("A")], vec![edge("A-B", "A", "B")])),
        Err(ImportError::MissingEndpoint { .. })
    ));
}

#[test]
fn scoped_segment_import_rejects_an_unmaterialized_owner_scope() {
    let group = node("group");
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let mut input = graph(vec![group, child, node("outer")], vec![]);
    input.options.hierarchy_handling = HierarchyHandling::SeparateChildren;
    let segment = ElkInputEdgeSegment {
        edge: edge("child-outer", "child", "outer"),
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "child".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "outer".to_string(),
        },
        segment: CompoundEdgeSegment::Output { depth: 0 },
        edge_order: 0,
    };

    let error = import_graph_with_segments(&input, &[segment])
        .expect_err("a flat import must reject a segment owned by a nested scope");

    assert_eq!(
        error,
        ImportError::UnavailableSegmentScope {
            edge_id: "child-outer".to_string(),
            graph_parent: "group".to_string(),
        }
    );
}

#[test]
fn scoped_segment_import_rejects_a_scope_below_an_unmaterialized_ancestor() {
    let mut outer = node("outer-group");
    outer.hierarchy_handling = Some(HierarchyHandling::SeparateChildren);
    let mut inner = node("inner-group");
    inner.parent = Some("outer-group".to_string());
    inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("child");
    child.parent = Some("inner-group".to_string());
    let mut input = graph(vec![outer, inner, child, node("root-node")], vec![]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let segment = ElkInputEdgeSegment {
        edge: edge("child-root", "child", "root-node"),
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "child".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "root-node".to_string(),
        },
        segment: CompoundEdgeSegment::Output { depth: 0 },
        edge_order: 0,
    };

    let error = import_graph_with_segments(&input, &[segment])
        .expect_err("a locally includable scope below a SeparateChildren ancestor is unreachable");

    assert_eq!(
        error,
        ImportError::UnavailableSegmentScope {
            edge_id: "child-root".to_string(),
            graph_parent: "outer-group".to_string(),
        }
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, give the segment between the two paths
// the depth `common_depth + 1`.
#[test]
fn scoped_segment_import_accepts_a_materialized_owner_scope() {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let mut input = graph(vec![group, child, node("outer")], vec![]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let mut segment_edge = edge("child-outer", "child", "outer");
    segment_edge.model_order = Some(7);
    segment_edge.labels = vec![ElkInputLabel::center("identity", 40.0, 12.0)];
    let segment = ElkInputEdgeSegment {
        edge: segment_edge,
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "child".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "outer".to_string(),
        },
        segment: CompoundEdgeSegment::Output { depth: 2 },
        edge_order: 0,
    };

    let imported = import_graph_with_segments(&input, &[segment])
        .expect("the nested owner scope is materialized");

    assert_eq!(imported.cross_hierarchy_edges.len(), 1);
    let nested = imported
        .layerless_nodes
        .iter()
        .find(|node| node.id == "group")
        .and_then(|node| node.nested_graph.map(|id| &imported[id]))
        .expect("group scope should be materialized");
    assert_eq!(nested.cross_hierarchy_edges.len(), 1);
    assert_eq!(
        imported.cross_hierarchy_edges[0].original_model_order,
        Some(7)
    );
    assert_eq!(
        nested.cross_hierarchy_edges[0].original_model_order,
        Some(7)
    );
    assert_eq!(
        imported.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Output { depth: 2 }
    );
    assert_eq!(
        nested.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Output { depth: 3 }
    );
    let labels = imported
        .edges
        .iter()
        .chain(nested.edges.iter())
        .flat_map(|edge| edge.labels.iter())
        .collect::<Vec<_>>();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].text, "identity");
    assert_eq!(
        labels[0].original_label_edge.as_deref(),
        Some("child-outer")
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn scoped_input_segment_rebases_every_materialized_hierarchy_level() {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let mut input = graph(vec![group, child, node("outer")], vec![]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let segment = ElkInputEdgeSegment {
        edge: edge("outer-child", "outer", "child"),
        source: ElkInputEdgeSegmentEndpoint::Node {
            id: "outer".to_string(),
        },
        target: ElkInputEdgeSegmentEndpoint::Node {
            id: "child".to_string(),
        },
        segment: CompoundEdgeSegment::Input { depth: 4 },
        edge_order: 0,
    };

    let imported = import_graph_with_segments(&input, &[segment])
        .expect("the nested owner scope is materialized");
    let nested = imported
        .layerless_nodes
        .iter()
        .find(|node| node.id == "group")
        .and_then(|node| node.nested_graph.map(|id| &imported[id]))
        .expect("group scope should be materialized");

    assert_eq!(
        imported.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Input { depth: 4 }
    );
    assert_eq!(
        nested.cross_hierarchy_edges[0].segment,
        CompoundEdgeSegment::Input { depth: 5 }
    );
}

#[test]
fn import_preserves_model_order_strategy_without_enabling_wrapping() {
    let mut input = graph(vec![node("A"), node("B")], vec![edge("A-B", "A", "B")]);
    input.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;

    let lgraph = import_graph(&input).unwrap();

    assert_eq!(
        lgraph.options.consider_model_order_strategy,
        OrderingStrategy::NodesAndEdges
    );
    assert!(!lgraph.options.graph_has_hyperedges);
}

#[test]
fn import_keeps_elk_zero_seed_and_the_pipeline_rejects_it() {
    let mut input = graph(vec![node("A")], vec![]);
    input.options.random_seed = 0;
    let mut imported = import_graph(&input).expect("zero is a valid upstream input value");

    assert!(matches!(
        crate::pipeline::execute_ported_processors(&mut imported),
        Err(crate::pipeline::PipelineError::RandomSeed(crate::random::RandomSeedError::ZeroSeed {
            graph_id
        })) if graph_id == "root"
    ));
    assert_eq!(imported.options.random_seed, 0);
}

// --- Ports on nodes without children. ---

fn input_port(id: &str, side: PortSide) -> ElkInputPort {
    ElkInputPort {
        id: id.to_string(),
        x: 0.0,
        y: 0.0,
        width: 6.0,
        height: 4.0,
        side,
        index: None,
        border_offset: None,
        allow_non_flow_ports_to_switch_sides: false,
        net_flow: 0,
        child_edges: 0,
        connected_to_external_nodes: false,
        inside_connections: false,
        has_edges: false,
        labels: Vec::new(),
    }
}

/// A node of 80 by 40 with the given ports and port constraints.
fn ported(
    id: &str,
    constraints: Option<PortConstraints>,
    ports: Vec<ElkInputPort>,
) -> ElkInputNode {
    let mut node = node(id);
    node.port_constraints = constraints;
    node.ports = ports;
    node
}

fn flat_graph(
    direction: ElkDirection,
    nodes: Vec<ElkInputNode>,
    edges: Vec<ElkInputEdge>,
) -> ElkInputGraph {
    let mut input = graph(nodes, edges);
    input.options.direction = direction;
    input.options.hierarchy_handling = HierarchyHandling::SeparateChildren;
    input
}

fn layered_node<'a>(graph: &'a LGraph, id: &str) -> &'a LNode {
    graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == id)
        .unwrap()
}

// Mutation: skip the `non_free_ports` property in `transform_node`, or compute the anchor
// as the centre for a fixed-side port.
#[test]
fn a_fixed_side_port_keeps_its_size_position_side_and_side_anchor() {
    let mut east = input_port("p", PortSide::East);
    east.x = 80.0;
    east.y = 10.0;
    let west = input_port("q", PortSide::West);
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported(
            "A",
            Some(PortConstraints::FixedSide),
            vec![east, west],
        )],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    let node = layered_node(&lgraph, "A");
    assert_eq!(node.port_constraints, PortConstraints::FixedSide);
    assert!(lgraph.graph_properties.non_free_ports);
    assert!(!lgraph.graph_properties.north_south_ports);
    let ids: Vec<_> = node.ports.iter().map(|port| port.id.as_str()).collect();
    assert_eq!(ids, ["p", "q"]);
    let (p, q) = (&node.ports[PortId::new(0)], &node.ports[PortId::new(1)]);
    assert_eq!(p.side, PortSide::East);
    assert_eq!((p.size.width, p.size.height), (6.0, 4.0));
    assert_eq!((p.position.x, p.position.y), (80.0, 10.0));
    assert_eq!((p.anchor.x, p.anchor.y), (6.0, 2.0));
    assert_eq!((q.anchor.x, q.anchor.y), (0.0, 2.0));
    assert_eq!((p.input_index, q.input_index), (Some(0), Some(1)));
}

// Mutation: give a port that is not side fixed the anchor of its side.
#[test]
fn a_free_port_keeps_its_side_but_has_the_centre_as_anchor() {
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported("A", None, vec![input_port("p", PortSide::East)])],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    let port = &layered_node(&lgraph, "A").ports[PortId::new(0)];
    assert_eq!(port.side, PortSide::East);
    assert_eq!((port.anchor.x, port.anchor.y), (3.0, 2.0));
    assert!(!lgraph.graph_properties.non_free_ports);
}

// Mutation: keep `UNDEFINED` on the layered node, or let it set `non_free_ports`.
#[test]
fn undefined_port_constraints_become_free() {
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported(
            "A",
            Some(PortConstraints::Undefined),
            vec![input_port("p", PortSide::West)],
        )],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    assert_eq!(
        layered_node(&lgraph, "A").port_constraints,
        PortConstraints::Free
    );
    assert!(!lgraph.graph_properties.non_free_ports);
}

// Mutation: set `non_free_ports` only for a node that has ports.
#[test]
fn a_node_without_ports_still_marks_the_graph_as_not_free() {
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported("A", Some(PortConstraints::FixedOrder), vec![])],
        vec![],
    );
    assert!(
        import_graph(&input)
            .unwrap()
            .graph_properties
            .non_free_ports
    );
}

// Mutation: drop the call to `calc_port_side`, or the border offset next to it.
#[test]
fn a_fixed_side_port_without_a_side_takes_it_from_its_position() {
    // Node of 80 by 40, port of 6 by 4. (x, y, expected side, expected border offset)
    let cases = [
        (-8.0, 10.0, PortSide::West, Some(2.0)),
        (84.0, 10.0, PortSide::East, Some(4.0)),
        // At the corner the offset stays unset, so the port sits on the border.
        (0.0, 0.0, PortSide::West, None),
    ];
    for (x, y, side, offset) in cases {
        let mut port = input_port("p", PortSide::Undefined);
        port.x = x;
        port.y = y;
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", Some(PortConstraints::FixedSide), vec![port])],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        let port = &layered_node(&lgraph, "A").ports[PortId::new(0)];
        assert_eq!((port.side, port.border_offset), (side, offset), "x = {x}");
    }
}

// Mutation: overwrite a border offset the input sets with the computed one.
#[test]
fn a_border_offset_from_the_input_wins() {
    let mut port = input_port("p", PortSide::Undefined);
    port.x = -8.0;
    port.y = 10.0;
    port.border_offset = Some(7.0);
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported("A", Some(PortConstraints::FixedSide), vec![port])],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    assert_eq!(
        layered_node(&lgraph, "A").ports[PortId::new(0)].border_offset,
        Some(7.0)
    );
}

// Mutation: remember the ratio for FIXED_POS too, or divide by the width on the east side.
#[test]
fn only_fixed_ratio_remembers_the_ratio_of_a_port() {
    for (constraints, ratio) in [
        (PortConstraints::FixedRatio, 0.25),
        (PortConstraints::FixedPos, 0.0),
        (PortConstraints::FixedOrder, 0.0),
    ] {
        let mut port = input_port("p", PortSide::East);
        port.x = 80.0;
        port.y = 10.0;
        let input = flat_graph(
            ElkDirection::Right,
            vec![ported("A", Some(constraints), vec![port])],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        assert_eq!(
            layered_node(&lgraph, "A").ports[PortId::new(0)].ratio_or_position,
            ratio
        );
    }
}

// Mutation: ignore the `elk.port.index` of the input port.
#[test]
fn the_port_index_reaches_the_layered_port() {
    let mut indexed = input_port("p", PortSide::East);
    indexed.index = Some(3);
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported(
            "A",
            Some(PortConstraints::FixedOrder),
            vec![indexed, input_port("q", PortSide::East)],
        )],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    let ports = &layered_node(&lgraph, "A").ports;
    assert_eq!(
        (
            ports[PortId::new(0)].port_index,
            ports[PortId::new(1)].port_index
        ),
        (Some(3), None)
    );
}

// Mutation: attach the edge to a new port instead of the named one.
#[test]
fn an_edge_attaches_to_its_named_ports() {
    let mut named = edge("e", "A", "B");
    named.source_port = Some("p".to_string());
    named.target_port = Some("q".to_string());
    let input = flat_graph(
        ElkDirection::Right,
        vec![
            ported(
                "A",
                Some(PortConstraints::FixedSide),
                vec![
                    input_port("x", PortSide::East),
                    input_port("p", PortSide::East),
                ],
            ),
            ported(
                "B",
                Some(PortConstraints::FixedSide),
                vec![input_port("q", PortSide::West)],
            ),
        ],
        vec![named],
    );
    let lgraph = import_graph(&input).unwrap();
    // No port was added, and the second port of A holds the edge.
    assert_eq!(layered_node(&lgraph, "A").ports.len(), 2);
    assert_eq!(layered_node(&lgraph, "B").ports.len(), 1);
    assert!(
        layered_node(&lgraph, "A").ports[PortId::new(0)]
            .outgoing_edges()
            .is_empty()
    );
    assert_eq!(
        layered_node(&lgraph, "A").ports[PortId::new(1)].outgoing_edges(),
        vec![EdgeId::new(0)]
    );
    assert_eq!(
        layered_node(&lgraph, "B").ports[PortId::new(0)].incoming_edges(),
        vec![EdgeId::new(0)]
    );
}

// Mutation: count the incoming and outgoing edges of a port together, as one number.
#[test]
fn a_port_with_two_edges_in_one_direction_makes_a_hyperedge_but_one_each_does_not() {
    let from_port = |id: &str, target: &str| {
        let mut e = edge(id, "A", target);
        e.source_port = Some("p".to_string());
        e
    };
    let into_port = |id: &str, source: &str| {
        let mut e = edge(id, source, "A");
        e.target_port = Some("p".to_string());
        e
    };
    let build = |edges: Vec<ElkInputEdge>| {
        let input = flat_graph(
            ElkDirection::Right,
            vec![
                ported(
                    "A",
                    Some(PortConstraints::FixedSide),
                    vec![input_port("p", PortSide::East)],
                ),
                node("B"),
            ],
            edges,
        );
        import_graph(&input).unwrap().graph_properties.hyperedges
    };
    assert!(!build(vec![from_port("e1", "B")]));
    assert!(!build(vec![from_port("e1", "B"), into_port("e2", "B")]));
    assert!(build(vec![from_port("e1", "B"), from_port("e2", "B")]));
    assert!(build(vec![into_port("e1", "B"), into_port("e2", "B")]));
}

// Mutation: look up the crossing edges of a port by its id alone (`crossing_at` and the
// writes in `transform_cross_hierarchy_edge` ignore `named`). The implicit port `G:2` then
// reads the edge into the named port `G:2` and the graph becomes one with hyperedges.
#[test]
fn a_made_up_port_does_not_share_crossing_edges_with_a_named_port_of_the_same_id() {
    let build = |flat_target_port: Option<&str>| {
        let mut child = node("c");
        child.parent = Some("G".to_string());
        let mut into_port = edge("e1", "c", "G");
        into_port.target_port = Some("G:2".to_string());
        // The root owns the edge, so it is imported before the flat edge, as the bug needs.
        into_port.container = Some("root".to_string());
        let mut flat = edge("e2", "s", "G");
        flat.target_port = flat_target_port.map(str::to_string);
        let mut input = flat_graph(
            ElkDirection::Right,
            vec![
                node("s"),
                ported(
                    "G",
                    None,
                    vec![
                        input_port("G:1", PortSide::West),
                        input_port("G:2", PortSide::West),
                    ],
                ),
                child,
            ],
            vec![into_port, flat],
        );
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        import_graph(&input).unwrap().graph_properties.hyperedges
    };
    // G has two ports, so the edge from `s` gets a made-up port that is also called `G:2`.
    assert!(!build(None));
    // Control: two edges into the named port do make a hyperedge.
    assert!(build(Some("G:2")));
}

// Mutation: reject a port that has the id of a node, or let two ports share an id.
#[test]
fn a_port_id_may_repeat_a_node_id_but_not_a_port_id() {
    let two_with = |first: &str, second: &str| {
        flat_graph(
            ElkDirection::Right,
            vec![
                ported("A", None, vec![input_port(first, PortSide::East)]),
                ported("B", None, vec![input_port(second, PortSide::West)]),
            ],
            vec![],
        )
    };
    assert!(matches!(
        import_graph(&two_with("p", "p")),
        Err(ImportError::DuplicatePort { id }) if id == "p"
    ));
    // elkjs keeps node and port ids apart.
    assert!(import_graph(&two_with("B", "A")).is_ok());
    assert!(import_graph(&two_with("p", "q")).is_ok());
}

// Mutation: accept a port of another node.
#[test]
fn an_edge_may_only_name_a_port_of_its_own_node() {
    let build = |source_port: &str| {
        let mut e = edge("e", "A", "B");
        e.source_port = Some(source_port.to_string());
        flat_graph(
            ElkDirection::Right,
            vec![
                ported("A", None, vec![input_port("p", PortSide::East)]),
                ported("B", None, vec![input_port("q", PortSide::West)]),
            ],
            vec![e],
        )
    };
    assert!(import_graph(&build("p")).is_ok());
    assert!(matches!(
        import_graph(&build("q")),
        Err(ImportError::MissingPort { port_id, node_id, .. }) if port_id == "q" && node_id == "A"
    ));
    assert!(matches!(
        import_graph(&build("nowhere")),
        Err(ImportError::MissingPort { .. })
    ));

    // An edge the flat import leaves out is checked too: both its ends sit inside `G`.
    let mut inside = edge("e", "X", "X");
    inside.source_port = Some("q".to_string());
    let mut x = ported("X", None, vec![input_port("p", PortSide::East)]);
    x.parent = Some("G".to_string());
    let input = flat_graph(
        ElkDirection::Right,
        vec![
            node("G"),
            x,
            ported("B", None, vec![input_port("q", PortSide::West)]),
        ],
        vec![inside],
    );
    assert!(matches!(
        import_graph(&input),
        Err(ImportError::MissingPort { port_id, .. }) if port_id == "q"
    ));
}

// Mutation: drop the direction from the test, so that a port across the flow is one that is
// north or south in every direction.
#[test]
fn a_port_across_the_flow_of_a_node_without_children_is_imported_and_marks_the_graph() {
    // (direction, side, across the flow)
    let cases = [
        (ElkDirection::Right, PortSide::North, true),
        (ElkDirection::Left, PortSide::South, true),
        (ElkDirection::Right, PortSide::East, false),
        (ElkDirection::Down, PortSide::East, true),
        (ElkDirection::Up, PortSide::West, true),
        (ElkDirection::Down, PortSide::North, false),
        (ElkDirection::Up, PortSide::South, false),
    ];
    for (direction, side, across_flow) in cases {
        let input = flat_graph(
            direction,
            vec![ported(
                "A",
                Some(PortConstraints::FixedSide),
                vec![input_port("p", side)],
            )],
            vec![],
        );
        let lgraph = import_graph(&input).unwrap();
        assert_eq!(
            lgraph.graph_properties.north_south_ports, across_flow,
            "{direction:?} {side:?}"
        );
    }
}

// Mutation: refuse a port across the flow although every node has free ports.
#[test]
fn a_port_across_the_flow_is_fine_while_all_ports_are_free() {
    let input = flat_graph(
        ElkDirection::Right,
        vec![ported("A", None, vec![input_port("p", PortSide::North)])],
        vec![],
    );
    let lgraph = import_graph(&input).unwrap();
    // The graph still has the property. `PortSideProcessor` moves the port later.
    assert!(lgraph.graph_properties.north_south_ports);
    // A fixed node anywhere else in the graph makes the graph need the north and south
    // port steps, which a node without children does not stop.
    let mixed = flat_graph(
        ElkDirection::Right,
        vec![
            ported("A", None, vec![input_port("p", PortSide::North)]),
            ported("B", Some(PortConstraints::FixedOrder), vec![]),
        ],
        vec![],
    );
    assert!(import_graph(&mixed).is_ok());
}

// Mutation: hand out the model order of ports even when the graph does not ask for it.
#[test]
fn ports_get_a_model_order_when_the_graph_asks_for_one() {
    let ports = || {
        vec![
            input_port("p", PortSide::East),
            input_port("q", PortSide::East),
        ]
    };
    let mut asking = flat_graph(
        ElkDirection::Right,
        vec![ported("A", None, ports())],
        vec![],
    );
    asking.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
    let lgraph = import_graph(&asking).unwrap();
    let orders: Vec<_> = layered_node(&lgraph, "A")
        .ports
        .iter()
        .map(|p| p.model_order)
        .collect();
    assert_eq!(orders, [Some(0), Some(1)]);

    let mut silent = flat_graph(
        ElkDirection::Right,
        vec![ported("A", None, ports())],
        vec![],
    );
    silent.options.consider_model_order_strategy = OrderingStrategy::None;
    let lgraph = import_graph(&silent).unwrap();
    let orders: Vec<_> = layered_node(&lgraph, "A")
        .ports
        .iter()
        .map(|p| p.model_order)
        .collect();
    assert_eq!(orders, [None, None]);
}

// --- Labels of the ports of a graph's node: `transformExternalPort`. ---

mod external_port_labels {
    use super::*;
    use crate::graph::{DummyLabelSpacings, LSize};
    use crate::options::PortLabelPlacement;

    fn label(text: &str, width: f64, height: f64) -> ElkInputLabel {
        ElkInputLabel {
            source_index: None,
            text: text.to_string(),
            width,
            height,
            placement: crate::graph::EdgeLabelPlacement::Center,
            inline: false,
            node_label_placement: None,
            x: 0.0,
            y: 0.0,
        }
    }

    /// A port of 6 by 4 with the given labels. `child_edges` of 1 makes it external by itself.
    fn port(
        id: &str,
        side: PortSide,
        child_edges: usize,
        labels: Vec<ElkInputLabel>,
    ) -> ElkInputPort {
        ElkInputPort {
            child_edges,
            labels,
            ..input_port(id, side)
        }
    }

    /// Imports a graph of one node, run with the given ports as the ports of its own node.
    fn import(
        ports: &[ElkInputPort],
        placement: PortLabelPlacement,
        spacings: DummyLabelSpacings,
    ) -> GraphArena {
        let mut input = graph(vec![node("A")], vec![]);
        input.options.port_label_placement = placement;
        input.options.port_constraints = PortConstraints::FixedSide;
        let scope = ScopePorts {
            ports,
            node_size: LSize {
                width: 80.0,
                height: 40.0,
            },
            label_spacings: spacings,
        };
        import_scope(&input, &[], ElkPadding::uniform(0.0), scope)
            .unwrap()
            .graphs
    }

    fn defaults() -> DummyLabelSpacings {
        DummyLabelSpacings::default()
    }

    fn dummy_labels(graph: &GraphArena, port: &str) -> Vec<(f64, f64)> {
        let dummy = graph
            .root()
            .layerless_nodes
            .iter()
            .find(|node| node.id == format!("root:external:{port}"))
            .expect("the port has a dummy");
        dummy.ports[PortId::new(0)]
            .labels
            .iter()
            .map(|label| (label.size.width, label.size.height))
            .collect()
    }

    // Mutation: leave out the `inside_port_labels &&` branch of the external check, or let the
    // OUTSIDE placement count as well.
    #[test]
    fn a_label_makes_a_port_external_only_under_inside_port_labels() {
        let ports = [port("p", PortSide::East, 0, vec![label("req", 20.0, 10.0)])];
        let inside = import(&ports, PortLabelPlacement::INSIDE, defaults());
        assert!(inside.root().graph_properties.external_ports);
        assert!(!inside.root().graph_properties.hyperedges);
        let outside = import(&ports, PortLabelPlacement::OUTSIDE, defaults());
        assert!(!outside.root().graph_properties.external_ports);
        assert!(
            outside
                .layerless_nodes
                .iter()
                .all(|n| n.id != "root:external:p")
        );
    }

    // Mutation: look for a label with text in the external check, which skips this port.
    #[test]
    fn a_label_without_text_still_makes_a_port_external_but_gets_no_dummy_label() {
        let ports = [port("p", PortSide::East, 0, vec![label("", 20.0, 10.0)])];
        let graph = import(&ports, PortLabelPlacement::INSIDE, defaults());
        assert!(graph.root().graph_properties.external_ports);
        assert_eq!(dummy_labels(&graph, "p"), vec![]);
    }

    // Mutation: make a port without labels external under INSIDE port labels.
    #[test]
    fn inside_port_labels_alone_make_no_port_external() {
        let ports = [port("p", PortSide::East, 0, vec![])];
        let graph = import(&ports, PortLabelPlacement::INSIDE, defaults());
        assert!(!graph.root().graph_properties.external_ports);
    }

    // Mutation: keep the full size of the label under OUTSIDE port labels, or reduce the
    // height of an east label.
    #[test]
    fn outside_labels_lose_the_size_along_the_axis_of_their_side() {
        let ports = [
            port("e", PortSide::East, 1, vec![label("req", 20.0, 10.0)]),
            port("w", PortSide::West, 1, vec![label("req", 20.0, 10.0)]),
            port("n", PortSide::North, 1, vec![label("req", 20.0, 10.0)]),
            port("s", PortSide::South, 1, vec![label("req", 20.0, 10.0)]),
        ];
        let graph = import(&ports, PortLabelPlacement::OUTSIDE, defaults());
        assert_eq!(dummy_labels(&graph, "e"), vec![(0.0, 10.0)]);
        assert_eq!(dummy_labels(&graph, "w"), vec![(0.0, 10.0)]);
        assert_eq!(dummy_labels(&graph, "n"), vec![(20.0, 0.0)]);
        assert_eq!(dummy_labels(&graph, "s"), vec![(20.0, 0.0)]);
    }

    // Mutation: reduce the size of the labels under INSIDE port labels too.
    #[test]
    fn inside_labels_keep_their_size() {
        let ports = [port("e", PortSide::East, 1, vec![label("req", 20.0, 10.0)])];
        let graph = import(&ports, PortLabelPlacement::INSIDE, defaults());
        assert_eq!(dummy_labels(&graph, "e"), vec![(20.0, 10.0)]);
    }

    // Mutation: keep the label size of a label with a position under fixed placement, or take
    // the inside part with the border offset of the port.
    #[test]
    fn fixed_labels_keep_the_part_that_reaches_into_the_node() {
        let mut east = label("req", 20.0, 10.0);
        // The label starts 12 left of the port: 12 of it lie in the node.
        east.x = -12.0;
        let mut north = label("req", 20.0, 10.0);
        // The label ends 7 below the top edge of the port's bottom (4): 10 + 1 - 4 = 7.
        north.y = 1.0;
        let ports = [
            port("e", PortSide::East, 1, vec![east]),
            port("n", PortSide::North, 1, vec![north]),
        ];
        let graph = import(&ports, PortLabelPlacement::FIXED, defaults());
        assert_eq!(dummy_labels(&graph, "e"), vec![(12.0, 10.0)]);
        assert_eq!(dummy_labels(&graph, "n"), vec![(20.0, 7.0)]);
    }

    // Mutation: give the dummy the label's size rather than reducing it under non-fixed OUTSIDE
    // placement, which would read the position of a label that has none.
    #[test]
    fn non_fixed_outside_labels_ignore_their_position() {
        let mut moved = label("req", 20.0, 10.0);
        moved.x = -12.0;
        let ports = [port("e", PortSide::East, 1, vec![moved])];
        let graph = import(&ports, PortLabelPlacement::OUTSIDE, defaults());
        assert_eq!(dummy_labels(&graph, "e"), vec![(0.0, 10.0)]);
    }

    // Mutation: copy the position of the label to the dummy's label only for fixed placement.
    #[test]
    fn a_dummy_label_keeps_the_position_of_its_label() {
        let mut moved = label("req", 20.0, 10.0);
        moved.x = 3.0;
        moved.y = 5.0;
        let ports = [port("e", PortSide::East, 1, vec![moved])];
        let graph = import(&ports, PortLabelPlacement::INSIDE, defaults());
        let dummy = graph
            .root()
            .layerless_nodes
            .iter()
            .find(|node| node.id == "root:external:e")
            .unwrap();
        let at = dummy.ports[PortId::new(0)].labels[0].position;
        assert_eq!((at.x, at.y), (3.0, 5.0));
    }

    // Mutation: leave the label spacings of the dummy at their defaults.
    #[test]
    fn the_dummy_carries_the_label_spacings_of_the_parent() {
        let ports = [port("e", PortSide::East, 1, vec![])];
        let spacings = DummyLabelSpacings {
            label_port_horizontal: 7.0,
            label_port_vertical: 8.0,
            label_label: 9.0,
        };
        let graph = import(&ports, PortLabelPlacement::OUTSIDE, spacings);
        let dummy = graph
            .root()
            .layerless_nodes
            .iter()
            .find(|node| node.id == "root:external:e")
            .unwrap();
        assert_eq!(dummy.external_port().unwrap().label_spacings, spacings);
    }

    // Mutation: copy only the first label of the port.
    #[test]
    fn every_label_with_text_reaches_the_dummy_in_order() {
        let ports = [port(
            "e",
            PortSide::East,
            1,
            vec![
                label("a", 20.0, 10.0),
                label("", 5.0, 5.0),
                label("b", 30.0, 6.0),
            ],
        )];
        let graph = import(&ports, PortLabelPlacement::INSIDE, defaults());
        assert_eq!(dummy_labels(&graph, "e"), vec![(20.0, 10.0), (30.0, 6.0)]);
    }
}

// --- What the cells read from the ports of a node with children. ---

mod cell_ports {
    use super::*;
    use crate::options::PortLabelPlacement;

    fn cell_ports_of(ports: Vec<ElkInputPort>) -> Vec<crate::common::cells::CellPort> {
        let mut group = node("g");
        group.nested_options = Some(Box::default());
        group.port_constraints = Some(PortConstraints::FixedSide);
        group.port_label_placement = PortLabelPlacement::OUTSIDE;
        group.ports = ports;
        cell_node(&group).ports.ports
    }

    // Mutation: leave `has_edges` false in `cell_ports`. ELK asks the port adapter, which shows
    // every edge of the port: a label may go next to a port only when no edge ends there.
    #[test]
    fn a_cell_port_has_edges_when_any_edge_ends_there() {
        let mut with_edge = input_port("a", PortSide::West);
        with_edge.has_edges = true;
        let ports = cell_ports_of(vec![with_edge, input_port("b", PortSide::West)]);
        assert!(ports[0].has_edges);
        assert!(!ports[1].has_edges);
    }

    // Mutation: leave `has_compound_connections` false in `cell_ports`. Inside labels of a node
    // with children go next to a port only when no edge to a descendant ends there.
    #[test]
    fn a_cell_port_has_compound_connections_when_a_descendant_reaches_it() {
        let mut reached = input_port("a", PortSide::West);
        reached.inside_connections = true;
        let ports = cell_ports_of(vec![reached, input_port("b", PortSide::West)]);
        assert!(ports[0].has_compound_connections);
        assert!(!ports[1].has_compound_connections);
    }

    // Mutation: give the port a ratio from its position. An `ElkPort` has none: only the layered
    // port gets one in `initializePort`, so the minimum size does not depend on ratios.
    #[test]
    fn a_cell_port_has_the_ratio_zero() {
        let mut moved = input_port("a", PortSide::West);
        moved.y = 20.0;
        let ports = cell_ports_of(vec![moved]);
        assert_eq!(ports[0].ratio_or_position, 0.0);
    }
}

// --- The compound flag and the labels of a port. ---

mod compound_flag_and_port_labels {
    use super::*;

    // Mutation: derive the flag from `nested_options` again. A leaf that carries options is no
    // compound node, and a compound node without options is one.
    #[test]
    fn the_compound_flag_follows_the_field_not_the_options() {
        let mut leaf_with_options = node("leaf");
        leaf_with_options.nested_options = Some(Box::default());
        let mut compound_without_options = node("compound");
        compound_without_options.compound_node = true;
        let imported = import_graph(&graph(
            vec![leaf_with_options, compound_without_options],
            vec![],
        ))
        .unwrap();
        assert!(!layered_node(&imported, "leaf").compound_node);
        assert!(layered_node(&imported, "compound").compound_node);
    }

    // Mutation: keep the labels without text in `port_labels_to_lgraph`, or drop the position.
    #[test]
    fn a_port_keeps_its_labels_with_text_and_the_position_they_have() {
        let mut port = input_port("p", PortSide::East);
        let mut at = ElkInputLabel::center("a", 10.0, 5.0);
        at.x = 3.0;
        at.y = -4.0;
        port.labels = vec![ElkInputLabel::center("", 1.0, 1.0), at];
        let labels: Vec<_> = port_labels_to_lgraph(&port).collect();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "a");
        assert_eq!((labels[0].position.x, labels[0].position.y), (3.0, -4.0));
    }
}
