//! Tests of the classes in this module.

use std::collections::HashMap;

use crate::graph::{
    EdgeId, GraphArena, GraphId, LGraph, LNode, LNodeKind, NodeId, PortId, PortRef, PortSide,
    PortType,
};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::intermediate::greedyswitch::between_layer_edge_two_node_crossings_counter::{
    Adjacency, AdjacencyList, BetweenLayerEdgeTwoNodeCrossingsCounter,
};
use crate::intermediate::greedyswitch::crossing_matrix_filler::{
    CrossingCountSide, CrossingMatrixFiller,
};
use crate::intermediate::greedyswitch::greedy_switch_heuristic::GreedySwitchHeuristic;
use crate::intermediate::greedyswitch::north_south_edge_neighbouring_node_crossings_counter::NorthSouthEdgeNeighbouringNodeCrossingsCounter;
use crate::intermediate::greedyswitch::switch_decider::{
    SwitchDecider, has_edges_on_side, have_layout_unit_constraints, have_successor_constraints,
};
use crate::intermediate::split_long_edges;
use crate::intermediate::{process_port_sides, sort_port_lists};
use crate::options::{ElkDirection, HierarchyHandling, LayeredOptions};
use crate::options::{OrderingStrategy, PortConstraints};
use crate::phase::layering::layer_network_simplex;
use crate::phase::ordering::abstract_barycenter_port_distributor::{
    BarycenterPortDistributor, north_south_port_barycenter_sum,
};
use crate::phase::ordering::barycenter_heuristic::{BarycenterHeuristic, BarycenterState};
use crate::phase::ordering::counting::crossings_counter::CrossingsCounter;
use crate::phase::ordering::forster_constraint_resolver::{
    ConstraintGroup, merge_constraint_groups,
};
use crate::phase::ordering::graph_info_holder::initialize_crossing_minimization_port_ids;
use crate::phase::ordering::greedy_port_distributor::GreedyPortDistributor;
use crate::phase::ordering::layer_sweep_crossing_minimizer::{
    CrossMinType, HierarchySweep, apply_parent_hierarchical_port_order_on_sweep_side,
    minimize_crossings_layer_sweep,
};
use crate::phase::ordering::layer_sweep_type_decider::external_dummy_origin_side;
use crate::phase::ordering::sweep_port_distributor::{PortDistributor, PortDistributorKind};
use crate::random::JavaRandom;

/// Sweeps one graph alone, as a flat layout does: the graph takes the root's place in an arena.
fn sweep_alone(graph: &mut LGraph, cross_min_type: CrossMinType) -> bool {
    let mut graphs = GraphArena::new(LGraph::new("alone", LayeredOptions::default()));
    std::mem::swap(&mut graphs[GraphId::ROOT], graph);
    let changed = minimize_crossings_layer_sweep(&mut graphs, GraphId::ROOT, false, cross_min_type);
    std::mem::swap(&mut graphs[GraphId::ROOT], graph);
    changed
}

// Mutation: in `add_adjacency`, merge with any entry at the same position, not only the last.
// A second mutation deletes the sort in `AdjacencyList::from_positions`. Either one changes the
// counted crossings.
#[test]
fn an_adjacency_merges_only_with_the_entry_collected_just_before_it() {
    // Edges to positions 5, 5, 3 and 5: the first two merge, the last stays its own entry,
    // and the stable sort keeps it behind the merged one.
    let upper = AdjacencyList::from_positions([5, 5, 3, 5]);
    let entries: Vec<(usize, usize)> = upper
        .adjacency_list
        .iter()
        .map(|adjacency| (adjacency.position, adjacency.cardinality))
        .collect();
    assert_eq!(entries, vec![(3, 1), (5, 2), (5, 1)]);

    // One lower edge to position 5. The upper edge to 3 lies above it, which is one crossing
    // for the switched order. At position 5 the merged entry holds 2 of the 3 upper edges
    // left, so ELK counts the separate third one as below: one crossing in this order. A
    // full merge would count none.
    let mut counter = BetweenLayerEdgeTwoNodeCrossingsCounter::new();
    counter.count_crossings_by_merging_adjacency_lists(upper, AdjacencyList::from_positions([5]));
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (1, 1)
    );
}

// Mutation: in `AdjacencyList::from_positions`, count `size` from the entries instead of the
// positions (`size = adjacency_list.len()`). The single list then still holds 1, but the
// two-edge list below holds 1 instead of 2.
#[test]
fn an_adjacency_list_of_no_edge_or_one_edge_counts_nothing_against_itself() {
    // A node with no edge on the side: an empty list that stays empty after a reset.
    let mut empty = AdjacencyList::from_positions([]);
    assert!(empty.adjacency_list.is_empty());
    assert_eq!((empty.size, empty.current_size), (0, 0));
    empty.reset();
    assert!(empty.is_empty());

    // One edge: one entry of cardinality 1, with nothing below it.
    let single = AdjacencyList::from_positions([7]);
    assert_eq!(single.adjacency_list, vec![Adjacency::new(7)]);
    assert_eq!(single.count_adjacencies_below_node_of_first_port(), 0);

    // Two edges to one port merge into one entry, but the list still holds two edges.
    let double = AdjacencyList::from_positions([7, 7]);
    assert_eq!(double.adjacency_list.len(), 1);
    assert_eq!(double.size, 2);

    // An empty list against a full one counts no crossing in either order.
    let mut counter = BetweenLayerEdgeTwoNodeCrossingsCounter::new();
    counter.count_crossings_by_merging_adjacency_lists(empty, single);
    assert_eq!(
        (counter.upper_lower_crossings, counter.lower_upper_crossings),
        (0, 0)
    );
}

