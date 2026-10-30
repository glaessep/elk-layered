use super::*;
use crate::graph::{LNode, LPoint};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::{HierarchyHandling, LayeredOptions, NodeLabelPlacement};

// Mutation: in `CompoundEndpointPlan::new`, plan an endpoint for every node, not only for a
// node that holds a nested graph.
#[test]
fn the_endpoint_plan_lists_only_the_endpoints_at_nodes_that_hold_a_graph() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let holder = graph.push_node_with(|_| LNode::new("holder", 10.0, 10.0, None));
    let plain = graph.push_node_with(|_| LNode::new("plain", 10.0, 10.0, None));
    graph.layerless_nodes[holder].nested_graph = Some(GraphId::new(1));
    let mut edge_between = |node, segment| {
        let port = graph
            .add_port(node, PortType::Output, PortSide::East, LPoint::default())
            .unwrap();
        let mut edge = LayeredEdge::new("e", port, port);
        edge.compound_segment = Some(segment);
        graph.add_edge(edge).unwrap()
    };
    let segment = CompoundEdgeSegment::Output { depth: 0 };
    let at_holder = edge_between(holder, segment);
    edge_between(plain, segment);

    let plan = CompoundEndpointPlan::new(&graph, &mut NoopCompoundPreprocessObserver);

    assert_eq!(plan.for_node(holder).len(), 1);
    assert_eq!(plan.for_node(holder)[0].edge, at_holder);
    assert!(plan.for_node(plain).is_empty());
}

/// Steps of made-up graphs, one for each name, for tests that only look at the shape of the
/// segments. Two paths that share a name share the graph.
fn steps(names: &[&str]) -> Vec<HierarchyStep> {
    names
        .iter()
        .map(|name| HierarchyStep {
            node_id: name.to_string(),
            graph: GraphId::new(
                1 + name.bytes().fold(7usize, |hash, byte| {
                    (hash * 131 + usize::from(byte)) % 1_000_003
                }),
            ),
        })
        .collect()
}

// Mutation: compare only the graph in `common_graph_depth`. Two nodes without a graph both
// fall back to the root, so the paths would share a step they do not share.
#[test]
fn paths_through_different_nodes_that_share_the_root_fallback_have_no_common_depth() {
    let step = |node: &str| HierarchyStep {
        node_id: node.to_string(),
        graph: GraphId::ROOT,
    };
    assert_eq!(common_graph_depth(&[step("a")], &[step("b")]), 0);
    assert_eq!(common_graph_depth(&[step("a")], &[step("a")]), 1);
}

/// Steps of the node called `name` in `graphs`, with the graph inside it.
fn real_step(graphs: &GraphArena, name: &str) -> HierarchyStep {
    let graph = graphs
        .iter()
        .flat_map(|graph| &graph.layerless_nodes)
        .find(|node| node.id == name)
        .and_then(|node| node.nested_graph)
        .expect("the node has a graph inside");
    HierarchyStep {
        node_id: name.to_string(),
        graph,
    }
}

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
        inside_self_loops_yo: false,
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

fn compound_fanout_graph(edge_count: usize) -> ElkInputGraph {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut nodes = vec![group, node("outer")];
    let mut edges = Vec::with_capacity(edge_count);
    for index in 0..edge_count {
        let leaf_id = format!("leaf-{index}");
        let mut leaf = node(&leaf_id);
        leaf.parent = Some("group".to_string());
        nodes.push(leaf);
        edges.push(edge(&format!("edge-{index}"), &leaf_id, "outer"));
    }
    let mut input = graph(nodes, edges);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    input.options.merge_hierarchy_edges = false;
    input
}

