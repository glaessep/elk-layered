use super::assembler::{
    CrossingMinimizer, CycleBreaker, EdgeRouter, IntermediateProcessor, Layerer, NodePlacer, Place,
    Strategies, assemble, assemble_graph, hypernodes_refusal,
};
use super::compound::{CompoundRun, check_strategies, execute_compound_graph_round};
use super::execute::{execute_in_arena, execute_processor, resize_layered_node};
use super::*;
use crate::components::ComponentSplit;
use crate::compound::preprocess_source_ported_compound_graph;
use crate::graph::EdgeId;
use crate::graph::PortId;
use crate::graph::{GraphParent, GraphProperties, LPoint, NodeId, OtherGraphs};
use crate::graph::{LLabel, LNode, LNodeKind, LayeredEdge, PortRef, PortSide, PortType};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, import_graph};
use crate::intermediate::hierarchical_node_resizing_processor::{
    resize_graph_no_really_i_mean_it, resize_graph_to_minimum,
};
use crate::options::ContentAlignment;
use crate::options::{
    CrossingMinimizationStrategy, CycleBreakingStrategy, EdgeRouting, ElkDirection, FixedAlignment,
    GreedySwitchType, LayeredOptions, LayeringStrategy, NodePlacementStrategy, OrderingStrategy,
    PortConstraints, WrappingStrategy,
};

fn compound_run(graphs: &GraphArena) -> CompoundRun {
    CompoundRun::new(graphs).unwrap()
}

fn try_kinds(options: &LayeredOptions) -> PipelineResult<Vec<Processor>> {
    let properties = GraphProperties::default();
    let strategies = Strategies::select(options, &properties)?;
    Ok(assemble(strategies, options, &properties, 0, true))
}

fn kinds(options: &LayeredOptions) -> Vec<Processor> {
    try_kinds(options).expect("the options need only ported processors")
}

fn try_graph_kinds(graph: &LGraph) -> PipelineResult<Vec<Processor>> {
    assemble_graph(graph)
}

fn graph_kinds(graph: &LGraph) -> Vec<Processor> {
    try_graph_kinds(graph).expect("the graph needs only ported processors")
}

fn assert_processors_are_source_ported(case: &str, assembled: PipelineResult<Vec<Processor>>) {
    assert!(
        assembled.is_ok(),
        "{case} reached an unported ELK processor: {assembled:?}"
    );
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

// Mutation: move only the root into a new one-graph arena inside `run_processors_until_processor`,
// and put it back afterwards. The graph inside `group` is then dropped from the arena the
// test passes in.
#[test]
fn running_processors_until_a_phase_keeps_the_graphs_inside_nodes() {
    let mut group = node("group");
    group.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child = node("child");
    child.parent = Some("group".to_string());
    let input = ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
            ..p3_options()
        },
        nodes: vec![group, child],
        edges: Vec::new(),
    };
    let mut graphs = import_graph(&input).unwrap();
    assert_eq!(graphs.len(), 2);

    run_processors_until_phase(&mut graphs, LayeredPhase::P1CycleBreaking).unwrap();

    assert_eq!(graphs.len(), 2);
    let nested = graphs.root().layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("group still holds its graph");
    assert_eq!(graphs[nested].layerless_nodes.len(), 1);
}

fn p3_options() -> LayeredOptions {
    LayeredOptions {
        direction: ElkDirection::Right,
        greedy_switch_type: GreedySwitchType::Off,
        ..LayeredOptions::default()
    }
}

/// The processors the assembler schedules up to and including the end of `phase`.
fn kinds_through_phase(options: &LayeredOptions, phase: LayeredPhase) -> Vec<Processor> {
    let processors = kinds(options);
    let end = processors
        .iter()
        .position(|processor| processor.phase() == Some(phase))
        .unwrap();
    processors[..=end].to_vec()
}

fn deep_compound_chain(graph_count: usize) -> GraphArena {
    assert!(graph_count > 0);
    let options = LayeredOptions {
        hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
        greedy_switch_type: GreedySwitchType::Off,
        greedy_switch_hierarchical_type: GreedySwitchType::Off,
        ..LayeredOptions::default()
    };
    let mut graphs = GraphArena::new(LGraph::new("graph-0", options.clone()));
    let mut parent = GraphId::ROOT;
    for depth in 1..graph_count {
        let holder = LNode::new(format!("node-{}", depth - 1), 1.0, 1.0, None);
        graphs[parent].layerless_nodes.push(holder);
        let graph = LGraph::new(format!("graph-{depth}"), options.clone());
        parent = graphs.nest(parent, NodeId::new(0), graph);
    }
    graphs
}

fn branched_compound_graph() -> GraphArena {
    let options = LayeredOptions {
        hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
        greedy_switch_type: GreedySwitchType::Off,
        greedy_switch_hierarchical_type: GreedySwitchType::Off,
        ..LayeredOptions::default()
    };
    let mut graphs = GraphArena::new(LGraph::new("root", options.clone()));
    for branch in ["first", "second"] {
        let holder = LNode::new(format!("{branch}-holder"), 1.0, 1.0, None);
        let position = graphs.root().next_node_id();
        graphs.root_mut().layerless_nodes.push(holder);

        let mut child = LGraph::new(format!("{branch}-child"), options.clone());
        let grandchild_holder = LNode::new(format!("{branch}-grandchild-holder"), 1.0, 1.0, None);
        child.layerless_nodes.push(grandchild_holder);
        let child = graphs.nest(GraphId::ROOT, position, child);

        let grandchild = LGraph::new(format!("{branch}-grandchild"), options.clone());
        graphs.nest(child, NodeId::new(0), grandchild);
    }
    graphs
}

// Mutation: give each `GraphExecution` a `parent` of `None` in `graph_algorithm`. The nested graphs
// then have no parent link.
#[test]
fn compound_run_holds_one_algorithm_per_graph_and_a_parent_for_each_nested_one() {
    for graph_count in [8usize, 16, 32, 64, 128] {
        let graphs = deep_compound_chain(graph_count);
        let run = compound_run(&graphs);
        let parent_links = run
            .algorithms
            .iter()
            .filter(|algorithm| algorithm.execution.parent.is_some())
            .count();

        assert_eq!(run.algorithms.len(), graph_count);
        assert_eq!(parent_links, graph_count - 1);
        assert!(run.algorithms[0].execution.parent.is_none());
    }
}

// Mutation: build the run in pre-order (`GraphArena::preorder`) instead of ELK's discovery
// order. The grandchildren then come in another place.
#[test]
fn compound_run_preserves_official_reverse_discovery_order() {
    let graphs = branched_compound_graph();
    let run = compound_run(&graphs);
    let names = run
        .algorithms
        .iter()
        .rev()
        .map(|algorithm| algorithm.execution.graph_name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "first-grandchild",
            "second-grandchild",
            "second-child",
            "first-child",
            "root",
        ]
    );
}

// Mutation: drop the `.rev()` in `CompoundRun::into_executions`. The results then come in
// discovery order, root first.
#[test]
fn public_compound_execution_reports_official_reverse_discovery_order() {
    let mut root = branched_compound_graph();
    let executions = execute_ported_compound_processors(&mut root).unwrap();
    let names = executions
        .iter()
        .map(|execution| execution.graph_name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "first-grandchild",
            "second-grandchild",
            "second-child",
            "first-child",
            "root",
        ]
    );
}

// Mutation: drop the `?` after a nested graph's run in `execute_compound_graph_round`, so a
// failing child processor no longer fails the round.
#[test]
fn compound_round_reports_a_failing_child_processor_and_keeps_the_hierarchy() {
    let options = LayeredOptions::default();
    let mut graphs = GraphArena::new(LGraph::new("root", options.clone()));
    graphs
        .root_mut()
        .layerless_nodes
        .push(LNode::new("holder", 1.0, 1.0, None));
    let child = graphs.nest(GraphId::ROOT, NodeId::new(0), LGraph::new("child", options));
    // A minimum size that is not a number makes the resizer of the child graph fail.
    graphs[child].options.node_size = crate::options::NodeSizing::with_minimum(LSize {
        width: f64::NAN,
        height: 1.0,
    });
    let mut run = compound_run(&graphs);
    let child_index = run
        .algorithms
        .iter()
        .position(|algorithm| algorithm.execution.graph_name == "child")
        .unwrap();
    run.algorithms[child_index].processors = vec![Processor::Intermediate(
        IntermediateProcessor::HierarchicalNodeResizer,
    )];

    assert!(matches!(
        execute_compound_graph_round(&mut graphs, &mut run.algorithms),
        Err(PipelineError::NodeSize(_))
    ));
    assert_eq!(
        graphs.root().layerless_nodes[NodeId::new(0)].nested_graph,
        Some(child)
    );
    assert_eq!(graphs[child].name, "child");
}

#[test]
fn full_runner_rejects_unported_strategies_before_mutating_graph() {
    let cases = [
        (
            LayeredOptions {
                layering_strategy: LayeringStrategy::BreadthFirstModelOrder,
                ..LayeredOptions::default()
            },
            UnportedProcessor::BreadthFirstModelOrderLayerer,
        ),
        (
            LayeredOptions {
                crossing_minimization_strategy: CrossingMinimizationStrategy::Interactive,
                ..LayeredOptions::default()
            },
            UnportedProcessor::InteractiveCrossingMinimizer,
        ),
        (
            LayeredOptions {
                node_placement_strategy: NodePlacementStrategy::Interactive,
                ..LayeredOptions::default()
            },
            UnportedProcessor::InteractiveNodePlacer,
        ),
        (
            LayeredOptions {
                node_placement_strategy: NodePlacementStrategy::LinearSegments,
                ..LayeredOptions::default()
            },
            UnportedProcessor::LinearSegmentsNodePlacer,
        ),
        (
            LayeredOptions {
                edge_routing: EdgeRouting::Polyline,
                ..LayeredOptions::default()
            },
            UnportedProcessor::PolylineEdgeRouter,
        ),
        (
            LayeredOptions {
                wrapping_strategy: WrappingStrategy::SingleEdge,
                ..LayeredOptions::default()
            },
            UnportedProcessor::SingleEdgeGraphWrapper,
        ),
    ];

    for (options, expected) in cases {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".to_string(),
            options,
            nodes: vec![node("A"), node("B")],
            edges: vec![edge("A-B", "A", "B")],
        })
        .unwrap();
        let before = graph.clone();

        let result = execute_ported_processors(&mut graph);

        assert_eq!(
            result,
            Err(PipelineError::UnsupportedProcessor { kind: expected })
        );
        assert_eq!(
            graph, before,
            "{expected:?} mutated the graph before failing"
        );
    }
}

// Mutation: check the strategies of the root only in `check_strategies`. The child's processor
// is then found after the configurator has changed the graph.
#[test]
fn compound_full_runner_rejects_child_unported_processor_before_mutation() {
    let mut graph = deep_compound_chain(2);
    let child = graph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("the fixture should contain one child graph");
    graph[child].options.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;
    let before = graph.clone();

    assert_eq!(
        execute_ported_compound_processors(&mut graph),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::BreadthFirstModelOrderLayerer,
        })
    );
    assert_eq!(graph, before);
}

/// A root that the configurator would rewrite (undefined direction, edge spacing below the
/// minimum) and a child with seed 0, so a runner that configures before it checks the seed
/// leaves a visible trace on the root.
fn root_to_rewrite_with_zero_seed_child() -> GraphArena {
    let mut graph = deep_compound_chain(2);
    graph.options.direction = ElkDirection::Undefined;
    graph.options.spacing.edge_edge = 0.0;
    let child = graph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("the fixture should contain one child graph");
    graph[child].options.random_seed = 0;
    graph
}