// Mutation: in `AdjacencyList::from_positions`, use `sort_unstable_by_key`. The test uses
// 2,000 entries, far above the small-sort cutoff of the standard library: below it Rust sorts
// by insertion, which keeps the order of equal entries even in the unstable sort.
#[test]
fn the_sort_keeps_entries_at_one_position_in_the_order_they_were_collected() {
    // A thousand rounds of edges to position 1, twice in even rounds and once in odd ones,
    // each round closed by an edge to position 0. No entry merges across a round, so position
    // 1 holds 1,000 entries whose cardinality alternates 2, 1, 2, 1.
    let mut positions = Vec::new();
    for round in 0..1000 {
        positions.push(1);
        if round % 2 == 0 {
            positions.push(1);
        }
        positions.push(0);
    }
    let list = AdjacencyList::from_positions(positions);
    let at_one: Vec<usize> = list
        .adjacency_list
        .iter()
        .filter(|adjacency| adjacency.position == 1)
        .map(|adjacency| adjacency.cardinality)
        .collect();
    let collected: Vec<usize> = (0..1000).map(|round| 2 - round % 2).collect();
    assert_eq!(at_one, collected);
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
        node_label_placement: crate::options::NodeLabelPlacement::FIXED,
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: None,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
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

fn nested_node(id: &str, parent: Option<&str>) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width: 54.0,
        height: 68.0,
        parent: parent.map(str::to_string),
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_alignment: None,
        port_constraints: None,
        node_label_placement: crate::options::NodeLabelPlacement::FIXED,
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: None,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
    }
}

fn nested_group(id: &str, parent: Option<&str>) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width: 0.0,
        height: 0.0,
        parent: parent.map(str::to_string),
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_alignment: None,
        port_constraints: None,
        node_label_placement: crate::options::NodeLabelPlacement::from_location(
            crate::options::NodeLabelLocation::InsideTopCenter,
        ),
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: Some(Box::new(crate::options::LayeredOptions {
            spacing: crate::options::SpacingOptions::layered_base_value(30.0),
            ..Default::default()
        })),
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
    }
}

fn prepared_graph(nodes: Vec<ElkInputNode>, edges: Vec<ElkInputEdge>) -> crate::graph::GraphArena {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes,
        edges,
    })
    .unwrap();
    layer_network_simplex(&mut graph).unwrap();
    split_long_edges(&mut graph);
    process_port_sides(&mut graph, crate::graph::OtherGraphs::none());
    sort_port_lists(&mut graph);
    graph
}

#[test]
fn complete_dag_randomizes_after_the_shared_initial_order_attempt() {
    // Pinned elkjs 0.9.3, seed 1: both property names have the same ID. Giving
    // SECOND_TRY a distinct ID instead leaves this graph at four crossings.
    for hierarchical in [false, true] {
        let nodes = (0..6).map(|i| node(&format!("n{i}"))).collect();
        let mut edges = Vec::new();
        for source in 0..6 {
            for target in source + 1..6 {
                edges.push(edge(
                    &format!("e{source}{target}"),
                    &format!("n{source}"),
                    &format!("n{target}"),
                ));
            }
        }
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".into(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes,
            edges,
        })
        .unwrap();
        // Run every processor the schedule has before the sweep, as the full runner does.
        crate::pipeline::run_processors_until(
            &mut graph,
            crate::pipeline::Processor::Intermediate(
                crate::pipeline::IntermediateProcessor::SortByInputModelProcessor,
            ),
        )
        .unwrap();

        if hierarchical {
            minimize_crossings_layer_sweep(
                &mut graph,
                GraphId::ROOT,
                true,
                CrossMinType::Barycenter,
            );
        } else {
            sweep_alone(&mut graph, CrossMinType::Barycenter);
        }

        assert_eq!(CrossingsCounter::default().count_all_crossings(&graph), 3.0);
        let layer_order = graph.layers()[2]
            .nodes()
            .iter()
            .map(|&index| {
                let node = &graph.layerless_nodes[index];
                if node.kind() == LNodeKind::Normal {
                    node.id.as_str()
                } else {
                    let edge = node
                        .ports
                        .iter()
                        .flat_map(|port| port.outgoing_edges())
                        .next()
                        .unwrap();
                    graph.edges[*edge].id.as_str()
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            layer_order,
            ["e03", "e15", "e13", "e14", "n2", "e04", "e05"]
        );
    }
}