fn wide_compound_sibling_graph(sibling_count: usize) -> ElkInputGraph {
    let mut nodes = Vec::with_capacity(sibling_count * 2 + 1);
    for index in 0..sibling_count {
        let group_id = format!("group-{index}");
        let leaf_id = format!("leaf-{index}");
        let mut group = node(&group_id);
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut leaf = node(&leaf_id);
        leaf.parent = Some(group_id);
        nodes.push(group);
        nodes.push(leaf);
    }
    nodes.push(node("outer"));
    let mut input = graph(nodes, vec![edge("crossing", "leaf-0", "outer")]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    input.options.merge_hierarchy_edges = false;
    input
}

fn compound_observed_work(input: &ElkInputGraph) -> CompoundPreprocessWork {
    let mut graph = import_graph(input).expect("compound fixture import");
    let mut work = CompoundPreprocessWork::default();
    preprocess_source_ported_compound_graph_observed(&mut graph, &mut work);
    work
}

// Mutation: in `source_ported_cross_hierarchy_segments`, give the segment between the two paths
// the depth `common_depth + 1`. The sorted depths then read 2, 1, 1, input 1, not 2, 1, 0,
// input 1.
#[test]
fn source_ported_segments_sort_like_cross_hierarchy_edge_comparator() {
    let segments = source_ported_cross_hierarchy_segments(
        "A",
        "B",
        "A:source",
        "B:target",
        &steps(&["outer", "inner"]),
        &steps(&["sibling"]),
    );

    let mut sorted = segments
        .iter()
        .map(|segment| segment.segment)
        .collect::<Vec<_>>();
    sorted.sort_by(|left, right| compare_compound_segments(*left, *right));

    assert_eq!(
        sorted,
        vec![
            CompoundEdgeSegment::Output { depth: 2 },
            CompoundEdgeSegment::Output { depth: 1 },
            CompoundEdgeSegment::Output { depth: 0 },
            CompoundEdgeSegment::Input { depth: 1 },
        ]
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, set `connects_parent_node` to `true`
// for the output steps. The boundary of `inner` then claims to connect the parent node, which
// it does not.
#[test]
fn source_ported_segments_mark_parent_boundary_endpoints() {
    let segments = source_ported_cross_hierarchy_segments(
        "A",
        "B",
        "A:source",
        "B:target",
        &steps(&["outer", "inner"]),
        &steps(&["sibling"]),
    );

    assert!(matches!(
        &segments[0].target,
        PendingSegmentEndpoint::ParentBoundary {
            node_id,
            port_key,
            connects_parent_node: false,
            ..
        } if node_id == "inner" && port_key == "A:source"
    ));
    assert!(matches!(
        &segments[2].target,
        PendingSegmentEndpoint::LocalNode { node_id, port_key }
            if node_id == "sibling" && port_key == "B:target"
    ));
    assert!(matches!(
        &segments[3].source,
        PendingSegmentEndpoint::ParentBoundary {
            node_id,
            port_key,
            connects_parent_node: false,
            ..
        } if node_id == "sibling" && port_key == "B:target"
    ));
}

// Mutation: vote by the dummy's port type alone (`votes_output = !leaves_port`), as a port
// with one edge would allow. The shared port then reads -1 instead of 1.
#[test]
fn edges_that_share_a_parent_port_vote_together() {
    let edge = |id: &str,
                source: &str,
                source_key: &str,
                source_path: &[&str],
                target: &str,
                target_key: &str,
                target_path: &[&str]| HierarchyEdge {
        id: id.into(),
        source_node_id: source.into(),
        target_node_id: target.into(),
        source_port_key: source_key.into(),
        target_port_key: target_key.into(),
        source_port_named: false,
        target_port_named: false,
        source_path: steps(source_path),
        target_path: steps(target_path),
        labels: Vec::new(),
        minlen: 1,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
    };
    let edges = [
        // Leaves the port for a direct child: votes "input".
        edge("e1", "P", "P:shared", &[], "c", "c:in", &["P"]),
        // Leaves the port for a grandchild: votes "output".
        edge("e2", "P", "P:shared", &[], "g", "g:in", &["P", "q"]),
        // Enters the port from a direct child: votes "output".
        edge("e3", "c", "c:out", &["P"], "P", "P:shared", &[]),
        // Another port that only feeds a direct child.
        edge("e4", "P", "P:other", &[], "c", "c:in2", &["P"]),
    ];
    let segments = edges
        .iter()
        .flat_map(materialize_hierarchy_edge_segments)
        .collect::<Vec<_>>();

    let flows = parent_port_net_flows(&segments);

    assert_eq!(flows.len(), 2);
    assert_eq!(flows[&("P".to_string(), "P:shared".to_string())], 1);
    assert_eq!(flows[&("P".to_string(), "P:other".to_string())], -1);
}

// Mutation: in `original_net_flows`, key the source of a hierarchy edge as a made-up port
// (`false` for `edge.source_port_named`). The edge out of the named port `G:2` then counts at
// the made-up port `G:2`, and the named port reads 0 instead of -1.
#[test]
fn a_made_up_port_keeps_its_net_flow_apart_from_a_named_port_of_the_same_id() {
    let port = |id: &str| crate::importer::ElkInputPort {
        id: id.to_string(),
        x: 0.0,
        y: 0.0,
        width: 6.0,
        height: 6.0,
        side: crate::graph::PortSide::West,
        index: None,
        border_offset: None,
        net_flow: 0,
        child_edges: 0,
        connected_to_external_nodes: false,
        inside_connections: false,
        has_edges: false,
        allow_non_flow_ports_to_switch_sides: false,
        labels: Vec::new(),
    };
    let mut group = node("G");
    group.ports = vec![port("G:1"), port("G:2")];
    let mut child = node("c");
    child.parent = Some("G".to_string());
    // The named port `G:2` leaves for its child: net flow -1.
    let mut leaving = edge("e1", "G", "c");
    leaving.source_port = Some("G:2".to_string());
    leaving.container = Some("root".to_string());
    // `s` enters `G` at a made-up port. G has two ports, so ELK's `createPort` would add a
    // third, and this importer calls it `G:2`, too. Net flow +1.
    let entering = edge("e2", "s", "G");
    let mut input = graph(vec![node("s"), group, child], vec![leaving, entering]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let lgraph = import_graph(&input).unwrap();

    let flows = original_net_flows(&lgraph);

    assert_eq!(flows[&("G".to_string(), "G:2".to_string(), true)], -1);
    assert_eq!(flows[&("G".to_string(), "G:2".to_string(), false)], 1);
}

// Mutation: in `materialize_hierarchy_edge_segments`, insert each label at the front of its
// segment's list (`insert(0, label)`). The labels then come out in reverse order.
#[test]
fn compound_label_buckets_preserve_order_identity_and_single_ownership() {
    for depth in [1, 16, 128] {
        let edge = HierarchyEdge {
            id: "ab".into(),
            source_node_id: "A".into(),
            target_node_id: "B".into(),
            source_port_key: "A:source".into(),
            target_port_key: "B:target".into(),
            source_port_named: false,
            target_port_named: false,
            source_path: (0..depth)
                .map(|index| HierarchyStep {
                    node_id: format!("group-{index}"),
                    graph: GraphId::new(index + 1),
                })
                .collect(),
            target_path: steps(&["other"]),
            labels: (0..4096)
                .map(|index| {
                    let mut label = LLabel::new(index.to_string(), 12.0, 10.0);
                    label.placement = [
                        EdgeLabelPlacement::Tail,
                        EdgeLabelPlacement::Center,
                        EdgeLabelPlacement::Head,
                    ][index % 3];
                    label.source_index = Some(index);
                    label
                })
                .collect(),
            minlen: 1,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
        };
        let output = materialize_hierarchy_edge_segments(&edge);
        let mut seen = vec![false; edge.labels.len()];
        for segment in output {
            let indices = segment
                .labels
                .iter()
                .map(|label| label.source_index.unwrap())
                .collect::<Vec<_>>();
            assert!(indices.windows(2).all(|pair| pair[0] < pair[1]));
            for label in segment.labels {
                let index = label.source_index.unwrap();
                assert!(!seen[index]);
                seen[index] = true;
                assert_eq!(label.original_label_edge.as_deref(), Some("ab"));
            }
        }
        assert!(seen.into_iter().all(|seen| seen));
    }
}

// Mutation: in `center_segment_index`, keep the deepest output segment (`depth >= current`).
// The label then lands on the output segment of depth 2, not depth 0.
#[test]
fn center_label_uses_shallowest_segment() {
    let segments = source_ported_cross_hierarchy_segments(
        "A",
        "B",
        "A:source",
        "B:target",
        &steps(&["outer", "inner"]),
        &steps(&["sibling"]),
    );

    let label_index = compound_label_segment_index(&segments, EdgeLabelPlacement::Center);

    assert_eq!(
        segments[label_index].segment,
        CompoundEdgeSegment::Output { depth: 0 }
    );
}

// Mutation: in `center_segment_index`, keep the deepest output segment (`depth >= current`).
// The center label then picks a different segment than the sorted reference does.
#[test]
fn allocation_free_label_selection_matches_stable_sorted_identity() {
    let segments = source_ported_cross_hierarchy_segments(
        "A",
        "B",
        "A:source",
        "B:target",
        &steps(&["outer", "inner"]),
        &steps(&["sibling"]),
    );
    let reference = |placement| {
        let mut sorted = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| (index, segment.segment))
            .collect::<Vec<_>>();
        sorted.sort_by(|(_, left), (_, right)| compare_compound_segments(*left, *right));
        match placement {
            EdgeLabelPlacement::Tail => sorted.first().map(|(index, _)| *index).unwrap_or(0),
            EdgeLabelPlacement::Head => sorted.last().map(|(index, _)| *index).unwrap_or(0),
            EdgeLabelPlacement::Center => sorted
                .iter()
                .position(|(_, segment)| matches!(segment, CompoundEdgeSegment::Input { .. }))
                .map(|index| index.saturating_sub(1))
                .or_else(|| sorted.len().checked_sub(1))
                .and_then(|index| sorted.get(index).map(|(segment_index, _)| *segment_index))
                .unwrap_or(0),
        }
    };

    for placement in [
        EdgeLabelPlacement::Tail,
        EdgeLabelPlacement::Center,
        EdgeLabelPlacement::Head,
    ] {
        assert_eq!(
            compound_label_segment_index(&segments, placement),
            reference(placement)
        );
    }
}

#[test]
fn compound_segment_registration_and_endpoint_indexing_are_linear() {
    let mut previous_total = None;
    for edge_count in [1usize, 8, 64, 256] {
        let work = compound_observed_work(&compound_fanout_graph(edge_count));
        assert_eq!(
            work.existing_cross_hierarchy_edges_indexed,
            work.segment_edges_processed
        );
        assert!(work.endpoint_nested_nodes_indexed > 0);
        assert!(work.endpoint_incident_edges_indexed > 0);

        let total = work.graph_edges_planned
            + work.existing_cross_hierarchy_edges_indexed
            + work.endpoint_nested_nodes_indexed
            + work.endpoint_incident_edges_indexed
            + work.segment_edges_processed;
        if let Some((previous_edges, previous_total)) = previous_total {
            assert!(
                total * previous_edges <= previous_total * edge_count + 8 * edge_count,
                "compound preprocessing work grew faster than its owned edge/node payload"
            );
        }
        previous_total = Some((edge_count, total));
    }
}

// Mutation: skip the `origin_port` branch of `parent_port_for_external_dummy`, so the port is
// found by its name and the first port of that name wins.
#[test]
fn compound_endpoint_link_uses_exact_port_identity_when_ids_repeat() {
    let mut graph = import_graph(&compound_fanout_graph(1)).expect("compound fixture import");
    introduce_source_ported_hierarchy_edge_segments(&mut graph);
    let group = graph.node_named("group");
    let edge_index = EdgeId::new(
        graph
            .edges
            .iter()
            .position(|edge| {
                edge.compound_segment.is_some()
                    && (edge.source().node == group || edge.target().node == group)
            })
            .expect("root compound segment"),
    );
    let original_port = if graph.edges[edge_index].source().node == group {
        graph.edges[edge_index].source()
    } else {
        graph.edges[edge_index].target()
    };
    let mut duplicate =
        graph.layerless_nodes[group].ports[original_port.port].clone_without_edges();
    duplicate.port_dummy = None;
    let duplicate_port = PortRef {
        node: group,
        port: graph.layerless_nodes[group].ports.push(duplicate),
    };
    if graph.edges[edge_index].source() == original_port {
        assert!(graph.set_edge_source(edge_index, duplicate_port));
    } else {
        assert!(graph.set_edge_target(edge_index, duplicate_port));
    }

    preprocess_source_ported_compound_graph(&mut graph);

    let nested = graph.layerless_nodes[group]
        .nested_graph
        .map(|id| &graph[id])
        .expect("compound child graph");
    let incident_dummy = nested
        .nodes_with_ids()
        .find(|(_, node)| {
            node.kind() == LNodeKind::ExternalPort
                && node.ports.iter().any(|port| {
                    port.incoming_edges()
                        .iter()
                        .chain(port.outgoing_edges().iter())
                        .any(|edge| nested.edges[*edge].id == "edge-0")
                })
        })
        .map(|(index, _)| index)
        .expect("incident external dummy");
    assert_eq!(
        nested.layerless_nodes[incident_dummy]
            .external_port()
            .unwrap()
            .origin_port
            .as_ref()
            .map(|origin| origin.port),
        Some(duplicate_port)
    );
    assert_eq!(
        graph.layerless_nodes[group].ports[duplicate_port.port]
            .port_dummy
            .as_ref()
            .map(|dummy| dummy.node),
        Some(incident_dummy)
    );
}

#[test]
fn parent_port_index_is_owner_local_in_wide_hierarchy() {
    for sibling_count in [1usize, 8, 64, 256] {
        let work = compound_observed_work(&wide_compound_sibling_graph(sibling_count));

        // Only group-0 owns an external dummy. Unrelated compound siblings must not trigger
        // whole-parent port-index rebuilds.
        assert_eq!(work.parent_port_indexes_built, 1);
    }
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the output segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn nested_exports_publish_child_ports_and_dummies_before_parent_edges() {
    // Minimal State007 provider oracle, including the reversed input direction:
    // Outer -> After is already local; Deep's export precedes InnerB's own edge.
    for reverse in [false, true] {
        for spacing in [20.0, 40.0] {
            let mut outer = node("Outer");
            outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            let mut inner = node("InnerB");
            inner.parent = Some("Outer".to_string());
            inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
            let mut deep = node("Deep");
            deep.parent = Some("InnerB".to_string());
            let mut input = graph(
                vec![outer, inner, deep, node("After")],
                ["Outer", "InnerB", "Deep"]
                    .into_iter()
                    .map(|id| {
                        if reverse {
                            edge(id, "After", id)
                        } else {
                            edge(id, id, "After")
                        }
                    })
                    .collect(),
            );
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
            input.options.spacing.edge_edge = spacing;
            let mut graph = import_graph(&input).unwrap();
            graph
                .try_for_each_graph_mut(|graph| {
                    graph.options.spacing.edge_edge = spacing;
                    Ok::<(), std::convert::Infallible>(())
                })
                .unwrap();
            preprocess_source_ported_compound_graph(&mut graph);
            let outer = graph
                .layerless_nodes
                .iter()
                .find(|node| node.id == "Outer")
                .unwrap();
            let incident = |port: &LPort| {
                if reverse {
                    port.incoming_edges()
                } else {
                    port.outgoing_edges()
                }
                .first()
                .copied()
            };
            let published = outer
                .ports
                .iter()
                .filter_map(incident)
                .map(|edge| graph.edges[edge].id.as_str())
                .collect::<Vec<_>>();
            assert_eq!(published, ["Outer", "Deep", "InnerB"]);

            let nested = outer.nested_graph.map(|id| &graph[id]).unwrap();
            let dummy_edges = nested
                .layerless_nodes
                .iter()
                .filter(|node| node.kind() == LNodeKind::ExternalPort)
                .filter_map(|node| {
                    let port = &node.ports[PortId::new(0)];
                    let edge = if reverse {
                        port.outgoing_edges().first()
                    } else {
                        port.incoming_edges().first()
                    }?;
                    assert_eq!(port.border_offset, Some(spacing / 2.0));
                    let origin = node
                        .external_port()
                        .unwrap()
                        .origin_port
                        .as_ref()
                        .unwrap()
                        .port;
                    let parent = &graph.layerless_nodes[origin.node].ports[origin.port];
                    assert_eq!(parent.border_offset, Some(spacing / 2.0));
                    assert_eq!(
                        graph.edges[incident(parent).unwrap()].id,
                        nested.edges[*edge].id
                    );
                    Some(nested.edges[*edge].id.as_str())
                })
                .collect::<Vec<_>>();
            assert_eq!(dummy_edges, ["Deep", "InnerB"]);
            for edge in &graph.edges {
                let (source, target) = if reverse {
                    ("After", edge.id.as_str())
                } else {
                    (edge.id.as_str(), "After")
                };
                assert_eq!(edge.source_node_id, source);
                assert_eq!(edge.target_node_id, target);
            }
        }
    }
}

#[test]
fn nested_publication_keeps_child_graph_then_local_node_order() {
    let mut outer = node("Outer");
    outer.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut first = node("First");
    first.parent = Some("Outer".to_string());
    let mut inner = node("Inner");
    inner.parent = Some("Outer".to_string());
    inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut deep = node("Deep");
    deep.parent = Some("Inner".to_string());
    let mut last = node("Last");
    last.parent = Some("Outer".to_string());
    let mut input = graph(
        vec![outer, first, inner, deep, last, node("After")],
        vec![
            edge("Last", "Last", "After"),
            edge("First", "First", "After"),
            edge("Deep", "Deep", "After"),
        ],
    );
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let mut graph = import_graph(&input).unwrap();
    preprocess_source_ported_compound_graph(&mut graph);
    let outer = graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "Outer")
        .unwrap();
    let publication = outer
        .ports
        .iter()
        .filter_map(|port| port.outgoing_edges().first())
        .map(|edge| graph.edges[*edge].id.as_str())
        .collect::<Vec<_>>();
    // All child exports precede direct nodes, including First which occurs before Inner.
    // The direct nodes then follow node order, not the reversed edge declaration order.
    assert_eq!(publication, ["Deep", "First", "Last"]);
}

#[test]
fn sibling_groups_publish_each_end_in_its_own_child_order() {
    let mut nodes = Vec::new();
    for (group_id, children) in [("Left", ["A", "B"]), ("Right", ["C", "D"])] {
        let mut group = node(group_id);
        group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        nodes.push(group);
        for child_id in children {
            let mut child = node(child_id);
            child.parent = Some(group_id.to_string());
            nodes.push(child);
        }
    }
    let mut input = graph(nodes, vec![edge("AD", "A", "D"), edge("BC", "B", "C")]);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let mut graph = import_graph(&input).unwrap();
    preprocess_source_ported_compound_graph(&mut graph);
    for (id, expected) in [("Left", ["AD", "BC"]), ("Right", ["BC", "AD"])] {
        let group = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap();
        let published = group
            .ports
            .iter()
            .map(|port| {
                let edge = port
                    .outgoing_edges()
                    .first()
                    .or_else(|| port.incoming_edges().first())
                    .unwrap();
                graph.edges[*edge].id.as_str()
            })
            .collect::<Vec<_>>();
        assert_eq!(published, expected, "{id} publishes its own child ports");
    }
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn scoped_publication_uses_local_input_side_without_changing_absolute_side() {
    for source_is_boundary in [false, true] {
        let mut inner = node("Inner");
        inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
        let mut a = node("A");
        a.parent = Some("Inner".to_string());
        let mut b = node("B");
        b.parent = Some("Inner".to_string());
        let mut input = graph(vec![node("Outside"), inner, a, b], Vec::new());
        input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
        let mut graph = import_graph(&input).unwrap();
        let mut segments = Vec::new();
        for target in ["B", "A"] {
            let edge = HierarchyEdge {
                id: target.to_string(),
                source_node_id: "Outside".to_string(),
                target_node_id: target.to_string(),
                source_port_key: "Outside:shared".to_string(),
                target_port_key: format!("{target}:input"),
                source_port_named: false,
                target_port_named: false,
                source_path: Vec::new(),
                target_path: vec![real_step(&graph, "Inner")],
                labels: Vec::new(),
                minlen: 1,
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
            };
            for mut scoped in materialize_hierarchy_edge_segments(&edge) {
                let depth = match scoped.pending.segment {
                    CompoundEdgeSegment::Input { depth }
                    | CompoundEdgeSegment::Output { depth } => depth,
                };
                scoped.pending.segment = CompoundEdgeSegment::Output { depth };
                if source_is_boundary && scoped.pending.lands_in.is_none() {
                    scoped.pending.source = PendingSegmentEndpoint::ParentBoundary {
                        node_id: "Outside".to_string(),
                        port_key: "Outside:shared".to_string(),
                        port_type: PortType::Input,
                        parent_port_type: PortType::Input,
                        connects_parent_node: false,
                    };
                }
                segments.push(scoped);
            }
        }
        introduce_source_ported_scoped_edge_segments(&mut graph, GraphId::ROOT, segments);
        assert_eq!(
            graph
                .edges
                .iter()
                .map(|edge| edge.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        let inner = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "Inner")
            .unwrap()
            .nested_graph
            .map(|id| &graph[id])
            .unwrap();
        assert_eq!(
            inner
                .edges
                .iter()
                .map(|edge| edge.id.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
        for graph in [&graph, inner] {
            assert!(graph.edges.iter().all(|edge| matches!(
                edge.compound_segment,
                Some(CompoundEdgeSegment::Output { .. })
            )));
        }
    }
}

fn parent_and_crossing_edge_graph(
    reverse: bool,
    spacing: f64,
    original_offset: Option<f64>,
) -> (GraphArena, String) {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let edges = if reverse {
        vec![
            edge("direct", "child", "group"),
            edge("crossing", "child", "outside"),
        ]
    } else {
        vec![
            edge("direct", "group", "child"),
            edge("crossing", "outside", "child"),
        ]
    };
    let mut input = graph(vec![group, child, node("outside")], edges);
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    input.options.merge_hierarchy_edges = false;
    input.options.spacing.edge_edge = spacing;
    let mut graph = import_graph(&input).unwrap();
    let direct = graph
        .hierarchy_edges
        .iter()
        .find(|edge| edge.id == "direct")
        .unwrap();
    let original_key = if reverse {
        direct.target_port_key.clone()
    } else {
        direct.source_port_key.clone()
    };
    let group = graph
        .layerless_nodes
        .iter_mut()
        .find(|node| node.id == "group")
        .unwrap();
    group
        .ports
        .slots_mut()
        .find(|port| port.id == original_key)
        .unwrap()
        .border_offset = original_offset;
    let nested = group.nested_graph.unwrap();
    graph[nested].options.spacing.edge_edge = spacing;
    (graph, original_key)
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the output segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn parent_border_offsets_survive_alongside_crossing_spacing_margins() {
    // Captured elkjs oracle: spacing 20/40 never changes a direct parent's offset;
    // original port offsets +7/-4 survive, while exported crossing ports keep spacing/2.
    for reverse in [false, true] {
        for spacing in [20.0, 40.0] {
            for original_offset in [None, Some(7.0), Some(-4.0)] {
                let (mut graph, key) =
                    parent_and_crossing_edge_graph(reverse, spacing, original_offset);
                let group_index = graph.node_named("group");
                let original_port = graph.layerless_nodes[group_index]
                    .ports
                    .iter_with_ids()
                    .find(|(_, port)| port.id == key)
                    .map(|(id, _)| id)
                    .unwrap();
                preprocess_source_ported_compound_graph(&mut graph);
                let group = &graph.layerless_nodes[group_index];
                let nested = group.nested_graph.map(|id| &graph[id]).unwrap();
                let parent = &group.ports[original_port];
                let dummy_ref = parent.port_dummy.as_ref().unwrap();
                let direct = &nested.layerless_nodes[dummy_ref.node];
                assert_eq!(
                    direct
                        .external_port()
                        .unwrap()
                        .origin_port
                        .as_ref()
                        .unwrap()
                        .port,
                    PortRef {
                        node: group_index,
                        port: original_port
                    }
                );
                assert_eq!(parent.border_offset, Some(original_offset.unwrap_or(0.0)));
                assert_eq!(
                    direct.ports[PortId::new(0)].border_offset,
                    Some(original_offset.unwrap_or(0.0))
                );
                if original_offset == Some(-4.0) {
                    let depth = match direct.external_port().unwrap().side {
                        PortSide::North | PortSide::South => direct.size.height,
                        PortSide::East | PortSide::West => direct.size.width,
                        PortSide::Undefined => panic!("external port side must be resolved"),
                    };
                    assert_eq!(
                        depth, 4.0,
                        "negative offsets must size the dummy before layout"
                    );
                }
                let crossing = nested
                    .edges
                    .iter()
                    .find(|edge| edge.id == "crossing")
                    .unwrap();
                let boundary = if reverse {
                    crossing.target()
                } else {
                    crossing.source()
                };
                let crossing_dummy = &nested.layerless_nodes[boundary.node];
                assert_eq!(crossing_dummy.kind(), LNodeKind::ExternalPort);
                assert_ne!(boundary.node, dummy_ref.node);
                assert_eq!(
                    crossing_dummy.ports[boundary.port].border_offset,
                    Some(spacing / 2.0)
                );
                let crossing_parent = crossing_dummy
                    .external_port()
                    .unwrap()
                    .origin_port
                    .as_ref()
                    .unwrap()
                    .port;
                assert_eq!(
                    graph.layerless_nodes[crossing_parent.node].ports[crossing_parent.port]
                        .border_offset,
                    Some(spacing / 2.0)
                );
            }
        }
    }
}

#[test]
fn routed_parent_ports_keep_their_original_border_offset() {
    for reverse in [false, true] {
        for original_offset in [None, Some(7.0), Some(-4.0)] {
            let (mut graph, key) = parent_and_crossing_edge_graph(reverse, 40.0, original_offset);
            crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();
            let group = graph
                .layerless_nodes
                .iter()
                .find(|node| node.id == "group")
                .unwrap();
            let port = group.ports.iter().find(|port| port.id == key).unwrap();
            let anchor = LPoint {
                x: port.position.x + port.anchor.x,
                y: port.position.y + port.anchor.y,
            };
            let offset = match port.side {
                PortSide::North => -anchor.y,
                PortSide::South => anchor.y - group.size.height,
                PortSide::West => -anchor.x,
                PortSide::East => anchor.x - group.size.width,
                PortSide::Undefined => panic!("routed parent port has no side"),
            };
            assert!(
                (offset - original_offset.unwrap_or(0.0)).abs() < 1e-9,
                "reverse={reverse}, requested={original_offset:?}, actual={offset}"
            );
        }
    }
}

// Mutation: in `hierarchy_port_key`, write the key as `node:role:order` instead of
// `node:order:role`. The dummy then reads `cluster:source:0`, not `cluster:0:source`.
#[test]
fn parent_end_external_dummy_uses_fixed_parent_port_side() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    cluster.port_constraints = Some(PortConstraints::FixedSide);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![cluster, child],
        vec![edge("cluster-A", "cluster", "A")],
    ))
    .unwrap();
    let cluster_index = lgraph.node_named("cluster");
    {
        let parent_port = &mut lgraph.layerless_nodes[cluster_index].ports[PortId::new(0)];
        parent_port.set_side(PortSide::East);
        parent_port.position = LPoint { x: 80.0, y: 20.0 };
        parent_port.size = LSize {
            width: 6.0,
            height: 8.0,
        };
        parent_port.border_offset = Some(-4.0);
    }

    preprocess_source_ported_compound_graph(&mut lgraph);

    let cluster = &lgraph.layerless_nodes[cluster_index];
    let parent_port = &cluster.ports[PortId::new(0)];
    let port_dummy = parent_port
        .port_dummy
        .as_ref()
        .expect("parent port should link to the nested external dummy");
    let nested = cluster.nested_graph.map(|id| &lgraph[id]).unwrap();
    let external = &nested.layerless_nodes[port_dummy.node];

    assert_eq!(parent_port.side, PortSide::South);
    assert_eq!(external.external_port().unwrap().side, PortSide::South);
    assert_eq!(
        external.external_port().unwrap().parent_port_key.as_deref(),
        Some("cluster:0:source")
    );
    assert_eq!(
        external.external_port().unwrap().parent_port_type,
        Some(PortType::Output)
    );
    assert_eq!(external.ports[PortId::new(0)].side, PortSide::North);
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the input segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn parent_end_external_dummy_uses_parent_port_net_flow() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());

    let mut lgraph = import_graph(&graph(
        vec![cluster, child],
        vec![edge("cluster-A", "cluster", "A")],
    ))
    .unwrap();
    let cluster_index = lgraph.node_named("cluster");

    preprocess_source_ported_compound_graph(&mut lgraph);

    let cluster = &lgraph.layerless_nodes[cluster_index];
    let parent_port = &cluster.ports[PortId::new(0)];
    assert_eq!(parent_port.net_flow(), 0);
    let port_dummy = parent_port
        .port_dummy
        .as_ref()
        .expect("parent port should link to the nested external dummy");
    let nested = cluster.nested_graph.map(|id| &lgraph[id]).unwrap();
    let external = &nested.layerless_nodes[port_dummy.node];

    assert_eq!(
        external.external_port().unwrap().parent_port_type,
        Some(PortType::Output)
    );
    assert_eq!(external.external_port().unwrap().side, PortSide::North);
    assert_eq!(external.ports[PortId::new(0)].side, PortSide::South);
}

// Mutation: read the port constraints of S2 after the sibling's dummy is linked, so S2 is
// fixed by side and its unlinked port keeps the side it got from the direction.
#[test]
fn unlinked_parent_port_uses_net_flow_after_sibling_external_port_fixing() {
    let mut s1 = node("S1");
    s1.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut sub1 = node("sub1");
    sub1.parent = Some("S1".to_string());
    let mut s2 = node("S2");
    s2.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut sub4 = node("sub4");
    sub4.parent = Some("S2".to_string());

    let mut lgraph = import_graph(&graph(
        vec![s1, sub1, s2, sub4],
        vec![edge("S1-S2", "S1", "S2"), edge("sub1-sub4", "sub1", "sub4")],
    ))
    .unwrap();
    let s2_index = lgraph.node_named("S2");

    preprocess_source_ported_compound_graph(&mut lgraph);

    let s2 = &lgraph.layerless_nodes[s2_index];
    let parent_port = s2
        .ports
        .iter()
        .find(|port| port.id == "S2:0" && port.port_type == PortType::Input)
        .expect("top-level edge target port should exist on S2");
    let port_dummy = parent_port
        .port_dummy
        .as_ref()
        .expect("S2 top-level target port should link to nested external dummy");
    let nested = s2.nested_graph.map(|id| &lgraph[id]).unwrap();
    let external = &nested.layerless_nodes[port_dummy.node];

    // One edge enters the port, so the flow is -1 for the dummy. The layout flows down, so
    // the dummy goes to the north, whatever side the port had before.
    assert_eq!(parent_port.net_flow(), 1);
    assert_eq!(parent_port.side, PortSide::North);
    assert_eq!(external.external_port().unwrap().side, PortSide::North);
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the output segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn nested_graph_hierarchy_edges_resolve_their_relative_root_scope() {
    let mut group = node("group");
    group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut inner = node("inner");
    inner.parent = Some("group".to_string());
    inner.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
    let mut leaf = node("leaf");
    leaf.parent = Some("inner".to_string());
    let mut sibling = node("sibling");
    sibling.parent = Some("group".to_string());
    let mut input = graph(vec![group, inner, leaf, sibling], Vec::new());
    input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;
    let mut lgraph = import_graph(&input).unwrap();
    let group_graph = lgraph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("group graph is materialized");
    let inner_step = real_step(&lgraph, "inner");
    lgraph[group_graph].hierarchy_edges.push(HierarchyEdge {
        id: "leaf-sibling".to_string(),
        source_node_id: "leaf".to_string(),
        target_node_id: "sibling".to_string(),
        source_port_key: "leaf:source".to_string(),
        target_port_key: "sibling:target".to_string(),
        source_port_named: false,
        target_port_named: false,
        source_path: vec![inner_step],
        target_path: Vec::new(),
        labels: Vec::new(),
        minlen: 1,
        model_order: Some(3),
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
    });

    preprocess_source_ported_compound_graph(&mut lgraph);

    let group_graph = lgraph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .map(|id| &lgraph[id])
        .expect("group graph remains materialized");
    assert!(group_graph.hierarchy_edges.is_empty());
    assert_eq!(group_graph.cross_hierarchy_edges.len(), 1);
    assert_eq!(
        group_graph.cross_hierarchy_edges[0].original_model_order,
        Some(3)
    );
    let inner_graph = group_graph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .map(|id| &lgraph[id])
        .expect("inner graph remains materialized");
    assert_eq!(inner_graph.cross_hierarchy_edges.len(), 1);
    assert_eq!(
        inner_graph.cross_hierarchy_edges[0].original_model_order,
        Some(3)
    );
}

// Mutation: in `source_ported_cross_hierarchy_segments`, let the output segments land in
// `GraphId::ROOT` instead of the graph of their step.
#[test]
fn compound_external_ports_are_small_stack_safe_for_deep_mixed_hierarchy() {
    const DEPTH: usize = 512;

    std::thread::Builder::new()
        .name("elk-compound-external-port-small-stack".to_string())
        .stack_size(128 * 1024)
        .spawn(|| {
            let mut nodes = Vec::with_capacity(DEPTH + 2);
            for depth in 0..DEPTH {
                let group_id = format!("group-{depth}");
                let mut group = node(&group_id);
                group.parent = depth.checked_sub(1).map(|parent| format!("group-{parent}"));
                group.hierarchy_handling = Some(HierarchyHandling::IncludeChildren);
                nodes.push(group);
            }
            let mut leaf = node("leaf");
            leaf.parent = Some(format!("group-{}", DEPTH - 1));
            nodes.push(leaf);
            nodes.push(node("outer"));
            let mut input = graph(nodes, vec![edge("leaf-outer", "leaf", "outer")]);
            input.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

            let mut lgraph =
                std::mem::ManuallyDrop::new(import_graph(&input).expect("deep hierarchy import"));
            // The recursive-layout owner materializes SeparateChildren scopes before they
            // reach this pass. Alternate the retained scope policy after import so this one
            // materialized chain covers the mixed-policy ancestry seen by preprocessing.
            let mut graph_order = 0usize;
            lgraph
                .try_for_each_graph_mut(|graph| {
                    graph.options.hierarchy_handling = if graph_order.is_multiple_of(2) {
                        HierarchyHandling::IncludeChildren
                    } else {
                        HierarchyHandling::SeparateChildren
                    };
                    graph_order += 1;
                    Ok::<(), std::convert::Infallible>(())
                })
                .expect("hierarchy option update is infallible");

            preprocess_source_ported_compound_graph(&mut lgraph);

            let mut current: &LGraph = &lgraph;
            for depth in 0..DEPTH {
                let group_id = format!("group-{depth}");
                let group = current
                    .layerless_nodes
                    .iter()
                    .find(|node| node.id == group_id)
                    .expect("every group remains in its parent graph");
                assert!(
                    group.ports.iter().any(|port| port.port_dummy.is_some()),
                    "group {group_id} must link to its nested external dummy"
                );
                current = group
                    .nested_graph
                    .map(|id| &lgraph[id])
                    .expect("every group retains its materialized child graph");
            }
        })
        .expect("small-stack worker should start")
        .join()
        .expect("compound preprocessing must not overflow the worker stack");
}

// Mutation: pick the graph of a segment by the id of its parent node, as the segment used to
// name it, and take the first graph with that name.
#[test]
fn a_segment_lands_in_the_graph_its_id_names_even_when_two_graphs_share_a_parent_name() {
    let options = LayeredOptions::default();
    let mut graphs = GraphArena::new(crate::graph::LGraph::new("root", options.clone()));
    let mut lands = Vec::new();
    for (position, holder) in ["p0", "p1"].into_iter().enumerate() {
        graphs
            .root_mut()
            .layerless_nodes
            .push(crate::graph::LNode::new(holder, 10.0, 10.0, None));
        let mut nested = crate::graph::LGraph::new("dup", options.clone());
        for child in ["a", "b"] {
            nested
                .layerless_nodes
                .push(crate::graph::LNode::new(child, 1.0, 1.0, None));
        }
        lands.push(graphs.nest(GraphId::ROOT, NodeId::new(position), nested));
    }
    let edge = HierarchyEdge {
        id: "e".to_string(),
        source_node_id: "a".to_string(),
        target_node_id: "b".to_string(),
        source_port_key: "a:out".to_string(),
        target_port_key: "b:in".to_string(),
        source_port_named: false,
        target_port_named: false,
        source_path: Vec::new(),
        target_path: Vec::new(),
        labels: Vec::new(),
        minlen: 1,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
    };
    let segment = ScopedHierarchySegment {
        edge,
        pending: PendingCompoundSegment {
            lands_in: Some(lands[1]),
            source: PendingSegmentEndpoint::LocalNode {
                node_id: "a".to_string(),
                port_key: "a:out".to_string(),
            },
            target: PendingSegmentEndpoint::LocalNode {
                node_id: "b".to_string(),
                port_key: "b:in".to_string(),
            },
            segment: CompoundEdgeSegment::Output { depth: 1 },
        },
        labels: Vec::new(),
    };

    introduce_source_ported_scoped_edge_segments(&mut graphs, GraphId::ROOT, vec![segment]);

    assert_eq!(graphs[lands[0]].edges.len(), 0);
    assert_eq!(graphs[lands[1]].edges.len(), 1);
    assert_eq!(graphs[lands[1]].cross_hierarchy_edges[0].graph_id, lands[1]);
}

// Mutation: key the parent ports by the parent node's id again (`HashMap<String,
// ParentNodeInfo>`). The graph inside the inner `p` then gets the default info, and its
// dummy takes the side of the layout direction, not the side of the inner `p`'s port.
//
// `layout()` and the importer refuse two nodes with one id, so this test builds the graphs by
// hand: the node `p` of the root holds a graph, and that graph holds another node `p`.
#[test]
fn two_parent_nodes_with_one_id_each_give_their_own_ports_to_their_graph() {
    let options = LayeredOptions::default();
    let mut graphs = GraphArena::new(crate::graph::LGraph::new("root", options.clone()));
    let mut lands = Vec::new();
    let mut holder = GraphId::ROOT;
    // The root's `p` has its port `pp` on the north side, the inner `p` on the south side.
    for side in [PortSide::North, PortSide::South] {
        let mut parent = crate::graph::LNode::new("p", 10.0, 10.0, None);
        parent.port_constraints = PortConstraints::FixedSide;
        let mut port = LPort::new("pp", crate::graph::NodeId::new(0), PortType::Output);
        port.named = true;
        port.side = side;
        parent.ports.push(port);
        let mut nested = crate::graph::LGraph::new("nested", options.clone());
        nested
            .layerless_nodes
            .push(crate::graph::LNode::new("child", 1.0, 1.0, None));
        graphs[holder].layerless_nodes.push(parent);
        let node = graphs[holder].layerless_nodes.len() - 1;
        let node = crate::graph::NodeId::new(node);
        holder = graphs.nest(holder, node, nested);
        lands.push(holder);
    }
    assert_eq!(graphs[lands[0]].layerless_nodes[NodeId::new(1)].id, "p");
    assert_eq!(graphs[lands[1]].layerless_nodes[NodeId::new(0)].id, "child");
    let segments = lands
        .iter()
        .map(|&lands_in| ScopedHierarchySegment {
            edge: HierarchyEdge {
                id: format!("e{}", lands_in.index()),
                source_node_id: "child".to_string(),
                target_node_id: "p".to_string(),
                source_port_key: "child:out".to_string(),
                target_port_key: "pp".to_string(),
                source_port_named: false,
                target_port_named: true,
                source_path: Vec::new(),
                target_path: Vec::new(),
                labels: Vec::new(),
                minlen: 1,
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
            },
            pending: PendingCompoundSegment {
                lands_in: Some(lands_in),
                source: PendingSegmentEndpoint::LocalNode {
                    node_id: "child".to_string(),
                    port_key: "child:out".to_string(),
                },
                target: PendingSegmentEndpoint::ParentBoundary {
                    node_id: "p".to_string(),
                    port_key: "pp".to_string(),
                    port_type: PortType::Output,
                    parent_port_type: PortType::Output,
                    connects_parent_node: true,
                },
                segment: CompoundEdgeSegment::Output { depth: 1 },
            },
            labels: Vec::new(),
        })
        .collect::<Vec<_>>();

    introduce_source_ported_scoped_edge_segments(&mut graphs, GraphId::ROOT, segments);

    let dummy_side = |graph: GraphId| {
        let dummies = graphs[graph]
            .layerless_nodes
            .iter()
            .filter(|node| node.kind() == LNodeKind::ExternalPort)
            .collect::<Vec<_>>();
        assert_eq!(dummies.len(), 1);
        dummies[0].external_port().unwrap().side
    };
    assert_eq!(dummy_side(lands[0]), PortSide::North);
    assert_eq!(dummy_side(lands[1]), PortSide::South);
}

// --- Room for the labels of a compound node's ports: `CompoundGraphPreprocessor`. ---

mod port_label_room {
    use super::*;
    use crate::graph::{ExternalPortData, GraphNodeRef, LLabel, LSize};
    use crate::options::PortLabelPlacement;

    /// The outer graph, with a node holding one port of 6 by 4 on `side` with the labels, and the
    /// graph inside it, with the dummy of that port.
    fn setup(
        side: PortSide,
        placement: PortLabelPlacement,
        labels: Vec<LLabel>,
        constraints: PortConstraints,
    ) -> (LGraph, LGraph, NodeId) {
        let mut outer = LGraph::new("outer", LayeredOptions::default());
        let node = outer.push_node_with(|_| LNode::new("cluster", 80.0, 40.0, None));
        outer.layerless_nodes[node].port_label_placement = placement;
        let port = outer
            .add_port(node, PortType::Output, side, LPoint::default())
            .unwrap();
        outer.layerless_nodes[node].ports[port.port].size = LSize {
            width: 6.0,
            height: 4.0,
        };
        outer.layerless_nodes[node].ports[port.port].labels = labels;

        let mut inner = LGraph::new("inner", LayeredOptions::default());
        inner.id = GraphId::new(1);
        let dummy = inner.push_node_with(|id| {
            create_external_port_dummy(
                id,
                "dummy",
                "dummy:0",
                PortType::Output,
                constraints,
                side,
                0,
                LPoint::default(),
                LSize {
                    width: 6.0,
                    height: 4.0,
                },
                LSize::default(),
                0.0,
                ElkDirection::Right,
                ExternalPortData::default(),
            )
        });
        outer.layerless_nodes[node].ports[port.port].port_dummy = Some(GraphNodeRef {
            graph_id: GraphId::new(1),
            node: dummy,
        });
        (outer, inner, dummy)
    }

    fn label(width: f64, height: f64, x: f64, y: f64) -> LLabel {
        let mut label = LLabel::new("text", width, height);
        label.position = LPoint { x, y };
        label
    }

    fn room(
        side: PortSide,
        placement: PortLabelPlacement,
        constraints: PortConstraints,
        labels: Vec<LLabel>,
    ) -> Vec<(f64, f64)> {
        let (outer, mut inner, dummy) = setup(side, placement, labels, constraints);
        let node = NodeId::new(0);
        reserve_room_for_port_labels(&outer, node, PortId::new(0), &mut inner, constraints, side);
        inner.layerless_nodes[dummy].ports[PortId::new(0)]
            .labels
            .iter()
            .map(|label| (label.size.width, label.size.height))
            .collect()
    }

    // Mutation: reduce the labels also under INSIDE port labels.
    #[test]
    fn inside_labels_keep_their_size() {
        let labels = vec![label(20.0, 10.0, 0.0, 0.0), label(30.0, 6.0, 0.0, 0.0)];
        for side in [PortSide::East, PortSide::North] {
            assert_eq!(
                room(
                    side,
                    PortLabelPlacement::INSIDE,
                    PortConstraints::FixedSide,
                    labels.clone()
                ),
                vec![(20.0, 10.0), (30.0, 6.0)]
            );
        }
    }

    // Mutation: reduce the height of a label on an east or west port, or its width on a north
    // or south port.
    #[test]
    fn outside_labels_lose_the_size_along_the_axis_of_a_fixed_side() {
        let labels = vec![label(20.0, 10.0, 0.0, 0.0)];
        let outside = |side| {
            room(
                side,
                PortLabelPlacement::OUTSIDE,
                PortConstraints::FixedSide,
                labels.clone(),
            )
        };
        assert_eq!(outside(PortSide::East), vec![(0.0, 10.0)]);
        assert_eq!(outside(PortSide::West), vec![(0.0, 10.0)]);
        assert_eq!(outside(PortSide::North), vec![(20.0, 0.0)]);
        assert_eq!(outside(PortSide::South), vec![(20.0, 0.0)]);
    }

    // Mutation: read the side of a port with free constraints, which the node may still change.
    // ELK expects such a port on the east or west side: the width goes.
    #[test]
    fn outside_labels_of_a_free_port_lose_their_width_whatever_the_side() {
        let labels = vec![label(20.0, 10.0, 0.0, 0.0)];
        for side in [PortSide::North, PortSide::South, PortSide::East] {
            assert_eq!(
                room(
                    side,
                    PortLabelPlacement::OUTSIDE,
                    PortConstraints::Free,
                    labels.clone()
                ),
                vec![(0.0, 10.0)]
            );
        }
    }

    // Mutation: keep the inside part for placements that are not fixed, or take it with the
    // border offset of the port.
    #[test]
    fn fixed_labels_keep_the_part_that_reaches_into_the_node() {
        // A label 12 to the left of the east port: 12 of it lie in the node.
        let east = vec![label(20.0, 10.0, -12.0, 0.0)];
        assert_eq!(
            room(
                PortSide::East,
                PortLabelPlacement::FIXED,
                PortConstraints::FixedSide,
                east.clone()
            ),
            vec![(12.0, 10.0)]
        );
        // The same label under OUTSIDE placement that is not fixed: nothing reaches in.
        assert_eq!(
            room(
                PortSide::East,
                PortLabelPlacement::OUTSIDE,
                PortConstraints::FixedSide,
                east
            ),
            vec![(0.0, 10.0)]
        );
        // On a north port, a label 1 below the port top reaches 10 + 1 - 4 = 7 into the node.
        let north = vec![label(20.0, 10.0, 0.0, 1.0)];
        assert_eq!(
            room(
                PortSide::North,
                PortLabelPlacement::FIXED,
                PortConstraints::FixedSide,
                north
            ),
            vec![(20.0, 7.0)]
        );
    }

    // Mutation: read the current side of the port, not the side the importer gave it. A dummy
    // that links to the port changes the side, and ELK reads the old one: here "undefined",
    // which gives a fixed label no part inside the node.
    #[test]
    fn the_inside_part_follows_the_side_the_importer_gave_the_port() {
        let (outer, mut inner, dummy) = setup(
            PortSide::East,
            PortLabelPlacement::FIXED,
            vec![label(20.0, 10.0, -12.0, 0.0)],
            PortConstraints::Free,
        );
        reserve_room_for_port_labels(
            &outer,
            NodeId::new(0),
            PortId::new(0),
            &mut inner,
            PortConstraints::Free,
            PortSide::Undefined,
        );
        let at = &inner.layerless_nodes[dummy].ports[PortId::new(0)].labels[0];
        assert_eq!((at.size.width, at.size.height), (0.0, 10.0));
    }

    // Mutation: read the current side of the port for the choice of the axis. The importer gave
    // the port the north side, so ELK cuts the height of the label, not its width.
    #[test]
    fn the_axis_follows_the_side_the_importer_gave_the_port() {
        let (outer, mut inner, dummy) = setup(
            PortSide::East,
            PortLabelPlacement::OUTSIDE,
            vec![label(20.0, 10.0, 0.0, 0.0)],
            PortConstraints::FixedSide,
        );
        reserve_room_for_port_labels(
            &outer,
            NodeId::new(0),
            PortId::new(0),
            &mut inner,
            PortConstraints::FixedSide,
            PortSide::North,
        );
        let at = &inner.layerless_nodes[dummy].ports[PortId::new(0)].labels[0];
        assert_eq!((at.size.width, at.size.height), (20.0, 0.0));
    }

    // Mutation: leave out the `filter` on the graph of the dummy. A port whose dummy an earlier
    // step put into another graph would give its labels to an unrelated node there.
    #[test]
    fn a_dummy_in_another_graph_gets_no_labels() {
        let (mut outer, mut inner, dummy) = setup(
            PortSide::East,
            PortLabelPlacement::INSIDE,
            vec![label(20.0, 10.0, 0.0, 0.0)],
            PortConstraints::FixedSide,
        );
        outer.layerless_nodes[NodeId::new(0)].ports[PortId::new(0)].port_dummy =
            Some(GraphNodeRef {
                graph_id: GraphId::new(7),
                node: dummy,
            });
        inner.id = GraphId::new(1);
        reserve_room_for_port_labels(
            &outer,
            NodeId::new(0),
            PortId::new(0),
            &mut inner,
            PortConstraints::FixedSide,
            PortSide::East,
        );
        assert!(
            inner.layerless_nodes[dummy].ports[PortId::new(0)]
                .labels
                .is_empty()
        );
    }

    // Mutation: copy the labels in reverse, or only the first one.
    #[test]
    fn every_label_of_the_port_gets_one_on_the_dummy_in_order() {
        let labels = vec![
            label(20.0, 10.0, 0.0, 0.0),
            label(30.0, 6.0, 0.0, 0.0),
            label(5.0, 5.0, 0.0, 0.0),
        ];
        assert_eq!(
            room(
                PortSide::East,
                PortLabelPlacement::INSIDE,
                PortConstraints::FixedSide,
                labels
            ),
            vec![(20.0, 10.0), (30.0, 6.0), (5.0, 5.0)]
        );
    }

    // Mutation: copy the text of the label to the dummy's label. ELK makes a label without text.
    #[test]
    fn the_dummy_labels_have_no_text() {
        let (outer, mut inner, dummy) = setup(
            PortSide::East,
            PortLabelPlacement::INSIDE,
            vec![label(20.0, 10.0, 0.0, 0.0)],
            PortConstraints::FixedSide,
        );
        reserve_room_for_port_labels(
            &outer,
            NodeId::new(0),
            PortId::new(0),
            &mut inner,
            PortConstraints::FixedSide,
            PortSide::East,
        );
        assert_eq!(
            inner.layerless_nodes[dummy].ports[PortId::new(0)].labels[0].text,
            ""
        );
    }

    // Mutation: unwrap the dummy of the port, which panics for a port without one.
    #[test]
    fn a_port_without_a_dummy_gets_nothing() {
        let (mut outer, mut inner, dummy) = setup(
            PortSide::East,
            PortLabelPlacement::INSIDE,
            vec![label(20.0, 10.0, 0.0, 0.0)],
            PortConstraints::FixedSide,
        );
        outer.layerless_nodes[NodeId::new(0)].ports[PortId::new(0)].port_dummy = None;
        reserve_room_for_port_labels(
            &outer,
            NodeId::new(0),
            PortId::new(0),
            &mut inner,
            PortConstraints::FixedSide,
            PortSide::East,
        );
        assert!(
            inner.layerless_nodes[dummy].ports[PortId::new(0)]
                .labels
                .is_empty()
        );
    }
}

// Mutation: read the first port for every port, or list the ports of a node without a graph.
#[test]
fn the_port_sides_are_kept_by_port_position_for_nodes_with_a_graph_only() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let holder = graph.push_node_with(|_| LNode::new("holder", 10.0, 10.0, None));
    let plain = graph.push_node_with(|_| LNode::new("plain", 10.0, 10.0, None));
    graph.layerless_nodes[holder].nested_graph = Some(GraphId::new(1));
    for (node, side) in [
        (holder, PortSide::North),
        (holder, PortSide::West),
        (plain, PortSide::East),
    ] {
        graph
            .add_port(node, PortType::Output, side, LPoint::default())
            .unwrap();
    }
    let sides = port_sides_of(&graph);
    assert_eq!(sides[holder], vec![PortSide::North, PortSide::West]);
    assert!(sides[plain].is_empty());
}