// Mutation: leave out `check_random_seeds` in `execute_ported_processors`. The configurator then
// rewrites the root before it reaches the child with seed 0.
#[test]
fn full_runner_rejects_nested_zero_seed_without_changing_the_graph() {
    let mut graph = root_to_rewrite_with_zero_seed_child();
    let before = graph.clone();

    assert_eq!(
        execute_ported_processors(&mut graph),
        Err(PipelineError::RandomSeed(RandomSeedError::ZeroSeed {
            graph_id: "graph-1".to_string(),
        }))
    );
    assert_eq!(graph, before);
}

// Mutation: leave out `check_random_seeds` in `execute_ported_compound_processors`. The
// configurator then rewrites the root before it reaches the child with seed 0.
#[test]
fn compound_full_runner_rejects_nested_zero_seed_without_changing_the_graph() {
    let mut graph = root_to_rewrite_with_zero_seed_child();
    let before = graph.clone();

    assert_eq!(
        execute_ported_compound_processors(&mut graph),
        Err(PipelineError::RandomSeed(RandomSeedError::ZeroSeed {
            graph_id: "graph-1".to_string(),
        }))
    );
    assert_eq!(graph, before);
}

#[test]
fn full_runner_reports_first_unported_processor_in_pipeline_order() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            layering_strategy: LayeringStrategy::BreadthFirstModelOrder,
            node_placement_strategy: NodePlacementStrategy::Interactive,
            edge_routing: EdgeRouting::Splines,
            wrapping_strategy: WrappingStrategy::MultiEdge,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B")],
        edges: vec![edge("A-B", "A", "B")],
    })
    .unwrap();

    assert_eq!(
        execute_ported_processors(&mut graph),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::BreadthFirstModelOrderLayerer,
        })
    );
}

// Mutation: check the strategies of the root only in `check_strategies`. The processor of the
// deepest graph is then never found.
#[test]
fn compound_ported_processor_preflight_is_small_stack_safe() {
    const GRAPH_COUNT: usize = 4_096;

    std::thread::Builder::new()
        .name("elk-ported-processor-preflight-small-stack".to_string())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut graph = deep_compound_chain(GRAPH_COUNT);
            // The chain's graphs joined the arena one below the other, so the last is the leaf.
            let leaf = GraphId::new(GRAPH_COUNT - 1);
            graph[leaf].options.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;

            let graph = std::mem::ManuallyDrop::new(graph);
            assert_eq!(
                check_strategies(&graph),
                Err(PipelineError::UnsupportedProcessor {
                    kind: UnportedProcessor::BreadthFirstModelOrderLayerer,
                })
            );
        })
        .expect("the small-stack preflight thread should start")
        .join()
        .expect("ported processor validation must not recurse with hierarchy depth");
}

#[test]
fn layered_baseline_processor_sequence_matches_elkjs_0_9_3_logging() {
    assert_eq!(
        kinds(&LayeredOptions::default()),
        vec![
            Processor::Intermediate(IntermediateProcessor::EdgeAndLayerConstraintEdgeReverser),
            Processor::CycleBreaker(CycleBreaker::Greedy),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPreprocessor),
            Processor::Layerer(Layerer::NetworkSimplex),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPostprocessor),
            Processor::Intermediate(IntermediateProcessor::LongEdgeSplitter),
            Processor::Intermediate(IntermediateProcessor::PortSideProcessor),
            Processor::Intermediate(IntermediateProcessor::PortListSorter),
            Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep),
            Processor::Intermediate(IntermediateProcessor::TwoSidedGreedySwitch),
            Processor::Intermediate(IntermediateProcessor::InLayerConstraintProcessor),
            Processor::Intermediate(IntermediateProcessor::LabelAndNodeSizeProcessor),
            Processor::Intermediate(IntermediateProcessor::InnermostNodeMarginCalculator),
            Processor::NodePlacer(NodePlacer::BrandesKoepf),
            Processor::Intermediate(IntermediateProcessor::LayerSizeAndGraphHeightCalculator),
            Processor::EdgeRouter(EdgeRouter::Orthogonal),
            Processor::Intermediate(IntermediateProcessor::LongEdgeJoiner),
            Processor::Intermediate(IntermediateProcessor::EndLabelSorter),
            Processor::Intermediate(IntermediateProcessor::ReversedEdgeRestorer),
        ]
    );
}

#[test]
fn flowchart_test_defaults_insert_direction_model_order_and_hierarchy_processors() {
    let options = LayeredOptions::flowchart_test_defaults(ElkDirection::Down);
    assert_eq!(
        kinds(&options),
        vec![
            Processor::Intermediate(IntermediateProcessor::DirectionPreprocessor),
            Processor::Intermediate(IntermediateProcessor::EdgeAndLayerConstraintEdgeReverser),
            Processor::CycleBreaker(CycleBreaker::Greedy),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPreprocessor),
            Processor::Layerer(Layerer::NetworkSimplex),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPostprocessor),
            Processor::Intermediate(IntermediateProcessor::LongEdgeSplitter),
            Processor::Intermediate(IntermediateProcessor::PortSideProcessor),
            Processor::Intermediate(IntermediateProcessor::PortListSorter),
            Processor::Intermediate(IntermediateProcessor::SortByInputModelProcessor),
            Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep),
            Processor::Intermediate(IntermediateProcessor::InLayerConstraintProcessor),
            Processor::Intermediate(IntermediateProcessor::LabelAndNodeSizeProcessor),
            Processor::Intermediate(IntermediateProcessor::InnermostNodeMarginCalculator),
            Processor::NodePlacer(NodePlacer::BrandesKoepf),
            Processor::Intermediate(IntermediateProcessor::LayerSizeAndGraphHeightCalculator),
            Processor::EdgeRouter(EdgeRouter::Orthogonal),
            Processor::Intermediate(IntermediateProcessor::LongEdgeJoiner),
            Processor::Intermediate(IntermediateProcessor::EndLabelSorter),
            Processor::Intermediate(IntermediateProcessor::ReversedEdgeRestorer),
            Processor::Intermediate(IntermediateProcessor::HierarchicalNodeResizer),
            Processor::Intermediate(IntermediateProcessor::DirectionPostprocessor),
        ]
    );
}

// Mutation: in `Strategies::select`, refuse a graph with `wrapping_multi_edge_improve_cuts` set,
// instead of calling `wrapping_refusal(options.wrapping_strategy)`.
#[test]
fn mermaid_wrapping_flags_do_not_enable_wrapping_processors_without_strategy() {
    let options = LayeredOptions::flowchart_test_defaults(ElkDirection::Down);
    assert!(options.wrapping_multi_edge_improve_cuts);
    assert_eq!(options.wrapping_strategy, WrappingStrategy::Off);
    assert!(try_kinds(&options).is_ok());
}

// Mutation: refuse `LayeringStrategy::NetworkSimplex` in `Layerer::select`.
#[test]
fn mermaid_reachable_flowchart_elk_processors_are_source_ported() {
    for direction in [
        ElkDirection::Right,
        ElkDirection::Left,
        ElkDirection::Down,
        ElkDirection::Up,
    ] {
        assert_processors_are_source_ported(
            &format!("Mermaid flowchart direction {direction:?}"),
            try_kinds(&LayeredOptions::flowchart_test_defaults(direction)),
        );
    }

    for cycle_breaking_strategy in [
        CycleBreakingStrategy::Greedy,
        CycleBreakingStrategy::DepthFirst,
        CycleBreakingStrategy::Interactive,
        CycleBreakingStrategy::ModelOrder,
        CycleBreakingStrategy::GreedyModelOrder,
    ] {
        assert_processors_are_source_ported(
            &format!("public cycleBreakingStrategy {cycle_breaking_strategy:?}"),
            try_kinds(&LayeredOptions {
                cycle_breaking_strategy,
                ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
            }),
        );
    }

    for node_placement_strategy in [
        NodePlacementStrategy::BrandesKoepf,
        NodePlacementStrategy::Simple,
        NodePlacementStrategy::NetworkSimplex,
    ] {
        assert_processors_are_source_ported(
            &format!("public nodePlacementStrategy {node_placement_strategy:?}"),
            try_kinds(&LayeredOptions {
                node_placement_strategy,
                ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
            }),
        );
    }

    for node_placement_bk_fixed_alignment in [
        FixedAlignment::None,
        FixedAlignment::LeftUp,
        FixedAlignment::RightUp,
        FixedAlignment::LeftDown,
        FixedAlignment::RightDown,
        FixedAlignment::Balanced,
    ] {
        assert_processors_are_source_ported(
            &format!("public nodePlacementAlignment {node_placement_bk_fixed_alignment:?}"),
            try_kinds(&LayeredOptions {
                node_placement_bk_fixed_alignment,
                ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
            }),
        );
    }

    for consider_model_order_strategy in [
        OrderingStrategy::None,
        OrderingStrategy::NodesAndEdges,
        OrderingStrategy::PreferEdges,
        OrderingStrategy::PreferNodes,
    ] {
        assert_processors_are_source_ported(
            &format!("public considerModelOrder {consider_model_order_strategy:?}"),
            try_kinds(&LayeredOptions {
                consider_model_order_strategy,
                ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
            }),
        );
    }

    assert_processors_are_source_ported(
        "public forceNodeModelOrder",
        try_kinds(&LayeredOptions {
            force_node_model_order: true,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        }),
    );

    let merge_edges_graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            merge_edges: true,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        },
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![
            edge("A-C-1", "A", "C"),
            edge("A-C-2", "A", "C"),
            edge("B-C", "B", "C"),
        ],
    })
    .unwrap();
    assert_processors_are_source_ported(
        "public mergeEdges hyperedge graph",
        try_graph_kinds(&merge_edges_graph),
    );

    let mut constrained_entry = node("A");
    constrained_entry.layer_constraint = Some(crate::options::LayerConstraint::First);
    let keep_entry_graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![constrained_entry, node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    })
    .unwrap();
    assert_processors_are_source_ported(
        "public keepEntryNodeOnTop layer constraint",
        try_graph_kinds(&keep_entry_graph),
    );

    let mut head = ElkInputLabel::center("head", 20.0, 10.0);
    head.placement = crate::graph::EdgeLabelPlacement::Head;
    let mut end_label_edge = edge("A-B", "A", "B");
    end_label_edge.labels = vec![head];
    let label_and_self_loop_graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A"), node("B")],
        edges: vec![
            ElkInputEdge {
                source_port: None,
                target_port: None,
                labels: vec![ElkInputLabel::center("center", 28.0, 12.0)],
                ..edge("A-A", "A", "A")
            },
            end_label_edge,
        ],
    })
    .unwrap();
    assert_processors_are_source_ported(
        "Mermaid flowchart labels and self loops",
        try_graph_kinds(&label_and_self_loop_graph),
    );

    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut compound_graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![cluster, child, node("B")],
        edges: vec![edge("A-B", "A", "B")],
    })
    .unwrap();
    preprocess_source_ported_compound_graph(&mut compound_graph);
    assert_processors_are_source_ported(
        "Mermaid flowchart compound root graph",
        try_graph_kinds(&compound_graph),
    );
    let nested_graph = &compound_graph[compound_graph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("compound fixture should create a nested graph")];
    assert_processors_are_source_ported(
        "Mermaid flowchart compound child graph",
        try_graph_kinds(nested_graph),
    );
}