// Mutation: hand the random stream back to the root only (`if index == 0`). The stream of the
// nested graph is then not advanced.
#[test]
fn hierarchical_sweep_advances_graph_random_streams_without_order_changes() {
    for cross_min_type in [
        CrossMinType::Barycenter,
        CrossMinType::OneSidedGreedySwitch,
        CrossMinType::TwoSidedGreedySwitch,
    ] {
        let mut root = LGraph::new("root", LayeredOptions::default());
        root.options.consider_model_order_strategy = OrderingStrategy::NodesAndEdges;
        root.random = JavaRandom::new(17);
        let mut child = LGraph::new("child", LayeredOptions::default());
        child.random = JavaRandom::new(31);
        let parent = LNode::new("parent", 10.0, 10.0, None);
        root.layerless_nodes.push(parent);
        root.set_node_layer(NodeId::new(0), 0);
        let mut graphs = GraphArena::new(root);
        let child_id = graphs.nest(GraphId::ROOT, NodeId::new(0), child);

        // Source initialize() draws nextLong from the root. GraphInfoHolder draws
        // a distributor boolean from each graph except for two-sided greedy switch.
        // Barycenter then resets only the root stream before its first sweep; the
        // zero-crossing early return still consumes the sweep-direction boolean.
        let mut expected_root = JavaRandom::new(17);
        let seed = expected_root.next_long();
        let mut expected_child = JavaRandom::new(31);
        if cross_min_type != CrossMinType::TwoSidedGreedySwitch {
            expected_root.next_bool();
            expected_child.next_bool();
        }
        if cross_min_type == CrossMinType::Barycenter {
            expected_root.set_seed(seed);
        }
        expected_root.next_bool();

        minimize_crossings_layer_sweep(&mut graphs, GraphId::ROOT, true, cross_min_type);
        assert_eq!(graphs.root().layers()[0].nodes(), vec![NodeId::new(0)]);
        assert_eq!(graphs.root().random, expected_root, "{cross_min_type:?}");
        assert_eq!(
            graphs[child_id].random, expected_child,
            "{cross_min_type:?}",
        );
    }
}

#[test]
fn hierarchical_sweep_matches_source_node_port_counter_gate() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("A", 10.0, 10.0, None));
    graph.set_node_layer(NodeId::new(0), 0);
    graph
        .options
        .consider_model_order_crossing_counter_node_influence = 0.0;
    graph
        .options
        .consider_model_order_crossing_counter_port_influence = 1.0;

    let mut graphs = GraphArena::new(graph);
    let sweep = HierarchySweep::new(&mut graphs, GraphId::ROOT, true, CrossMinType::Barycenter);

    assert!(!sweep.use_node_port_order_crossing_counter(&graphs, 0));

    graphs
        .root_mut()
        .options
        .consider_model_order_crossing_counter_node_influence = 1.0;

    assert!(sweep.use_node_port_order_crossing_counter(&graphs, 0));
}

// Mutation: list the child graphs in node order, not in layer order: sort `children` by node
// index in `HierarchySweep::new`.
#[test]
fn hierarchical_sweep_discovers_child_graphs_in_current_node_order() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("second", 10.0, 10.0, None));
    graph
        .layerless_nodes
        .push(LNode::new("first", 10.0, 10.0, None));
    graph.set_layer_nodes(0, vec![NodeId::new(1), NodeId::new(0)]);
    let mut graphs = GraphArena::new(graph);
    let second = graphs.nest(
        GraphId::ROOT,
        NodeId::new(0),
        LGraph::new("second:nested", LayeredOptions::default()),
    );
    let first = graphs.nest(
        GraphId::ROOT,
        NodeId::new(1),
        LGraph::new("first:nested", LayeredOptions::default()),
    );

    let sweep = HierarchySweep::new(&mut graphs, GraphId::ROOT, true, CrossMinType::Barycenter);

    // The root lists node 1 before node 0, so its nested graph comes first.
    assert_eq!(sweep.infos[0].child_graphs, vec![first, second]);

    // Mutation: let `initialize_graphs` collect the nested graphs without `include_children`.
    // A flat layout has laid them out already, so its sweep holds the root alone.
    let flat = HierarchySweep::new(&mut graphs, GraphId::ROOT, false, CrossMinType::Barycenter);
    assert_eq!(flat.infos.len(), 1);
    assert!(flat.infos[0].child_graphs.is_empty());
}

#[test]
fn layer_sweep_type_decider_uses_origin_port_side_for_external_dummy() {
    let mut parent_graph = LGraph::new("root", LayeredOptions::default());
    parent_graph
        .layerless_nodes
        .push(LNode::new("parent", 10.0, 10.0, None));
    let origin = parent_graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .unwrap();

    let mut child_graph = LGraph::new("child", LayeredOptions::default());
    let mut external = LNode::new("external:parent", 0.0, 0.0, None);
    external.set_kind(LNodeKind::ExternalPort);
    external.external_port_mut().unwrap().side = PortSide::West;
    external.external_port_mut().unwrap().origin_port = Some(crate::graph::GraphPortRef {
        graph_id: parent_graph.id,
        port: origin,
    });
    child_graph.layerless_nodes.push(external);

    assert_eq!(
        external_dummy_origin_side(&child_graph, Some(&parent_graph), NodeId::new(0)),
        PortSide::East
    );
}

#[test]
fn hierarchical_sweep_saves_currently_best_snapshot_not_current_order() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(LNode::new("A", 10.0, 10.0, None));
    graph
        .layerless_nodes
        .push(LNode::new("B", 10.0, 10.0, None));
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);

    let mut graphs = GraphArena::new(graph);
    let mut sweep = HierarchySweep::new(&mut graphs, GraphId::ROOT, true, CrossMinType::Barycenter);
    sweep.changed.insert(0);
    sweep.set_currently_best_node_orders(&graphs);
    sweep.infos[0].current_node_order[0] = vec![NodeId::new(1), NodeId::new(0)];

    sweep.save_all_node_orders_of_changed_graphs(&graphs);

    assert_eq!(
        sweep.infos[0]
            .best_node_and_port_order
            .as_ref()
            .map(|copy| copy.node_order.as_slice()),
        Some([vec![NodeId::new(0), NodeId::new(1)]].as_slice())
    );
}

#[test]
fn barycenter_orders_free_layer_by_fixed_layer_port_ranks() {
    let mut graph = prepared_graph(
        vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
        vec![
            edge("Top-Right", "Top", "Right"),
            edge("Bottom-Left", "Bottom", "Left"),
        ],
    );
    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);
    initialize_crossing_minimization_port_ids(&mut graph);

    let mut order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let port_distributor =
        BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

    heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

    assert_eq!(order[1], vec![right, left]);
}

#[test]
fn forced_model_order_keeps_real_nodes_in_input_order() {
    let mut graph = prepared_graph(
        vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
        vec![
            edge("Top-Right", "Top", "Right"),
            edge("Bottom-Left", "Bottom", "Left"),
        ],
    );
    graph.options.force_node_model_order = true;
    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);
    initialize_crossing_minimization_port_ids(&mut graph);

    let mut order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let port_distributor =
        BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

    heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

    assert_eq!(order[1], vec![left, right]);
}

#[test]
fn forced_model_order_still_places_dummy_nodes_by_barycenter() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions {
            force_node_model_order: true,
            ..LayeredOptions::default()
        },
    );
    let mut first = LNode::new("first", 10.0, 10.0, None);
    first.model_order = Some(0);
    let mut dummy = LNode::new("dummy", 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::LongEdge);
    let mut second = LNode::new("second", 10.0, 10.0, None);
    second.model_order = Some(1);
    graph.layerless_nodes = vec![first, dummy, second].into();
    graph.set_node_layer(NodeId::new(0), 0);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(2), 0);
    let order = vec![vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)]];
    let port_distributor =
        BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);
    heuristic.state_mut(NodeId::new(0)).barycenter = Some(2.0);
    heuristic.state_mut(NodeId::new(1)).barycenter = Some(0.0);
    heuristic.state_mut(NodeId::new(2)).barycenter = Some(1.0);
    let mut layer = order[0].clone();

    heuristic.insertion_sort_by_model_order(&graph, &mut layer);

    assert_eq!(layer, vec![NodeId::new(1), NodeId::new(0), NodeId::new(2)]);
}

#[test]
fn barycenter_port_ranks_keep_same_id_ports_distinct() {
    let mut graph = prepared_graph(
        vec![node("Top"), node("Bottom"), node("Free")],
        vec![
            edge("Top-Free", "Top", "Free"),
            edge("Bottom-Free", "Bottom", "Free"),
        ],
    );
    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let free = graph.node_named("Free");
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![free]);
    for port in graph.layerless_nodes[top].ports.slots_mut() {
        port.id = "shared".to_string();
    }
    for port in graph.layerless_nodes[bottom].ports.slots_mut() {
        port.id = "shared".to_string();
    }
    initialize_crossing_minimization_port_ids(&mut graph);

    let order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let mut distributor = BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    distributor.calculate_port_ranks(&graph, &order[0], PortType::Output);
    distributor.distribute_ports(&mut graph, free, PortSide::West, &order);

    let free_ports = graph.layerless_nodes[free]
        .ports
        .iter()
        .map(|port| {
            port.incoming_edges()
                .first()
                .map(|edge| graph.edges[*edge].id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(free_ports, vec![Some("Bottom-Free"), Some("Top-Free")]);
}

#[test]
fn barycenter_port_ranks_persist_between_rank_calculations() {
    let mut graph = prepared_graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    );
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    graph.set_layer_nodes(0, vec![a]);
    graph.set_layer_nodes(1, vec![b]);
    initialize_crossing_minimization_port_ids(&mut graph);
    let a_out = graph.layerless_nodes[a]
        .ports
        .iter()
        .position(|port| !port.outgoing_edges().is_empty())
        .unwrap();
    let mut distributor =
        BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &[vec![a], vec![b]]);

    distributor.calculate_port_ranks(&graph, &[a], PortType::Output);
    let before = distributor.rank_of(
        &graph,
        PortRef {
            node: a,
            port: PortId::new(a_out),
        },
    );
    distributor.calculate_port_ranks(&graph, &[b], PortType::Input);
    let after = distributor.rank_of(
        &graph,
        PortRef {
            node: a,
            port: PortId::new(a_out),
        },
    );

    assert_eq!(before, after);
    assert_ne!(after, 0.0);
}

#[test]
fn layer_sweep_keeps_best_crossing_order() {
    let mut graph = prepared_graph(
        vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
        vec![
            edge("Top-Right", "Top", "Right"),
            edge("Bottom-Left", "Bottom", "Left"),
        ],
    );
    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);

    let before = CrossingsCounter::default().count_all_crossings(&graph);
    assert!(sweep_alone(&mut graph, CrossMinType::Barycenter));
    let after = CrossingsCounter::default().count_all_crossings(&graph);

    assert_eq!(before, 1.0);
    assert_eq!(after, 0.0);
}