#[test]
fn greedy_model_order_cycle_breaking_strategy_assembles_processor() {
    let options = LayeredOptions {
        cycle_breaking_strategy: CycleBreakingStrategy::GreedyModelOrder,
        ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
    };
    assert!(kinds(&options).contains(&Processor::CycleBreaker(CycleBreaker::GreedyModelOrder)));
}

// Mutation: map `NodePlacementStrategy::Simple` to `NodePlacer::BrandesKoepf` in
// `NodePlacer::select`.
#[test]
fn documented_node_placement_strategies_assemble_public_processors() {
    let cases = [
        (
            NodePlacementStrategy::BrandesKoepf,
            Processor::NodePlacer(NodePlacer::BrandesKoepf),
        ),
        (
            NodePlacementStrategy::Simple,
            Processor::NodePlacer(NodePlacer::Simple),
        ),
        (
            NodePlacementStrategy::NetworkSimplex,
            Processor::NodePlacer(NodePlacer::NetworkSimplex),
        ),
    ];

    for (strategy, expected_processor) in cases {
        let options = LayeredOptions {
            node_placement_strategy: strategy,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        };

        assert!(
            kinds(&options).contains(&expected_processor),
            "{strategy:?} should assemble {expected_processor:?}"
        );
    }
}

#[test]
fn merge_edges_hyperedge_graph_assembles_dummy_merger() {
    let graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            merge_edges: true,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        },
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![
            edge("A-C-1", "A", "C"),
            edge("A-C-2", "A", "C"),
            edge("B-C", "B", "C"),
        ],
    })
    .unwrap();

    assert!(graph.options.graph_has_hyperedges);
    assert!(
        graph_kinds(&graph).contains(&Processor::Intermediate(
            IntermediateProcessor::HyperedgeDummyMerger
        )),
        "mergeEdges hyperedge graph should assemble HyperedgeDummyMerger"
    );
}

#[test]
fn graph_properties_insert_label_self_loop_and_external_port_processors() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![
            ElkInputNode {
                ports: Vec::new(),
                parent_settings: Default::default(),
                id: "cluster".to_string(),
                width: 0.0,
                height: 0.0,
                parent: None,
                direction: None,
                hierarchy_handling: Some(crate::options::HierarchyHandling::IncludeChildren),
                layer_constraint: None,
                port_alignment: None,
                port_constraints: None,
                node_label_placement: crate::options::NodeLabelPlacement::FIXED,
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
            },
            ElkInputNode {
                ports: Vec::new(),
                parent_settings: Default::default(),
                id: "A".to_string(),
                width: 80.0,
                height: 40.0,
                parent: Some("cluster".to_string()),
                direction: None,
                hierarchy_handling: None,
                layer_constraint: None,
                port_alignment: None,
                port_constraints: None,
                node_label_placement: crate::options::NodeLabelPlacement::FIXED,
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
            },
        ],
        edges: vec![
            ElkInputEdge {
                source_port: None,
                target_port: None,
                id: "cluster-A".to_string(),
                source: "cluster".to_string(),
                target: "A".to_string(),
                labels: vec![ElkInputLabel::center("inside", 24.0, 12.0)],
                minlen: 1,
                inside_self_loops_yo: false,
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
                container: None,
            },
            ElkInputEdge {
                source_port: None,
                target_port: None,
                id: "A-A".to_string(),
                source: "A".to_string(),
                target: "A".to_string(),
                labels: Vec::new(),
                minlen: 1,
                inside_self_loops_yo: false,
                model_order: None,
                priority_direction: 0,
                priority_shortness: 0,
                priority_straightness: 0,
                container: None,
            },
        ],
    })
    .unwrap();
    preprocess_source_ported_compound_graph(&mut graph);

    let nested = &graph[graph.layerless_nodes[NodeId::new(0)].nested_graph.unwrap()];
    let processors = graph_kinds(nested);
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalPortConstraintProcessor
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalPortDummySizeProcessor
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalPortPositionProcessor
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummyInserter
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummySwitcher
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummyRemover
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPreProcessor
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopRouter
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPostProcessor
    )));
}

// Mutation: read `insideSelfLoops.activate` from the graph that holds the edge, not from the node.
#[test]
fn inside_self_loops_enable_self_loop_processors_on_nested_graphs() {
    let mut input = ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![ElkInputNode {
            ports: Vec::new(),
            parent_settings: Default::default(),
            id: "A".to_string(),
            width: 80.0,
            height: 40.0,
            parent: None,
            direction: None,
            hierarchy_handling: Some(crate::options::HierarchyHandling::IncludeChildren),
            layer_constraint: None,
            port_alignment: None,
            port_constraints: None,
            node_label_placement: crate::options::NodeLabelPlacement::FIXED,
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
        }],
        edges: vec![ElkInputEdge {
            source_port: None,
            target_port: None,
            id: "A-A".to_string(),
            source: "A".to_string(),
            target: "A".to_string(),
            labels: Vec::new(),
            minlen: 1,
            inside_self_loops_yo: true,
            model_order: None,
            priority_direction: 0,
            priority_shortness: 0,
            priority_straightness: 0,
            container: None,
        }],
    };
    let own_options = LayeredOptions {
        inside_self_loops_activate: true,
        ..Default::default()
    };
    input.nodes[0].nested_options = Some(Box::new(own_options));

    let mut graph = import_graph(&input).unwrap();

    preprocess_source_ported_compound_graph(&mut graph);

    let nested = &graph[graph.layerless_nodes[NodeId::new(0)]
        .nested_graph
        .expect("inside self-loop should create a nested graph")];
    let processors = graph_kinds(nested);

    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPreProcessor
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPortRestorer
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopRouter
    )));
    assert!(processors.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPostProcessor
    )));
}

#[test]
fn hierarchical_port_orthogonal_router_runs_for_east_west_external_ports() {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions {
            port_constraints: PortConstraints::FixedPos,
            ..LayeredOptions::default()
        },
    );
    let port = push_test_external_dummy(&mut graph, "west", PortSide::West);
    graph.layerless_nodes[port]
        .external_port_mut()
        .unwrap()
        .ratio_or_position = 20.0;
    graph.set_node_layer(port, 0);

    execute_processor(
        &mut graph,
        OtherGraphs::none(),
        Processor::Intermediate(IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter),
    )
    .unwrap();

    assert_eq!(graph.layerless_nodes[port].position.y, 20.0);
}

#[test]
fn hierarchical_port_orthogonal_router_runs_for_north_south_external_ports() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let port = push_test_external_dummy(&mut graph, "north", PortSide::North);
    graph.set_node_layer(port, 0);

    execute_processor(
        &mut graph,
        OtherGraphs::none(),
        Processor::Intermediate(IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter),
    )
    .unwrap();

    assert_eq!(graph.layerless_nodes[port].position.y, 0.0);
}

/// A graph after the hierarchical port routing took replacement `r` of dummy `o` out of its
/// layer: `a` and `o` in layer 0, joined by edge `e`, and `r` in no layer. The port of `r`
/// still holds an end label of `e`.
fn graph_with_a_released_replacement() -> GraphArena {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    let a = graph.next_node_id();
    graph
        .layerless_nodes
        .push(LNode::new("a", 10.0, 10.0, None));
    let a_out = graph
        .add_port(a, PortType::Output, PortSide::East, Default::default())
        .unwrap();
    let o = push_test_external_dummy(&mut graph, "o", PortSide::North);
    let edge = graph
        .add_edge(LayeredEdge::new(
            "e".to_string(),
            a_out,
            PortRef {
                node: o,
                port: PortId::new(0),
            },
        ))
        .unwrap();
    let r = push_test_external_dummy(&mut graph, "r", PortSide::North);
    graph.layerless_nodes[r]
        .external_port_mut()
        .unwrap()
        .replaced_dummy = Some(o);
    graph.layerless_nodes[r].ports[PortId::new(0)]
        .end_labels
        .push(LLabel {
            end_label_edge: Some(edge),
            ..LLabel::new("tail", 20.0, 10.0)
        });
    graph.set_node_layer(a, 0);
    graph.set_node_layer(o, 0);
    GraphArena::new(graph)
}

// Mutation: in `execute_in_arena`, leave out the drop after the hierarchical port routing. The
// replacement then stays in the node list.
#[test]
fn no_replacement_dummy_is_left_after_the_hierarchical_port_routing() {
    let mut graphs = graph_with_a_released_replacement();

    execute_in_arena(
        &mut graphs,
        GraphId::ROOT,
        false,
        ComponentSplit::Inactive,
        Processor::Intermediate(IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter),
    )
    .unwrap();

    let graph = graphs.root();
    assert!(graph.layerless_nodes.iter().all(|node| {
        node.external_port()
            .is_none_or(|external| external.replaced_dummy.is_none())
    }));
    let names = graph
        .layerless_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["a", "o"]);
}

// Mutation: in `release_temporary_north_south_dummies`, leave out the hand-back of the end
// labels. The label then goes with the replacement.
#[test]
fn the_end_labels_of_a_dropped_replacement_go_back_to_their_edge() {
    let mut graphs = graph_with_a_released_replacement();

    execute_in_arena(
        &mut graphs,
        GraphId::ROOT,
        false,
        ComponentSplit::Inactive,
        Processor::Intermediate(IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter),
    )
    .unwrap();

    let labels = graphs.root().edges[EdgeId::new(0)]
        .labels
        .iter()
        .map(|label| label.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, ["tail"]);
}

// Mutation: in `release_temporary_north_south_dummies`, release a replacement that still has
// an edge. The drop then breaks that edge, and the routing fails.
#[test]
fn a_replacement_that_still_has_an_edge_stays() {
    let mut graphs = graph_with_a_released_replacement();
    let graph = graphs.root_mut();
    let a = graph.node_named("a");
    let r = graph.node_named("r");
    let a_out = graph
        .add_port(a, PortType::Output, PortSide::East, Default::default())
        .unwrap();
    graph
        .add_edge(LayeredEdge::new(
            "a-r".to_string(),
            a_out,
            PortRef {
                node: r,
                port: PortId::new(0),
            },
        ))
        .unwrap();

    execute_in_arena(
        &mut graphs,
        GraphId::ROOT,
        false,
        ComponentSplit::Inactive,
        Processor::Intermediate(IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter),
    )
    .unwrap();

    let graph = graphs.root();
    let names = graph
        .layerless_nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["a", "o", "r"]);
}

#[test]
fn assembler_schedules_the_source_ported_processors_up_to_p3() {
    assert_eq!(
        kinds_through_phase(&p3_options(), LayeredPhase::P3NodeOrdering),
        vec![
            Processor::Intermediate(IntermediateProcessor::EdgeAndLayerConstraintEdgeReverser),
            Processor::CycleBreaker(CycleBreaker::Greedy),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPreprocessor),
            Processor::Layerer(Layerer::NetworkSimplex),
            Processor::Intermediate(IntermediateProcessor::LayerConstraintPostprocessor),
            Processor::Intermediate(IntermediateProcessor::LongEdgeSplitter),
            Processor::Intermediate(IntermediateProcessor::PortSideProcessor),
            Processor::Intermediate(IntermediateProcessor::PortListSorter),
            Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep),
        ]
    );
}