#[test]
fn parent_hierarchical_port_order_only_rewrites_sweep_side() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
    let west_a = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .unwrap()
        .port;
    let east_a = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .unwrap()
        .port;
    let west_b = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .unwrap()
        .port;
    let east_b = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .unwrap()
        .port;
    graph.layerless_nodes[NodeId::new(0)].ports[west_a].id = "west_a".to_string();
    graph.layerless_nodes[NodeId::new(0)].ports[east_a].id = "east_a".to_string();
    graph.layerless_nodes[NodeId::new(0)].ports[west_b].id = "west_b".to_string();
    graph.layerless_nodes[NodeId::new(0)].ports[east_b].id = "east_b".to_string();
    for port in graph.layerless_nodes[NodeId::new(0)].ports.slots_mut() {
        port.inside_connections = true;
    }

    apply_parent_hierarchical_port_order_on_sweep_side(
        &mut graph,
        NodeId::new(0),
        true,
        &[east_b, east_a],
    );

    let port_ids = graph.layerless_nodes[NodeId::new(0)]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    let port_sides = graph.layerless_nodes[NodeId::new(0)]
        .ports
        .iter()
        .map(|port| port.side)
        .collect::<Vec<_>>();
    assert_eq!(port_ids, vec!["west_a", "east_b", "west_b", "east_a"]);
    assert_eq!(
        port_sides,
        vec![
            PortSide::West,
            PortSide::East,
            PortSide::West,
            PortSide::East
        ]
    );
}

// Mutation: swap the ends of the edges when the ports change places, as the old index remap did.
#[test]
fn parent_hierarchical_port_order_leaves_edge_references_alone() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
    let _west_a = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .unwrap()
        .port;
    let east_a = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .unwrap()
        .port;
    let _west_b = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .unwrap()
        .port;
    let east_b = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .unwrap()
        .port;
    for port in graph.layerless_nodes[NodeId::new(0)].ports.slots_mut() {
        port.inside_connections = true;
    }
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("target", 10.0, 10.0, None));
    let target_port = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .unwrap()
        .port;

    let edge_a = graph
        .add_edge({
            let mut built_edge = crate::graph::LayeredEdge::new(
                "edge-a".to_string(),
                PortRef {
                    node: NodeId::new(0),
                    port: east_a,
                },
                PortRef {
                    node: NodeId::new(1),
                    port: target_port,
                },
            );
            built_edge.source_node_id = "parent".to_string();
            built_edge.target_node_id = "target".to_string();
            built_edge
        })
        .unwrap();
    let edge_b = graph
        .add_edge({
            let mut built_edge = crate::graph::LayeredEdge::new(
                "edge-b".to_string(),
                PortRef {
                    node: NodeId::new(0),
                    port: east_b,
                },
                PortRef {
                    node: NodeId::new(1),
                    port: target_port,
                },
            );
            built_edge.source_node_id = "parent".to_string();
            built_edge.target_node_id = "target".to_string();
            built_edge
        })
        .unwrap();

    apply_parent_hierarchical_port_order_on_sweep_side(
        &mut graph,
        NodeId::new(0),
        true,
        &[east_b, east_a],
    );

    // The ports moved in the list. The edges still start at the ports they started at.
    assert_eq!(graph.edges[edge_a].source().port, east_a);
    assert_eq!(graph.edges[edge_b].source().port, east_b);
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .map(|port| port.id.as_str())
            .collect::<Vec<_>>(),
        vec!["parent:0", "parent:3", "parent:2", "parent:1"]
    );
    assert_eq!(
        graph.layerless_nodes[NodeId::new(0)]
            .ports
            .iter()
            .enumerate()
            .map(|(index, port)| (index, port.id.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, "parent:0"),
            (1, "parent:3"),
            (2, "parent:2"),
            (3, "parent:1"),
        ]
    );
}

#[test]
fn north_south_hierarchical_ports_use_source_barycenter_keys() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("parent", 80.0, 40.0, None));
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("input_dummy", 0.0, 0.0, None));
    graph.layerless_nodes[NodeId::new(1)].set_kind(LNodeKind::ExternalPort);
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("output_dummy", 0.0, 0.0, None));
    graph.layerless_nodes[NodeId::new(2)].set_kind(LNodeKind::ExternalPort);
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("both_dummy", 0.0, 0.0, None));
    graph.layerless_nodes[NodeId::new(3)].set_kind(LNodeKind::ExternalPort);
    graph.set_node_layer(NodeId::new(1), 0);
    graph.set_node_layer(NodeId::new(2), 0);
    graph.set_node_layer(NodeId::new(3), 0);
    graph.set_layer_nodes(0, vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]);

    let north_input = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::North,
            Default::default(),
        )
        .unwrap();
    let south_input = graph
        .add_port(
            NodeId::new(0),
            PortType::Input,
            PortSide::South,
            Default::default(),
        )
        .unwrap();
    let north_output = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::North,
            Default::default(),
        )
        .unwrap();
    let south_both = graph
        .add_port(
            NodeId::new(0),
            PortType::Output,
            PortSide::South,
            Default::default(),
        )
        .unwrap();
    for (port, dummy) in [
        (north_input, NodeId::new(1)),
        (south_input, NodeId::new(1)),
        (north_output, NodeId::new(2)),
        (south_both, NodeId::new(3)),
    ] {
        graph.layerless_nodes[NodeId::new(0)].ports[port.port].port_dummy =
            Some(crate::graph::GraphNodeRef {
                graph_id: GraphId::ROOT,
                node: dummy,
            });
    }

    let in_dummy_port = graph
        .add_port(
            NodeId::new(1),
            PortType::Input,
            PortSide::South,
            Default::default(),
        )
        .unwrap();
    let out_dummy_port = graph
        .add_port(
            NodeId::new(2),
            PortType::Output,
            PortSide::South,
            Default::default(),
        )
        .unwrap();
    let both_in_dummy_port = graph
        .add_port(
            NodeId::new(3),
            PortType::Input,
            PortSide::South,
            Default::default(),
        )
        .unwrap();
    let both_out_dummy_port = graph
        .add_port(
            NodeId::new(3),
            PortType::Output,
            PortSide::South,
            Default::default(),
        )
        .unwrap();

    graph
        .add_edge({
            let mut built_edge =
                crate::graph::LayeredEdge::new("north-in".to_string(), north_input, in_dummy_port);
            built_edge.source_node_id = "parent".to_string();
            built_edge.target_node_id = "input_dummy".to_string();
            built_edge
        })
        .unwrap();
    graph
        .add_edge({
            let mut built_edge =
                crate::graph::LayeredEdge::new("out".to_string(), out_dummy_port, north_output);
            built_edge.source_node_id = "output_dummy".to_string();
            built_edge.target_node_id = "parent".to_string();
            built_edge
        })
        .unwrap();
    graph
        .add_edge({
            let mut built_edge = crate::graph::LayeredEdge::new(
                "both-in".to_string(),
                south_both,
                both_in_dummy_port,
            );
            built_edge.source_node_id = "parent".to_string();
            built_edge.target_node_id = "both_dummy".to_string();
            built_edge
        })
        .unwrap();
    graph
        .add_edge({
            let mut built_edge = crate::graph::LayeredEdge::new(
                "both-out".to_string(),
                both_out_dummy_port,
                south_both,
            );
            built_edge.source_node_id = "both_dummy".to_string();
            built_edge.target_node_id = "parent".to_string();
            built_edge
        })
        .unwrap();

    let node_positions = graph.layers()[0]
        .nodes()
        .iter()
        .copied()
        .enumerate()
        .map(|(position, node)| (node, position))
        .collect::<HashMap<_, _>>();
    assert_eq!(
        north_south_port_barycenter_sum(&graph, &node_positions, north_input, NodeId::new(1), 7.0),
        Some(-0.0)
    );
    assert_eq!(
        north_south_port_barycenter_sum(&graph, &node_positions, south_input, NodeId::new(1), 7.0),
        Some(7.0)
    );
    assert_eq!(
        north_south_port_barycenter_sum(&graph, &node_positions, north_output, NodeId::new(2), 7.0),
        Some(2.0)
    );
    assert_eq!(
        north_south_port_barycenter_sum(&graph, &node_positions, south_both, NodeId::new(3), 7.0),
        Some(3.5)
    );

    let current_node_positions = [
        (NodeId::new(3), 0usize),
        (NodeId::new(2), 1),
        (NodeId::new(1), 2),
    ]
    .into_iter()
    .collect::<HashMap<_, _>>();
    assert_eq!(
        north_south_port_barycenter_sum(
            &graph,
            &current_node_positions,
            south_input,
            NodeId::new(1),
            7.0
        ),
        Some(5.0)
    );
}

// Mutation: drop the port distribution after each free layer in
// `HierarchySweep::sweep_reducing_crossings_barycenter`.
#[test]
fn barycenter_sweep_distributes_free_and_fixed_layer_ports() {
    let mut graph = prepared_graph(
        vec![
            node("Top"),
            node("Bottom"),
            node("Left"),
            node("Right"),
            node("Free"),
            node("Fixed"),
        ],
        vec![
            edge("Top-Free", "Top", "Free"),
            edge("Bottom-Free", "Bottom", "Free"),
            edge("Free-Right", "Free", "Right"),
            edge("Free-Left", "Free", "Left"),
            edge("Fixed-Right", "Fixed", "Right"),
            edge("Fixed-Left", "Fixed", "Left"),
        ],
    );

    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    let free = graph.node_named("Free");
    let fixed = graph.node_named("Fixed");

    graph.set_layer_nodes(0, vec![top, bottom, fixed]);
    graph.set_layer_nodes(1, vec![free]);
    graph.set_layer_nodes(2, vec![right, left]);
    initialize_crossing_minimization_port_ids(&mut graph);

    // Seed 2 makes `ISweepPortDistributor.create` pick the node-relative distributor.
    graph.random = JavaRandom::new(2);
    let mut graphs = graph;
    let mut sweep =
        HierarchySweep::new(&mut graphs, GraphId::ROOT, false, CrossMinType::Barycenter);
    // The sweep draws the distributor kind from the graph's seed. This test is about the
    // node-relative distributor, so it checks that the seed picks it.
    assert_eq!(
        sweep.infos[0]
            .barycenter_heuristic
            .as_ref()
            .map(|heuristic| heuristic.port_distributor.kind),
        Some(PortDistributorKind::NodeRelative)
    );
    sweep.sweep_reducing_crossings_barycenter(&mut graphs, 0, true, false, false);
    let graph = graphs.root();

    let free_ports = graph.layerless_nodes[free]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();
    let fixed_ports = graph.layerless_nodes[fixed]
        .ports
        .iter()
        .map(|port| port.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(free_ports, vec!["Free:2", "Free:3", "Free:1", "Free:0"]);
    assert_eq!(fixed_ports, vec!["Fixed:0", "Fixed:1"]);
}

#[test]
fn hierarchical_sweep_orders_nested_sources_by_child_boundary_ports() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![
            nested_group("O", None),
            nested_group("A", Some("O")),
            nested_group("B", Some("A")),
            nested_node("b", Some("A")),
            nested_node("a", Some("A")),
            nested_node("c", Some("B")),
        ],
        edges: vec![edge("L_b_B_0", "b", "B"), edge("L_a_c_0", "a", "c")],
    })
    .unwrap();
    graph.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

    crate::compound::preprocess_source_ported_compound_graph(&mut graph);
    crate::configurator::configure_graph_properties(&mut graph).unwrap();

    crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();

    // The layout is top to bottom, so the order inside a layer is the order along x.
    let o = &graph[graph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];
    let a = &graph[o.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];
    let x_of = |id: &str| {
        a.layerless_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap()
            .position
            .x
    };
    assert!(x_of("a") < x_of("b"));
}