#[test]
fn full_runner_minimizes_crossings() {
    // The traversal order that layering gives the nodes crosses two edge pairs here. Only
    // the crossing minimizer can remove them.
    let edges = [("A", "X"), ("A", "Y"), ("A", "Z"), ("B", "X")];
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            separate_connected_components: false,
            ..p3_options()
        },
        nodes: ["A", "B", "X", "Y", "Z"].map(node).to_vec(),
        edges: edges
            .iter()
            .enumerate()
            .map(|(index, (source, target))| edge(&format!("e{index}"), source, target))
            .collect(),
    })
    .unwrap();

    execute_ported_processors(&mut graph).unwrap();

    let y_of = |id: &str| {
        graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == id)
            .unwrap()
            .position
            .y
    };
    let mut crossings = 0;
    for first in &edges {
        for second in &edges {
            let shares_an_end = first.0 == second.0 || first.1 == second.1;
            let flips_order = (y_of(first.0) < y_of(second.0)) != (y_of(first.1) < y_of(second.1));
            if !shares_an_end && flips_order {
                crossings += 1;
            }
        }
    }
    assert_eq!(crossings, 0);
}

#[test]
fn assembler_schedules_two_sided_greedy_switch_before_node_placement() {
    let executed = kinds_through_phase(
        &LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_activation_threshold: 0,
            ..LayeredOptions::default()
        },
        LayeredPhase::P4NodePlacement,
    );

    assert!(executed.contains(&Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep)));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::TwoSidedGreedySwitch
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::NodePlacer(NodePlacer::BrandesKoepf))
    );
}

#[test]
fn assembler_schedules_one_sided_greedy_switch_before_node_placement() {
    let executed = kinds_through_phase(
        &LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::OneSided,
            greedy_switch_activation_threshold: 0,
            ..LayeredOptions::default()
        },
        LayeredPhase::P4NodePlacement,
    );

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::OneSidedGreedySwitch
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::NodePlacer(NodePlacer::BrandesKoepf))
    );
}

#[test]
fn assembler_schedules_the_direction_preprocessor_for_down_layouts() {
    let executed = kinds_through_phase(
        &LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        LayeredPhase::P3NodeOrdering,
    );

    assert_eq!(
        executed[0],
        Processor::Intermediate(IntermediateProcessor::DirectionPreprocessor)
    );
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::SortByInputModelProcessor
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep))
    );
}

#[test]
fn node_placement_runs_after_node_size_and_margin_processors() {
    let options = LayeredOptions {
        direction: ElkDirection::Right,
        greedy_switch_type: GreedySwitchType::Off,
        ..LayeredOptions::default()
    };
    let executed = kinds_through_phase(&options, LayeredPhase::P4NodePlacement);
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelAndNodeSizeProcessor
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::InnermostNodeMarginCalculator
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::NodePlacer(NodePlacer::BrandesKoepf))
    );

    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options,
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    })
    .unwrap();

    execute_ported_processors(&mut graph).unwrap();

    for node in &graph.layerless_nodes {
        for port in &node.ports {
            if port.side == crate::graph::PortSide::East {
                assert_eq!(port.position.x, node.size.width);
            }
            if port.side == crate::graph::PortSide::West {
                assert_eq!(port.position.x, 0.0);
            }
        }
    }
    assert!(graph.layerless_nodes.iter().any(|node| {
        node.ports
            .iter()
            .any(|port| port.side != crate::graph::PortSide::Undefined)
    }));
}

#[test]
fn full_runner_routes_edges_with_the_orthogonal_router() {
    let options = LayeredOptions {
        direction: ElkDirection::Right,
        greedy_switch_type: GreedySwitchType::Off,
        ..LayeredOptions::default()
    };
    let executed = kinds_through_phase(&options, LayeredPhase::P5EdgeRouting);
    assert_eq!(
        executed.last(),
        Some(&Processor::EdgeRouter(EdgeRouter::Orthogonal))
    );

    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options,
        nodes: vec![node("A"), node("B")],
        edges: vec![edge("A-B", "A", "B")],
    })
    .unwrap();

    execute_ported_processors(&mut graph).unwrap();

    assert!(
        graph
            .layerless_nodes
            .iter()
            .all(|node| node.position.y.is_finite())
    );
    assert!(graph.size.height > 0.0);
    assert!(graph.size.width > 0.0);
    assert!(graph.edges.iter().all(|edge| {
        edge.bend_points
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
    }));
}

#[test]
fn source_ported_self_loop_runs_through_self_loop_lifecycle() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A")],
        edges: vec![edge("A-A", "A", "A")],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPreProcessor
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPortRestorer
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopRouter
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPostProcessor
    )));
    assert!(graph.edge_source_attached(EdgeId::new(0)));
    assert!(graph.edge_target_attached(EdgeId::new(0)));
    assert_eq!(
        graph.edges[EdgeId::new(0)].source().node,
        graph.edges[EdgeId::new(0)].target().node
    );
    assert!(graph.edges[EdgeId::new(0)].bend_points.len() >= 2);
    assert!(
        graph.edges[EdgeId::new(0)]
            .bend_points
            .iter()
            .all(|point| point.x.is_finite() && point.y.is_finite())
    );
    assert!(graph.layerless_nodes[NodeId::new(0)].margin.top > 0.0);
}

#[test]
fn assembled_processors_keep_long_edge_joiner_after_p5_router() {
    let processors = kinds(&p3_options());
    let router = processors
        .iter()
        .position(|kind| *kind == Processor::EdgeRouter(EdgeRouter::Orthogonal))
        .unwrap();
    let joiner = processors
        .iter()
        .position(|kind| *kind == Processor::Intermediate(IntermediateProcessor::LongEdgeJoiner))
        .unwrap();

    assert!(joiner > router);
}

#[test]
fn source_ported_plain_flowchart_runs_through_reversed_edge_restorer() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::EndLabelSorter
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::ReversedEdgeRestorer
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::Intermediate(
            IntermediateProcessor::ReversedEdgeRestorer
        ))
    );
    assert!(graph.edges.iter().all(|edge| !edge.reversed));
}

#[test]
fn source_ported_default_elk_runs_through_two_sided_greedy_switch() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::default(),
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::TwoSidedGreedySwitch
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::Intermediate(
            IntermediateProcessor::ReversedEdgeRestorer
        ))
    );
    assert!(graph.size.height > 0.0);
    assert!(graph.size.width > 0.0);
}

#[test]
fn source_ported_center_label_flowchart_runs_through_label_dummy_lifecycle() {
    let mut labelled = edge("A-C", "A", "C");
    labelled.labels = vec![ElkInputLabel::center("choice", 48.0, 12.0)];
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C"), labelled],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummyInserter
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummySwitcher
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelDummyRemover
    )));
    assert!(
        !graph
            .layers()
            .iter()
            .flat_map(|layer| layer.nodes().iter().copied())
            .any(|node| graph.layerless_nodes[node].kind() == crate::graph::LNodeKind::Label)
    );
    let restored = graph
        .edges
        .iter()
        .find(|edge| edge.id == "A-C" && !edge.labels.is_empty())
        .expect("center label should be restored to an A-C segment");
    assert_eq!(restored.labels[0].text, "choice");
}

// Reference: pinned elkjs 0.9.3, layered, DOWN, base spacing 40, padding 12.
#[test]
fn connected_components_pack_isolated_nodes_but_not_include_children() {
    for (separate, hierarchy, expected) in [
        (
            true,
            crate::options::HierarchyHandling::SeparateChildren,
            (160.0, 215.0),
        ),
        (
            false,
            crate::options::HierarchyHandling::SeparateChildren,
            (512.0, 61.0),
        ),
        (
            true,
            crate::options::HierarchyHandling::IncludeChildren,
            (512.0, 61.0),
        ),
    ] {
        let options = LayeredOptions {
            direction: ElkDirection::Down,
            spacing: crate::options::SpacingOptions::layered_base_value(40.0),
            separate_connected_components: separate,
            hierarchy_handling: hierarchy,
            padding: crate::options::ElkPadding::uniform(12.0),
            ..Default::default()
        };
        let nodes = ["A", "B", "C"]
            .into_iter()
            .map(|id| {
                let mut node = node(id);
                node.width = 136.0;
                node.height = 37.0;
                node
            })
            .collect();
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".into(),
            options,
            nodes,
            edges: vec![],
        })
        .unwrap();
        execute_ported_processors(&mut graph).unwrap();
        let size = graph.exported_root_size();
        assert_eq!(
            (size.width, size.height),
            expected,
            "{separate}/{hierarchy:?}"
        );
        if separate && hierarchy == crate::options::HierarchyHandling::SeparateChildren {
            for (index, node) in graph.nodes_with_ids() {
                assert_eq!(
                    node.id,
                    ["A", "B", "C"][index.index()],
                    "stable input arena indices"
                );
                assert_eq!(
                    (
                        node.position.x + graph.offset.x,
                        node.position.y + graph.offset.y
                    ),
                    (0.0, index.index() as f64 * 77.0)
                );
            }
        }
    }
}

#[test]
fn connected_components_translate_nodes_and_edges_together() {
    let options = LayeredOptions {
        direction: ElkDirection::Down,
        spacing: crate::options::SpacingOptions::layered_base_value(40.0),
        padding: crate::options::ElkPadding::uniform(12.0),
        ..Default::default()
    };
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for index in 0..3 {
        let mut start = node(&format!("start{index}"));
        start.width = 14.0;
        start.height = 14.0;
        let mut end = node(&format!("end{index}"));
        end.width = 115.0;
        end.height = 37.0;
        edges.push(edge(&format!("e{index}"), &start.id, &end.id));
        nodes.extend([start, end]);
    }
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".into(),
        options,
        nodes,
        edges,
    })
    .unwrap();
    execute_ported_processors(&mut graph).unwrap();
    assert_eq!(
        graph.exported_root_size(),
        crate::graph::LSize {
            width: 294.0,
            height: 246.0
        }
    );
    for (index, expected) in [(12.0, 66.0), (167.0, 66.0), (12.0, 197.0)]
        .into_iter()
        .enumerate()
    {
        let node = &graph.layerless_nodes[NodeId::new(index * 2 + 1)];
        assert_eq!(
            (
                node.position.x + graph.padding.left + graph.offset.x,
                node.position.y + graph.padding.top + graph.offset.y
            ),
            expected
        );
    }
    let edge = &graph.edges[EdgeId::new(2)];
    for (port_ref, expected) in [
        (edge.source(), (69.5, 157.0)),
        (edge.target(), (69.5, 197.0)),
    ] {
        let node = &graph.layerless_nodes[port_ref.node];
        let port = &node.ports[port_ref.port];
        assert_eq!(
            (
                node.position.x
                    + port.position.x
                    + port.anchor.x
                    + graph.padding.left
                    + graph.offset.x,
                node.position.y
                    + port.position.y
                    + port.anchor.y
                    + graph.padding.top
                    + graph.offset.y
            ),
            expected
        );
    }
}