#[test]
fn hierarchical_barycenter_keeps_current_order_between_randomized_tries() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![
            nested_group("bar", None),
            nested_group("foo", None),
            nested_node("A", None),
            nested_node("B", None),
            nested_node("G", None),
            nested_node("E", Some("bar")),
            nested_node("F", Some("bar")),
            nested_node("C", Some("foo")),
            nested_node("D", Some("foo")),
        ],
        edges: vec![
            edge("L_A_B_0", "A", "B"),
            edge("L_B_C_0", "B", "C"),
            edge("L_C_D_0", "C", "D"),
            edge("L_B_D_0", "B", "D"),
            edge("L_D_E_0", "D", "E"),
            edge("L_E_A_0", "E", "A"),
            edge("L_E_F_0", "E", "F"),
            edge("L_F_D_0", "F", "D"),
            edge("L_F_G_0", "F", "G"),
            edge("L_B_G_0", "B", "G"),
            edge("L_G_D_0", "G", "D"),
        ],
    })
    .unwrap();
    graph.options.hierarchy_handling = HierarchyHandling::IncludeChildren;

    crate::pipeline::execute_ported_compound_processors(&mut graph).unwrap();

    // Pinned elkjs 0.9.3: the inverted L_D_E port dummy comes before both groups. The old
    // test read the layers after the sweep: [dummy, bar, B] and [dummy, foo]. The layout is
    // top to bottom, so the order inside a layer is the order along x. Assigning SECOND_TRY a
    // distinct Property ID instead gives [bar, B, dummy] and [foo, dummy]. The dummies stay in
    // the node list after the long edge joiner and keep the x their layer gave them.
    let find = |id: &str| {
        graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap()
    };
    let (bar, foo, b) = (find("bar"), find("foo"), find("B"));
    let mut dummies = graph
        .layerless_nodes
        .iter()
        .filter(|node| node.id.starts_with("invertedPort:"))
        .collect::<Vec<_>>();
    assert_eq!(dummies.len(), 2);
    dummies.sort_by(|a, b| a.position.y.total_cmp(&b.position.y));
    let (first_dummy, last_dummy) = (dummies[0], dummies[1]);
    // Layers run along y. A node is in the layer of the group whose vertical extent holds it.
    let spans = |group: &LNode, other: &LNode| {
        group.position.y <= other.position.y
            && other.position.y <= group.position.y + group.size.height
    };
    assert!(spans(bar, first_dummy) && spans(foo, last_dummy) && spans(bar, b));
    assert!(
        first_dummy.position.x < bar.position.x && bar.position.x < b.position.x,
        "first layer along x: dummy {}, bar {}, B {}",
        first_dummy.position.x,
        bar.position.x,
        b.position.x
    );
    assert!(
        last_dummy.position.x < foo.position.x,
        "last layer along x: dummy {}, foo {}",
        last_dummy.position.x,
        foo.position.x
    );
}

#[test]
fn forster_constraints_restore_successor_order_after_barycenter_sort() {
    let mut graph = prepared_graph(
        vec![node("A"), node("B"), node("C")],
        vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
    );
    let a = graph.node_named("A");
    let b = graph.node_named("B");
    let c = graph.node_named("C");
    graph.set_layer_nodes(0, vec![c]);
    graph.set_layer_nodes(1, vec![b, a]);
    graph
        .tables
        .in_layer
        .row_mut(a)
        .successor_constraints
        .push(b);

    let mut order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let port_distributor =
        BarycenterPortDistributor::new(PortDistributorKind::NodeRelative, &order);
    let mut heuristic = BarycenterHeuristic::new(&graph, JavaRandom::new(1), port_distributor);

    heuristic.minimize_crossings(&graph, &mut order, 1, true, false);

    assert_eq!(order[1], vec![a, b]);
}

#[test]
fn forster_single_node_groups_merge_by_current_barycenters() {
    let mut states = HashMap::new();
    states.insert(
        NodeId::new(0),
        BarycenterState {
            summed_weight: 9.0,
            degree: 3,
            barycenter: Some(3.0),
            visited: false,
        },
    );
    states.insert(
        NodeId::new(1),
        BarycenterState {
            summed_weight: 1.0,
            degree: 1,
            barycenter: Some(1.0),
            visited: false,
        },
    );
    let first = ConstraintGroup::new(NodeId::new(0));
    let second = ConstraintGroup::new(NodeId::new(1));

    let merged = merge_constraint_groups(&mut states, &first, &second);

    assert_eq!(merged.barycenter(&states), Some(2.0));
    assert_eq!(states[&NodeId::new(0)].barycenter, Some(2.0));
    assert_eq!(states[&NodeId::new(1)].barycenter, Some(2.0));
}

#[test]
fn two_sided_greedy_switch_orders_layer_by_both_neighbors() {
    let mut graph = prepared_graph(
        vec![
            node("LeftTop"),
            node("LeftBottom"),
            node("MiddleTop"),
            node("MiddleBottom"),
            node("RightTop"),
            node("RightBottom"),
        ],
        vec![
            edge("LeftTop-MiddleBottom", "LeftTop", "MiddleBottom"),
            edge("LeftBottom-MiddleTop", "LeftBottom", "MiddleTop"),
            edge("MiddleTop-RightBottom", "MiddleTop", "RightBottom"),
            edge("MiddleBottom-RightTop", "MiddleBottom", "RightTop"),
        ],
    );
    let left_top = graph.node_named("LeftTop");
    let left_bottom = graph.node_named("LeftBottom");
    let middle_top = graph.node_named("MiddleTop");
    let middle_bottom = graph.node_named("MiddleBottom");
    let right_top = graph.node_named("RightTop");
    let right_bottom = graph.node_named("RightBottom");
    graph.set_layer_nodes(0, vec![left_top, left_bottom]);
    graph.set_layer_nodes(1, vec![middle_top, middle_bottom]);
    graph.set_layer_nodes(2, vec![right_top, right_bottom]);

    let before = CrossingsCounter::default().count_all_crossings(&graph);
    let mut order = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().to_vec())
        .collect::<Vec<_>>();
    let mut heuristic = GreedySwitchHeuristic::new(CrossMinType::TwoSidedGreedySwitch);

    assert!(heuristic.minimize_crossings(&graph, &mut order, 1, true));
    assert_eq!(order[1], vec![middle_bottom, middle_top]);

    graph.set_layer_nodes(1, order[1].clone());
    let after = CrossingsCounter::default().count_all_crossings(&graph);

    assert_eq!(before, 2.0);
    assert_eq!(after, 0.0);
}

#[test]
fn one_sided_greedy_switch_reduces_between_layer_crossings() {
    let mut graph = prepared_graph(
        vec![node("Top"), node("Bottom"), node("Left"), node("Right")],
        vec![
            edge("Top-Right", "Top", "Right"),
            edge("Bottom-Left", "Bottom", "Left"),
        ],
    );
    let top = graph.node_named("Top");
    let bottom = graph.node_named("Bottom");
    let left = graph.node_named("Left");
    let right = graph.node_named("Right");
    graph.set_layer_nodes(0, vec![top, bottom]);
    graph.set_layer_nodes(1, vec![left, right]);

    let before = CrossingsCounter::default().count_all_crossings(&graph);
    sweep_alone(&mut graph, CrossMinType::OneSidedGreedySwitch);
    let after = CrossingsCounter::default().count_all_crossings(&graph);

    assert_eq!(before, 1.0);
    assert_eq!(after, 0.0);
}

// Mutation: compare the two barycenters as `f32`, as Java on a JVM does.
#[test]
fn barycenters_that_differ_only_beyond_float_precision_are_not_equal() {
    use crate::phase::ordering::barycenter_heuristic::BarycenterState;
    use crate::phase::ordering::forster_constraint_resolver::{
        ConstraintGroup, add_forster_constraint, find_violated_forster_constraint,
    };
    // Group 1 must stay above group 0. Its barycenter is a little smaller, so the constraint
    // holds. As `f32`, the two are equal, and the later group 1 would count as a violation.
    let mut groups = vec![
        ConstraintGroup::new(NodeId::new(0)),
        ConstraintGroup::new(NodeId::new(1)),
    ];
    add_forster_constraint(&mut groups, 1, 0);
    let state = |barycenter: f64| BarycenterState {
        barycenter: Some(barycenter),
        ..BarycenterState::default()
    };
    let mut states = HashMap::new();
    states.insert(NodeId::new(0), state(1.0 + 1e-12));
    states.insert(NodeId::new(1), state(1.0));
    assert_eq!((1.0_f64 + 1e-12) as f32, 1.0_f32);
    assert_eq!(find_violated_forster_constraint(&states, &mut groups), None);

    // The same barycenter is a tie, and the tie goes against the later group.
    states.insert(NodeId::new(0), state(1.0));
    assert_eq!(
        find_violated_forster_constraint(&states, &mut groups),
        Some((1, 0))
    );
}

/// Tests of the parts of the greedy switch that deal with north and south port dummies.
mod north_south_tests;

/// Tests of `GreedyPortDistributor`.
mod greedy_port_distributor_tests;