#[test]
fn connected_components_keep_root_minimum_and_restore_label_and_dummy_references() {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for component in 0..2 {
        let ids = [
            format!("c{component}A"),
            format!("c{component}B"),
            format!("c{component}C"),
        ];
        nodes.extend(ids.iter().map(|id| node(id)));
        edges.push(edge(&format!("c{component}AB"), &ids[0], &ids[1]));
        edges.push(edge(&format!("c{component}BC"), &ids[1], &ids[2]));
        let mut long = edge(&format!("c{component}AC"), &ids[0], &ids[2]);
        long.labels = vec![ElkInputLabel::center(
            format!("center{component}"),
            50.0,
            18.0,
        )];
        edges.push(long);
        let mut loop_edge = edge(&format!("c{component}BB"), &ids[1], &ids[1]);
        loop_edge.labels = vec![ElkInputLabel::center(
            format!("loop{component}"),
            32.0,
            12.0,
        )];
        edges.push(loop_edge);
    }
    let ids = nodes.iter().map(|node| node.id.clone()).collect::<Vec<_>>();
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".into(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            ..Default::default()
        },
        nodes,
        edges,
    })
    .unwrap();
    let mut with_minimum = graph.clone();
    let minimum = crate::graph::LSize {
        width: 900.0,
        height: 800.0,
    };
    with_minimum.options.node_size = crate::options::NodeSizing::with_minimum(minimum);
    execute_ported_processors(&mut graph).unwrap();
    execute_ported_processors(&mut with_minimum).unwrap();
    assert_eq!(with_minimum.options.node_size.minimum, minimum);
    assert_eq!(with_minimum.exported_root_size(), minimum);
    for (index, id) in ids.iter().enumerate() {
        assert_eq!(&graph.layerless_nodes[NodeId::new(index)].id, id);
        assert_eq!(
            graph.layerless_nodes[NodeId::new(index)].position,
            with_minimum.layerless_nodes[NodeId::new(index)].position,
            "root minimum is applied after packing, through the root offset"
        );
    }
    for (index, node) in graph.nodes_with_ids() {
        for (port_index, port) in node.ports.iter_with_ids() {
            assert_eq!(port.node(), index);
            for &edge in port.incoming_edges() {
                assert_eq!(
                    graph.edges[edge].target(),
                    crate::graph::PortRef {
                        node: index,
                        port: port_index
                    }
                );
            }
            for &edge in port.outgoing_edges() {
                assert_eq!(
                    graph.edges[edge].source(),
                    crate::graph::PortRef {
                        node: index,
                        port: port_index
                    }
                );
            }
        }
    }
    for edge in &graph.edges {
        for endpoint in [edge.source(), edge.target()] {
            assert!(
                graph.layerless_nodes[endpoint.node]
                    .ports
                    .get(endpoint.port)
                    .is_some()
            );
        }
        for label in &edge.labels {
            assert!(
                label
                    .end_label_edge
                    .is_none_or(|index| index.index() < graph.edges.len())
            );
        }
    }
    for component in 0..2 {
        for suffix in ["AC", "BB"] {
            let edge =
                &graph.edges[EdgeId::new(component * 4 + if suffix == "AC" { 2 } else { 3 })];
            assert_eq!(edge.id, format!("c{component}{suffix}"));
            assert_eq!(
                edge.labels.len(),
                1,
                "label must return from its dummy to the original edge"
            );
        }
    }
}

#[test]
fn connected_components_share_random_stream_instead_of_replaying_seed() {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for component in 0..2 {
        for index in 0..3 {
            let id = format!("c{component}n{index}");
            let mut n = node(&id);
            n.width = 30.0;
            n.height = 20.0;
            nodes.push(n);
            edges.push(edge(
                &format!("c{component}e{index}"),
                &id,
                &format!("c{component}n{}", (index + 1) % 3),
            ));
        }
    }
    let options = LayeredOptions {
        direction: ElkDirection::Right,
        random_seed: 1,
        ..Default::default()
    };
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".into(),
        options,
        nodes,
        edges,
    })
    .unwrap();
    execute_ported_processors(&mut graph).unwrap();
    for (component, expected) in [[0.0, 50.0, 100.0], [100.0, 0.0, 50.0]]
        .into_iter()
        .enumerate()
    {
        let nodes = &graph.layerless_nodes.as_slice()[component * 3..component * 3 + 3];
        let left = nodes
            .iter()
            .map(|node| node.position.x)
            .fold(f64::INFINITY, f64::min);
        let actual = nodes
            .iter()
            .map(|node| node.position.x - left)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "component {component}");
    }
}

#[test]
fn source_ported_mermaid_defaults_run_through_hierarchical_resizer_for_flat_graph() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("B-C", "B", "C")],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalNodeResizer
    )));
    assert_eq!(
        executed.last(),
        Some(&Processor::Intermediate(
            IntermediateProcessor::DirectionPostprocessor
        ))
    );
    assert!(graph.layers().is_empty());
    assert!(
        graph
            .layerless_nodes
            .iter()
            .all(|node| node.layer_index().is_none())
    );
    assert!(graph.size.height > 0.0);
    assert!(graph.size.width > 0.0);
}

#[test]
fn source_ported_compound_runner_executes_bottom_up_and_resizes_parent_node() {
    let mut cluster = node("cluster");
    cluster.width = 1.0;
    cluster.height = 1.0;
    cluster.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child_a = node("A");
    child_a.parent = Some("cluster".to_string());
    let mut child_b = node("B");
    child_b.parent = Some("cluster".to_string());
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![cluster, child_a, child_b, node("C")],
        edges: vec![edge("A-B", "A", "B")],
    })
    .unwrap();

    let executed = execute_ported_compound_processors(&mut graph).unwrap();

    assert_eq!(executed.len(), 2);
    assert_eq!(executed[0].graph_name, "cluster");
    assert_eq!(executed[1].graph_name, "root");
    assert!(executed[0].processors.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalNodeResizer
    )));
    assert!(
        !executed[0]
            .processors
            .iter()
            .any(|kind| kind.hierarchy_aware_sweep().is_some())
    );
    assert!(
        executed[1]
            .processors
            .iter()
            .any(|kind| kind.hierarchy_aware_sweep().is_some())
    );
    let cluster = graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    let nested = &graph[cluster.nested_graph.unwrap()];
    assert!(cluster.size.width >= nested.size.width);
    assert!(cluster.size.height >= nested.size.height);
    assert!(nested.layers().is_empty());
}

#[test]
fn compound_runner_finishes_child_tail_between_root_hierarchy_aware_processors() {
    let mut cluster = node("cluster");
    cluster.width = 1.0;
    cluster.height = 1.0;
    cluster.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child_a = node("A");
    child_a.parent = Some("cluster".to_string());
    let mut child_b = node("B");
    child_b.parent = Some("cluster".to_string());
    let mut options = LayeredOptions::flowchart_test_defaults(ElkDirection::Down);
    options.greedy_switch_hierarchical_type = GreedySwitchType::TwoSided;
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options,
        nodes: vec![cluster, child_a, child_b, node("C")],
        edges: vec![edge("A-B", "A", "B"), edge("cluster-C", "cluster", "C")],
    })
    .unwrap();

    let executed = execute_ported_compound_processors(&mut graph).unwrap();

    let child_execution = &executed[0].processors;
    let root_execution = &executed[1].processors;
    assert!(
        root_execution
            .iter()
            .filter(|kind| kind.hierarchy_aware_sweep().is_some())
            .count()
            > 1
    );
    assert!(child_execution.contains(&Processor::Intermediate(
        IntermediateProcessor::HierarchicalNodeResizer
    )));
    assert_eq!(
        child_execution.last(),
        Some(&Processor::Intermediate(
            IntermediateProcessor::DirectionPostprocessor
        ))
    );
    let cluster = graph
        .layerless_nodes
        .iter()
        .find(|node| node.id == "cluster")
        .unwrap();
    assert!(cluster.size.width > 1.0);
    assert!(cluster.size.height > 1.0);
    assert!(graph[cluster.nested_graph.unwrap()].layers().is_empty());
}

#[test]
fn source_ported_compound_runner_routes_cross_hierarchy_edges() {
    let mut cluster = node("cluster");
    cluster.hierarchy_handling = Some(crate::options::HierarchyHandling::IncludeChildren);
    let mut child = node("A");
    child.parent = Some("cluster".to_string());
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
        nodes: vec![cluster, child],
        edges: vec![edge("cluster-A", "cluster", "A")],
    })
    .unwrap();

    let executed = execute_ported_compound_processors(&mut graph).unwrap();

    assert_eq!(executed.len(), 2);
    assert!(
        graph
            .layerless_nodes
            .iter()
            .filter(|node| node.kind() == crate::graph::LNodeKind::ExternalPort)
            .all(|node| node.layer_index().is_none())
    );
    assert!(graph.size.width > 0.0);
    assert!(graph.size.height > 0.0);
}

#[test]
fn minimum_size_and_content_alignment_match_elkjs_provider_results() {
    use crate::options::{ElkPadding, HierarchyHandling};

    // Captured from pinned elkjs 0.9.3. INCLUDE_CHILDREN applies its resizer before
    // direction postprocessing; its DOWN behavior is intentionally not normalized.
    for (direction, handling, expected_size, expected_position) in [
        (
            ElkDirection::Right,
            HierarchyHandling::SeparateChildren,
            (120.0, 80.0),
            (40.0, 0.0),
        ),
        (
            ElkDirection::Down,
            HierarchyHandling::SeparateChildren,
            (120.0, 80.0),
            (40.0, 0.0),
        ),
        (
            ElkDirection::Right,
            HierarchyHandling::IncludeChildren,
            (120.0, 80.0),
            (40.0, 0.0),
        ),
        (
            ElkDirection::Down,
            HierarchyHandling::IncludeChildren,
            (120.0, 120.0),
            (0.0, 50.0),
        ),
    ] {
        let mut child = node("a");
        child.width = 40.0;
        child.height = 20.0;
        let mut graph = import_graph(&ElkInputGraph {
            id: "r".into(),
            options: LayeredOptions {
                direction,
                hierarchy_handling: handling,
                padding: ElkPadding::uniform(0.0),
                node_size: crate::options::NodeSizing::with_minimum(LSize {
                    width: 120.0,
                    height: 80.0,
                }),
                horizontal_content_alignment: ContentAlignment::Center,
                ..Default::default()
            },
            nodes: vec![child],
            edges: vec![],
        })
        .unwrap();
        execute_ported_processors(&mut graph).unwrap();
        let size = graph.exported_root_size();
        let child = &graph.layerless_nodes[NodeId::new(0)];
        assert_eq!(
            (size.width, size.height),
            expected_size,
            "{direction:?} {handling:?}"
        );
        assert_eq!(
            (
                child.position.x + graph.offset.x + graph.padding.left,
                child.position.y + graph.offset.y + graph.padding.top
            ),
            expected_position,
            "{direction:?} {handling:?}",
        );
    }
}

#[test]
fn compound_title_minimum_matches_elkjs_symmetric_label_grid() {
    use crate::options::{ElkPadding, HierarchyHandling, NodeLabelPlacement};

    let mut group = node("g");
    group.width = 0.0;
    group.height = 0.0;
    group.node_label_placement =
        NodeLabelPlacement::from_location(crate::options::NodeLabelLocation::InsideTopCenter);
    group.labels = vec![ElkInputLabel::center("label", 100.0, 10.0)];
    group.node_size = crate::options::NodeSizing {
        constraints: crate::options::SizeConstraints::NODE_LABELS
            | crate::options::SizeConstraints::MINIMUM_SIZE,
        ..Default::default()
    };
    group.nested_options = Some(Box::new(LayeredOptions {
        padding: ElkPadding::uniform(0.0),
        node_size: crate::options::NodeSizing {
            constraints: crate::options::SizeConstraints::NODE_LABELS
                | crate::options::SizeConstraints::MINIMUM_SIZE,
            ..Default::default()
        },
        ..Default::default()
    }));
    let mut child = node("a");
    child.width = 1.0;
    child.height = 1.0;
    child.parent = Some("g".into());
    let mut graph = import_graph(&ElkInputGraph {
        id: "r".into(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            hierarchy_handling: HierarchyHandling::IncludeChildren,
            ..Default::default()
        },
        nodes: vec![group, child],
        edges: vec![],
    })
    .unwrap();
    execute_ported_compound_processors(&mut graph).unwrap();
    let group = &graph.layerless_nodes[NodeId::new(0)];
    let nested = &graph[group.nested_graph.unwrap()];
    assert_eq!(
        group.size,
        LSize {
            width: 110.0,
            height: 30.0
        }
    );
    assert_eq!(
        graph.exported_root_size(),
        LSize {
            width: 134.0,
            height: 54.0
        }
    );
    assert_eq!(
        nested.layerless_nodes[NodeId::new(0)].position.y + nested.offset.y + nested.padding.top,
        15.0
    );
}

#[test]
fn non_finite_minimum_and_overflowing_title_are_rejected() {
    for width in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut graph = import_graph(&ElkInputGraph {
            id: "root".into(),
            options: LayeredOptions {
                node_size: crate::options::NodeSizing::with_minimum(LSize {
                    width,
                    height: 80.0,
                }),
                horizontal_content_alignment: ContentAlignment::Center,
                ..Default::default()
            },
            nodes: vec![node("child")],
            edges: vec![],
        })
        .unwrap();
        assert!(matches!(
            execute_ported_processors(&mut graph),
            Err(PipelineError::NodeSize(_))
        ));
        assert_eq!(graph.offset, LPoint::default());
    }
}

#[test]
fn minimum_size_defaults_only_apply_when_constraint_is_enabled() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    resize_graph_to_minimum(&mut graph).unwrap();
    assert_eq!(actual_graph_size(&graph), LSize::default());
    graph.options.node_size = crate::options::NodeSizing::with_minimum(LSize {
        width: 0.0,
        height: -1.0,
    });
    resize_graph_to_minimum(&mut graph).unwrap();
    assert_eq!(
        actual_graph_size(&graph),
        LSize {
            width: 20.0,
            height: 20.0
        }
    );
}

#[test]
fn hierarchical_resizer_moves_east_and_south_external_ports_when_graph_grows() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.graph_properties.external_ports = true;
    graph.padding.left = 5.0;
    graph.padding.right = 5.0;
    graph.padding.top = 7.0;
    graph.padding.bottom = 7.0;
    graph.size = LSize {
        width: 10.0,
        height: 20.0,
    };

    let mut east_node = LNode::new("east", 0.0, 0.0, None);
    east_node.set_kind(LNodeKind::ExternalPort);
    east_node.external_port_mut().unwrap().side = PortSide::East;
    east_node.position.x = 12.0;
    let east = graph.next_node_id();
    graph.layerless_nodes.push(east_node);

    let mut south_node = LNode::new("south", 0.0, 0.0, None);
    south_node.set_kind(LNodeKind::ExternalPort);
    south_node.external_port_mut().unwrap().side = PortSide::South;
    south_node.position.y = 25.0;
    let south = graph.next_node_id();
    graph.layerless_nodes.push(south_node);

    resize_graph_no_really_i_mean_it(
        &mut graph,
        LSize {
            width: 20.0,
            height: 34.0,
        },
        LSize {
            width: 50.0,
            height: 74.0,
        },
    );

    assert_eq!(graph.layerless_nodes[east].position.x, 42.0);
    assert_eq!(graph.layerless_nodes[south].position.y, 65.0);
    assert_eq!(graph.size.width, 40.0);
    assert_eq!(graph.size.height, 60.0);
}

#[test]
fn layered_node_resize_moves_ports_when_compound_has_no_external_ports() {
    let mut node = LNode::new("cluster", 100.0, 50.0, None);
    node.port_constraints = PortConstraints::Free;
    node.ports.push(crate::graph::LPort::new(
        "east".to_string(),
        NodeId::new(0),
        crate::graph::PortType::Output,
    ));
    node.ports[PortId::new(0)].set_side(PortSide::East);
    node.ports[PortId::new(0)].position = LPoint { x: 100.0, y: 25.0 };
    node.ports.push(crate::graph::LPort::new(
        "south".to_string(),
        NodeId::new(0),
        crate::graph::PortType::Output,
    ));
    node.ports[PortId::new(1)].set_side(PortSide::South);
    node.ports[PortId::new(1)].position = LPoint { x: 50.0, y: 50.0 };

    resize_layered_node(
        &mut node,
        LSize {
            width: 150.0,
            height: 100.0,
        },
        true,
        true,
    );

    assert_eq!(node.size.width, 150.0);
    assert_eq!(node.size.height, 100.0);
    assert_eq!(
        node.ports[PortId::new(0)].position,
        LPoint { x: 150.0, y: 50.0 }
    );
    assert_eq!(
        node.ports[PortId::new(1)].position,
        LPoint { x: 75.0, y: 100.0 }
    );
}

#[test]
fn layered_node_resize_keeps_external_port_positions_fixed() {
    let mut node = LNode::new("cluster", 100.0, 50.0, None);
    node.port_constraints = PortConstraints::FixedPos;
    node.ports.push(crate::graph::LPort::new(
        "external".to_string(),
        NodeId::new(0),
        crate::graph::PortType::Input,
    ));
    node.ports[PortId::new(0)].set_side(PortSide::West);
    node.ports[PortId::new(0)].position = LPoint { x: -2.0, y: 20.0 };

    resize_layered_node(
        &mut node,
        LSize {
            width: 150.0,
            height: 100.0,
        },
        false,
        true,
    );

    assert_eq!(node.size.width, 150.0);
    assert_eq!(node.size.height, 100.0);
    assert_eq!(
        node.ports[PortId::new(0)].position,
        LPoint { x: -2.0, y: 20.0 }
    );
}

#[test]
fn reversed_edge_restorer_processor_restores_cycle_breaking_edges() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B")],
        edges: vec![edge("A-B", "A", "B"), edge("B-A", "B", "A")],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::ReversedEdgeRestorer
    )));
    assert!(graph.edges.iter().all(|edge| !edge.reversed));
}

#[test]
fn source_ported_end_label_flowchart_runs_through_end_label_lifecycle() {
    let mut head = ElkInputLabel::center("head", 20.0, 10.0);
    head.placement = crate::graph::EdgeLabelPlacement::Head;
    let mut labelled_edge = edge("A-B", "A", "B");
    labelled_edge.labels = vec![head];
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B")],
        edges: vec![labelled_edge],
    })
    .unwrap();

    let executed = execute_ported_processors(&mut graph).unwrap();

    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::LabelSideSelector
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::EndLabelPreprocessor
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::EndLabelSorter
    )));
    assert!(executed.contains(&Processor::Intermediate(
        IntermediateProcessor::EndLabelPostprocessor
    )));
    let edge = graph.edges.iter().find(|edge| edge.id == "A-B").unwrap();
    let label = edge
        .labels
        .iter()
        .find(|label| label.placement == crate::graph::EdgeLabelPlacement::Head)
        .expect("head label should be restored to its original edge");
    assert_eq!(label.text, "head");
    assert_eq!(label.size.width, 20.0);
    assert_eq!(label.size.height, 10.0);
    assert!(label.position.x.is_finite());
    assert!(label.position.y.is_finite());
    assert_eq!(label.end_label_edge, Some(EdgeId::new(0)));
    assert!(
        graph
            .layerless_nodes
            .iter()
            .flat_map(|node| node.ports.iter())
            .all(|port| port.labels.is_empty() && port.end_label_cell.is_none())
    );
}

// Mutation: replace `graph.parent.is_none()` with `true` in the rule for the hierarchical greedy
// switch. A graph inside a node then follows the root's rule.
#[test]
fn graph_aware_greedy_switch_matches_graph_configurator_activation_rules() {
    let mut options = LayeredOptions {
        hierarchy_handling: crate::options::HierarchyHandling::SeparateChildren,
        direction: ElkDirection::Right,
        greedy_switch_activation_threshold: 1,
        ..LayeredOptions::default()
    };
    let mut graph = LGraph::new("root", options.clone());
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("A", 10.0, 10.0, Some(0)));
    graph
        .layerless_nodes
        .push(crate::graph::LNode::new("B", 10.0, 10.0, Some(1)));
    assert!(!graph_kinds(&graph).contains(&Processor::Intermediate(
        IntermediateProcessor::TwoSidedGreedySwitch
    )));

    options.greedy_switch_activation_threshold = 0;
    graph.options = options;
    assert!(graph_kinds(&graph).contains(&Processor::Intermediate(
        IntermediateProcessor::TwoSidedGreedySwitch
    )));

    let mut nested = LGraph::new(
        "cluster",
        LayeredOptions {
            hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
            greedy_switch_hierarchical_type: crate::options::GreedySwitchType::TwoSided,
            ..LayeredOptions::default()
        },
    );
    nested.parent = Some(GraphParent {
        graph: GraphId::ROOT,
        node: NodeId::new(0),
    });
    assert!(!graph_kinds(&nested).contains(&Processor::Intermediate(
        IntermediateProcessor::TwoSidedGreedySwitch
    )));
}

fn push_test_external_dummy(graph: &mut LGraph, id: &str, side: PortSide) -> NodeId {
    let node = graph.next_node_id();
    let mut dummy = LNode::new(id, 0.0, 0.0, None);
    dummy.set_kind(LNodeKind::ExternalPort);
    dummy.external_port_mut().unwrap().side = side;
    graph.layerless_nodes.push(dummy);
    graph
        .add_port(node, PortType::Input, side.opposed(), Default::default())
        .unwrap();
    node
}

fn options_with(change: impl FnOnce(&mut LayeredOptions)) -> LayeredOptions {
    let mut options = LayeredOptions::default();
    change(&mut options);
    options
}

// Each case below names the mutation of `Strategies::select` that its row catches.
#[test]
fn every_phase_strategy_selects_its_processor_or_is_refused() {
    let cases: Vec<(LayeredOptions, Result<Processor, UnportedProcessor>)> = vec![
        // Mutation: in `CycleBreaker::select`, map `CycleBreakingStrategy::Greedy` to `CycleBreaker::DepthFirst`.
        (
            options_with(|o| o.cycle_breaking_strategy = CycleBreakingStrategy::Greedy),
            Ok(Processor::CycleBreaker(CycleBreaker::Greedy)),
        ),
        // Mutation: in `CycleBreaker::select`, map `CycleBreakingStrategy::DepthFirst` to `CycleBreaker::Interactive`.
        (
            options_with(|o| o.cycle_breaking_strategy = CycleBreakingStrategy::DepthFirst),
            Ok(Processor::CycleBreaker(CycleBreaker::DepthFirst)),
        ),
        // Mutation: in `CycleBreaker::select`, map `CycleBreakingStrategy::Interactive` to `CycleBreaker::ModelOrder`.
        (
            options_with(|o| o.cycle_breaking_strategy = CycleBreakingStrategy::Interactive),
            Ok(Processor::CycleBreaker(CycleBreaker::Interactive)),
        ),
        // Mutation: in `CycleBreaker::select`, map `CycleBreakingStrategy::ModelOrder` to `CycleBreaker::GreedyModelOrder`.
        (
            options_with(|o| o.cycle_breaking_strategy = CycleBreakingStrategy::ModelOrder),
            Ok(Processor::CycleBreaker(CycleBreaker::ModelOrder)),
        ),
        // Mutation: in `CycleBreaker::select`, map `CycleBreakingStrategy::GreedyModelOrder` to `CycleBreaker::Greedy`.
        (
            options_with(|o| o.cycle_breaking_strategy = CycleBreakingStrategy::GreedyModelOrder),
            Ok(Processor::CycleBreaker(CycleBreaker::GreedyModelOrder)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::NetworkSimplex` to `Layerer::LongestPath`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::NetworkSimplex),
            Ok(Processor::Layerer(Layerer::NetworkSimplex)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::LongestPath` to `Layerer::LongestPathSource`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::LongestPath),
            Ok(Processor::Layerer(Layerer::LongestPath)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::LongestPathSource` to `Layerer::CoffmanGraham`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::LongestPathSource),
            Ok(Processor::Layerer(Layerer::LongestPathSource)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::CoffmanGraham` to `Layerer::Interactive`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::CoffmanGraham),
            Ok(Processor::Layerer(Layerer::CoffmanGraham)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::Interactive` to `Layerer::StretchWidth`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::Interactive),
            Ok(Processor::Layerer(Layerer::Interactive)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::StretchWidth` to `Layerer::MinWidth`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::StretchWidth),
            Ok(Processor::Layerer(Layerer::StretchWidth)),
        ),
        // Mutation: in `Layerer::select`, map `LayeringStrategy::MinWidth` to `Layerer::NetworkSimplex`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::MinWidth),
            Ok(Processor::Layerer(Layerer::MinWidth)),
        ),
        // Mutation: in `Layerer::select`, accept `LayeringStrategy::BreadthFirstModelOrder` as `Layerer::NetworkSimplex`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::BreadthFirstModelOrder),
            Err(UnportedProcessor::BreadthFirstModelOrderLayerer),
        ),
        // Mutation: in `Layerer::select`, accept `LayeringStrategy::DepthFirstModelOrder` as `Layerer::NetworkSimplex`.
        (
            options_with(|o| o.layering_strategy = LayeringStrategy::DepthFirstModelOrder),
            Err(UnportedProcessor::DepthFirstModelOrderLayerer),
        ),
        // Mutation: in `CrossingMinimizer::select`, map `CrossingMinimizationStrategy::LayerSweep` to `CrossingMinimizer::NoMinimizer`.
        (
            options_with(|o| {
                o.crossing_minimization_strategy = CrossingMinimizationStrategy::LayerSweep
            }),
            Ok(Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep)),
        ),
        // Mutation: in `CrossingMinimizer::select`, map `CrossingMinimizationStrategy::None` to `CrossingMinimizer::LayerSweep`.
        (
            options_with(|o| o.crossing_minimization_strategy = CrossingMinimizationStrategy::None),
            Ok(Processor::CrossingMinimizer(CrossingMinimizer::NoMinimizer)),
        ),
        // Mutation: in `CrossingMinimizer::select`, accept `CrossingMinimizationStrategy::Interactive` as `CrossingMinimizer::LayerSweep`.
        (
            options_with(|o| {
                o.crossing_minimization_strategy = CrossingMinimizationStrategy::Interactive
            }),
            Err(UnportedProcessor::InteractiveCrossingMinimizer),
        ),
        // Mutation: in `NodePlacer::select`, map `NodePlacementStrategy::Simple` to `NodePlacer::BrandesKoepf`.
        (
            options_with(|o| o.node_placement_strategy = NodePlacementStrategy::Simple),
            Ok(Processor::NodePlacer(NodePlacer::Simple)),
        ),
        // Mutation: in `NodePlacer::select`, map `NodePlacementStrategy::BrandesKoepf` to `NodePlacer::NetworkSimplex`.
        (
            options_with(|o| o.node_placement_strategy = NodePlacementStrategy::BrandesKoepf),
            Ok(Processor::NodePlacer(NodePlacer::BrandesKoepf)),
        ),
        // Mutation: in `NodePlacer::select`, map `NodePlacementStrategy::NetworkSimplex` to `NodePlacer::Simple`.
        (
            options_with(|o| o.node_placement_strategy = NodePlacementStrategy::NetworkSimplex),
            Ok(Processor::NodePlacer(NodePlacer::NetworkSimplex)),
        ),
        // Mutation: in `NodePlacer::select`, accept `NodePlacementStrategy::Interactive` as `NodePlacer::Simple`.
        (
            options_with(|o| o.node_placement_strategy = NodePlacementStrategy::Interactive),
            Err(UnportedProcessor::InteractiveNodePlacer),
        ),
        // Mutation: in `NodePlacer::select`, accept `NodePlacementStrategy::LinearSegments` as `NodePlacer::Simple`.
        (
            options_with(|o| o.node_placement_strategy = NodePlacementStrategy::LinearSegments),
            Err(UnportedProcessor::LinearSegmentsNodePlacer),
        ),
        // Mutation: in `EdgeRouter::select`, refuse `EdgeRouting::Orthogonal` as an unported router.
        (
            options_with(|o| o.edge_routing = EdgeRouting::Orthogonal),
            Ok(Processor::EdgeRouter(EdgeRouter::Orthogonal)),
        ),
        // Mutation: in `EdgeRouter::select`, accept `EdgeRouting::Polyline` as `EdgeRouter::Orthogonal`.
        (
            options_with(|o| o.edge_routing = EdgeRouting::Polyline),
            Err(UnportedProcessor::PolylineEdgeRouter),
        ),
        // Mutation: in `EdgeRouter::select`, accept `EdgeRouting::Splines` as `EdgeRouter::Orthogonal`.
        (
            options_with(|o| o.edge_routing = EdgeRouting::Splines),
            Err(UnportedProcessor::SplineEdgeRouter),
        ),
    ];

    for (options, expected) in cases {
        match expected {
            Ok(processor) => {
                let in_phase: Vec<Processor> = kinds(&options)
                    .into_iter()
                    .filter(|candidate| candidate.phase() == processor.phase())
                    .collect();
                assert_eq!(in_phase, vec![processor]);
            }
            Err(kind) => assert_eq!(
                try_kinds(&options),
                Err(PipelineError::UnsupportedProcessor { kind })
            ),
        }
    }
}

#[test]
fn wrapping_strategies_are_refused() {
    let cases = [
        // Mutation: return `None` for `WrappingStrategy::SingleEdge` in `wrapping_refusal`.
        (
            WrappingStrategy::SingleEdge,
            UnportedProcessor::SingleEdgeGraphWrapper,
        ),
        // Mutation: return `None` for `WrappingStrategy::MultiEdge` in `wrapping_refusal`.
        (
            WrappingStrategy::MultiEdge,
            UnportedProcessor::BreakingPointInserter,
        ),
    ];
    for (wrapping_strategy, kind) in cases {
        let options = options_with(|o| o.wrapping_strategy = wrapping_strategy);
        assert_eq!(
            try_kinds(&options),
            Err(PipelineError::UnsupportedProcessor { kind })
        );
    }
}

// Mutation: make `hypernodes_refusal` return `None`.
#[test]
fn a_graph_with_hypernodes_is_refused() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.graph_properties.hypernodes = true;
    assert_eq!(
        try_graph_kinds(&graph),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::HypernodesProcessor,
        })
    );
}

#[test]
fn the_first_unported_processor_is_the_one_the_schedule_reaches_first() {
    let cases = [
        // Mutation: give `WrappingStrategy::MultiEdge` the place
        // `Place::Before(LayeredPhase::P1CycleBreaking)` in `wrapping_refusal`.
        (
            options_with(|o| {
                o.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;
                o.wrapping_strategy = WrappingStrategy::MultiEdge;
            }),
            UnportedProcessor::BreadthFirstModelOrderLayerer,
        ),
        // Mutation: give the interactive crossing minimizer the place
        // `Place::Before(LayeredPhase::P2Layering)` in `CrossingMinimizer::select`.
        (
            options_with(|o| {
                o.crossing_minimization_strategy = CrossingMinimizationStrategy::Interactive;
                o.wrapping_strategy = WrappingStrategy::MultiEdge;
            }),
            UnportedProcessor::BreakingPointInserter,
        ),
        // Mutation: give the polyline edge router the place
        // `Place::Before(LayeredPhase::P4NodePlacement)` in `EdgeRouter::select`.
        (
            options_with(|o| {
                o.node_placement_strategy = NodePlacementStrategy::Interactive;
                o.edge_routing = EdgeRouting::Polyline;
            }),
            UnportedProcessor::InteractiveNodePlacer,
        ),
        // Mutation: give `WrappingStrategy::SingleEdge` the place
        // `Place::Before(LayeredPhase::P3NodeOrdering)` in `wrapping_refusal`.
        (
            options_with(|o| {
                o.crossing_minimization_strategy = CrossingMinimizationStrategy::Interactive;
                o.wrapping_strategy = WrappingStrategy::SingleEdge;
            }),
            UnportedProcessor::InteractiveCrossingMinimizer,
        ),
        // Mutation: give `WrappingStrategy::SingleEdge` the place
        // `Place::After(LayeredPhase::P4NodePlacement)` in `wrapping_refusal`.
        (
            options_with(|o| {
                o.node_placement_strategy = NodePlacementStrategy::Interactive;
                o.wrapping_strategy = WrappingStrategy::SingleEdge;
            }),
            UnportedProcessor::SingleEdgeGraphWrapper,
        ),
    ];
    for (options, kind) in cases {
        assert_eq!(
            try_kinds(&options),
            Err(PipelineError::UnsupportedProcessor { kind })
        );
    }

    let mut graph = LGraph::new(
        "root",
        options_with(|o| o.edge_routing = EdgeRouting::Polyline),
    );
    graph.graph_properties.hypernodes = true;
    // Mutation: in `hypernodes_refusal`, ignore the edge router and give the refusal the place
    // `Place::Before(LayeredPhase::P5EdgeRouting)`.
    assert_eq!(
        try_graph_kinds(&graph),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::PolylineEdgeRouter,
        })
    );
}

// Mutation: in `hypernodes_refusal`, drop the `EdgeRouting::Orthogonal` check. A graph with
// hypernodes is then refused under a router that never asks for `HYPERNODE_PROCESSOR`.
#[test]
fn hypernodes_are_refused_only_for_the_router_that_asks_for_their_processor() {
    let with = GraphProperties {
        hypernodes: true,
        ..Default::default()
    };
    assert!(hypernodes_refusal(&with, EdgeRouting::Orthogonal).is_some());
    assert!(hypernodes_refusal(&with, EdgeRouting::Polyline).is_none());
    assert!(hypernodes_refusal(&with, EdgeRouting::Splines).is_none());
    let without = GraphProperties::default();
    assert!(hypernodes_refusal(&without, EdgeRouting::Orthogonal).is_none());
}

// The table guards three things, one mutation each:
// - Mutation: leave `hypernodes_refusal(properties)` out of `refusals` in `Strategies::select`.
//   The row checked alone then gives no refusal.
// - Mutation: use `max_by_key` instead of `min_by_key` in `Strategies::select`. Every pair then
//   names the later processor.
// - Mutation: give the refusal of `HypernodesProcessor` the place
//   `Place::Before(LayeredPhase::P1CycleBreaking)` in `hypernodes_refusal`. It then beats every
//   other refusal, and each pair that lists it second fails.
#[test]
fn of_two_refusals_the_one_earlier_in_the_schedule_wins() {
    type Trigger = fn(&mut LayeredOptions, &mut GraphProperties);
    // Each row: the option or property it sets, what a run is refused for, and how to set it.
    // The rows stand in the order of the schedule in ELK's `GraphConfigurator` and the phases.
    let schedule: [(&str, UnportedProcessor, Trigger); 10] = [
        (
            "layering",
            UnportedProcessor::BreadthFirstModelOrderLayerer,
            |o, _| o.layering_strategy = LayeringStrategy::BreadthFirstModelOrder,
        ),
        (
            "layering",
            UnportedProcessor::DepthFirstModelOrderLayerer,
            |o, _| o.layering_strategy = LayeringStrategy::DepthFirstModelOrder,
        ),
        (
            "wrapping",
            UnportedProcessor::BreakingPointInserter,
            |o, _| o.wrapping_strategy = WrappingStrategy::MultiEdge,
        ),
        (
            "crossing",
            UnportedProcessor::InteractiveCrossingMinimizer,
            |o, _| o.crossing_minimization_strategy = CrossingMinimizationStrategy::Interactive,
        ),
        (
            "wrapping",
            UnportedProcessor::SingleEdgeGraphWrapper,
            |o, _| o.wrapping_strategy = WrappingStrategy::SingleEdge,
        ),
        (
            "placement",
            UnportedProcessor::InteractiveNodePlacer,
            |o, _| o.node_placement_strategy = NodePlacementStrategy::Interactive,
        ),
        (
            "placement",
            UnportedProcessor::LinearSegmentsNodePlacer,
            |o, _| o.node_placement_strategy = NodePlacementStrategy::LinearSegments,
        ),
        ("routing", UnportedProcessor::PolylineEdgeRouter, |o, _| {
            o.edge_routing = EdgeRouting::Polyline
        }),
        ("routing", UnportedProcessor::SplineEdgeRouter, |o, _| {
            o.edge_routing = EdgeRouting::Splines
        }),
        (
            "hypernodes",
            UnportedProcessor::HypernodesProcessor,
            |_, p| p.hypernodes = true,
        ),
    ];

    let refused_for = |triggers: &[Trigger]| {
        let mut options = LayeredOptions::default();
        let mut properties = GraphProperties::default();
        for trigger in triggers {
            trigger(&mut options, &mut properties);
        }
        Strategies::select(&options, &properties).err()
    };

    let mut pairs = 0;
    for (i, (first_field, first_kind, first)) in schedule.iter().enumerate() {
        assert_eq!(
            refused_for(&[*first]),
            Some(PipelineError::UnsupportedProcessor { kind: *first_kind })
        );
        for (second_field, _, second) in &schedule[i + 1..] {
            // Two values of one option cannot be set together.
            if first_field == second_field {
                continue;
            }
            pairs += 1;
            assert_eq!(
                refused_for(&[*first, *second]),
                Some(PipelineError::UnsupportedProcessor { kind: *first_kind })
            );
        }
    }
    assert_eq!(pairs, 41);
}

// Mutation: in `Config::add_after`, use `slot_before` instead of `slot_after`. The processor that
// ELK adds after phase five then runs in front of the router, and the order of the places no
// longer matches the order of the run.
#[test]
fn the_places_stand_in_the_order_of_the_assembled_run() {
    let run = kinds(&LayeredOptions::default());
    let phase_index = |phase: LayeredPhase| {
        run.iter()
            .position(|processor| processor.phase() == Some(phase))
            .unwrap()
    };
    let index_of = |processor: IntermediateProcessor| {
        run.iter()
            .position(|candidate| *candidate == Processor::Intermediate(processor))
            .unwrap()
    };
    // Each row is a step of the real run and the place that names it.
    let mut steps = vec![
        (
            Place::In(LayeredPhase::P2Layering),
            phase_index(LayeredPhase::P2Layering),
        ),
        (
            Place::In(LayeredPhase::P3NodeOrdering),
            phase_index(LayeredPhase::P3NodeOrdering),
        ),
        (
            Place::In(LayeredPhase::P4NodePlacement),
            phase_index(LayeredPhase::P4NodePlacement),
        ),
        (
            Place::In(LayeredPhase::P5EdgeRouting),
            phase_index(LayeredPhase::P5EdgeRouting),
        ),
        (
            Place::Before(LayeredPhase::P4NodePlacement),
            index_of(IntermediateProcessor::InnermostNodeMarginCalculator),
        ),
        (
            Place::Before(LayeredPhase::P5EdgeRouting),
            index_of(IntermediateProcessor::LayerSizeAndGraphHeightCalculator),
        ),
        (
            Place::After(LayeredPhase::P5EdgeRouting),
            index_of(IntermediateProcessor::EndLabelSorter),
        ),
    ];
    steps.sort_by_key(|(place, _)| place.position());
    let indices: Vec<usize> = steps.iter().map(|(_, index)| *index).collect();
    assert!(
        indices.windows(2).all(|pair| pair[0] < pair[1]),
        "the places and the run disagree: {steps:?}"
    );
    // A slot after one phase is the slot in front of the next.
    assert_eq!(
        Place::After(LayeredPhase::P2Layering).position(),
        Place::Before(LayeredPhase::P3NodeOrdering).position()
    );
}

// Mutation: replace `assemble_graph(graph)?` in `graph_algorithm` by
// `assemble_graph(graph).unwrap_or_default()`. A refusal that only shows after the preprocessor
// then gives an empty list instead of an error.
#[test]
fn a_refusal_that_shows_after_the_preprocessor_still_stops_the_run() {
    let mut graphs = deep_compound_chain(2);
    check_strategies(&graphs).unwrap();
    graphs.root_mut().graph_properties.hypernodes = true;

    assert_eq!(
        CompoundRun::new(&graphs).map(|_| ()),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::HypernodesProcessor,
        })
    );
}

// Mutation: in `CompoundRun::new`, drop the `.rev()`, so the root is built first. The root's
// refusal then hides the one of the graph that runs first.
#[test]
fn the_lists_are_built_in_the_order_the_graphs_run() {
    let mut graphs = deep_compound_chain(3);
    graphs.root_mut().options.node_placement_strategy = NodePlacementStrategy::Interactive;
    graphs[GraphId::new(2)].options.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;

    assert_eq!(
        CompoundRun::new(&graphs).map(|_| ()),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::BreadthFirstModelOrderLayerer,
        })
    );
}

// Mutation: in `check_strategies`, drop the `.rev()`, so the root is selected first. The root's
// refusal then hides the one of the graph that runs first.
#[test]
fn the_graph_that_runs_first_names_the_first_refusal() {
    let mut graphs = deep_compound_chain(3);
    graphs.root_mut().options.node_placement_strategy = NodePlacementStrategy::Interactive;
    let leaf = GraphId::new(2);
    graphs[leaf].options.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;

    assert_eq!(
        check_strategies(&graphs),
        Err(PipelineError::UnsupportedProcessor {
            kind: UnportedProcessor::BreadthFirstModelOrderLayerer,
        })
    );
}

// Mutation: read `options.graph_has_self_loops` instead of `properties.self_loops` in
// `edge_routing_configuration`. The importer keeps that copy in step, but a graph that is not
// configured yet has the old value.
#[test]
fn the_list_follows_the_graph_properties_not_their_copies_in_the_options() {
    let mut graph = LGraph::new("root", LayeredOptions::default());
    graph.graph_properties.self_loops = true;
    assert!(!graph.options.graph_has_self_loops);

    assert!(graph_kinds(&graph).contains(&Processor::Intermediate(
        IntermediateProcessor::SelfLoopPreProcessor
    )));
}

// Mutation: treat `ElkDirection::Undefined` as `ElkDirection::Down` in
// `graph_configurator_configuration`. The configurator turns an undefined direction into `Right`,
// and the run selects before it and builds after it.
#[test]
fn the_configurator_does_not_change_the_list() {
    let mut graph = LGraph::new(
        "root",
        options_with(|o| {
            o.direction = ElkDirection::Undefined;
            o.spacing.edge_edge = 0.0;
        }),
    );
    graph.graph_properties.center_labels = true;
    graph.graph_properties.end_labels = true;
    let before = graph_kinds(&graph);
    let mut graphs = GraphArena::new(graph);

    crate::configurator::configure_graph_properties(&mut graphs).unwrap();

    assert_eq!(graph_kinds(graphs.root()), before);
}

// Mutation: swap `LongEdgeSplitter` and `InvertedPortProcessor` in the declaration of
// `IntermediateProcessor`. The enum's order is the order the processors run in.
#[test]
fn intermediate_processors_run_in_the_order_of_elks_enum() {
    use IntermediateProcessor as P;
    let mut graph = LGraph::new(
        "root",
        LayeredOptions {
            direction: ElkDirection::Left,
            hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
            feedback_edges: true,
            consider_model_order_strategy: OrderingStrategy::NodesAndEdges,
            greedy_switch_hierarchical_type: GreedySwitchType::OneSided,
            cycle_breaking_strategy: CycleBreakingStrategy::Interactive,
            ..LayeredOptions::default()
        },
    );
    graph.graph_properties = GraphProperties {
        self_loops: true,
        center_labels: true,
        end_labels: true,
        non_free_ports: true,
        north_south_ports: true,
        hyperedges: true,
        external_ports: true,
        hypernodes: false,
    };

    let intermediates: Vec<IntermediateProcessor> = graph_kinds(&graph)
        .into_iter()
        .filter_map(|processor| match processor {
            Processor::Intermediate(intermediate) => Some(intermediate),
            _ => None,
        })
        .collect();

    assert_eq!(
        intermediates,
        [
            P::DirectionPreprocessor,
            P::EdgeAndLayerConstraintEdgeReverser,
            P::InteractiveExternalPortPositioner,
            P::SelfLoopPreProcessor,
            P::PortSideProcessor,
            P::LabelDummyInserter,
            P::LayerConstraintPreprocessor,
            P::LayerConstraintPostprocessor,
            P::HierarchicalPortConstraintProcessor,
            P::LongEdgeSplitter,
            P::InvertedPortProcessor,
            P::PortListSorter,
            P::SortByInputModelProcessor,
            P::NorthSouthPortPreprocessor,
            P::OneSidedGreedySwitch,
            P::SelfLoopPortRestorer,
            P::InLayerConstraintProcessor,
            P::LabelAndNodeSizeProcessor,
            P::InnermostNodeMarginCalculator,
            P::SelfLoopRouter,
            P::EndLabelPreprocessor,
            P::LabelDummySwitcher,
            P::LabelSideSelector,
            P::HyperedgeDummyMerger,
            P::HierarchicalPortDummySizeProcessor,
            P::LayerSizeAndGraphHeightCalculator,
            P::HierarchicalPortPositionProcessor,
            P::HierarchicalPortOrthogonalEdgeRouter,
            P::LongEdgeJoiner,
            P::SelfLoopPostProcessor,
            P::NorthSouthPortPostprocessor,
            P::LabelDummyRemover,
            P::EndLabelSorter,
            P::ReversedEdgeRestorer,
            P::EndLabelPostprocessor,
            P::HierarchicalNodeResizer,
            P::DirectionPostprocessor,
        ]
    );
}

// Mutation: let `execute_processor` run a layer sweep processor as a no-op instead of returning
// the error.
#[test]
fn a_layer_sweep_outside_the_arena_is_an_error_not_a_panic() {
    for processor in [
        Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep),
        Processor::Intermediate(IntermediateProcessor::OneSidedGreedySwitch),
        Processor::Intermediate(IntermediateProcessor::TwoSidedGreedySwitch),
    ] {
        let mut graph = LGraph::new("root", LayeredOptions::default());
        assert_eq!(
            execute_processor(&mut graph, OtherGraphs::none(), processor),
            Err(PipelineError::SweepOutsideArena),
            "{processor:?}"
        );
    }
}
