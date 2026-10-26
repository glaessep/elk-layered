//! ELK layered processor pipeline.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/ElkLayered.java
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes
//! - https://github.com/eclipse-elk/elk/tree/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalNodeResizingProcessor.java

use super::options::{
    ContentAlignment, CrossingMinimizationStrategy, CycleBreakingStrategy, EdgeRouting,
    ElkDirection, GreedySwitchType, LayeredOptions, LayeringStrategy, NodePlacementStrategy,
    OrderingStrategy, PortConstraints, WrappingStrategy,
};
use crate::RandomSeedError;
use crate::compound::preprocess_source_ported_compound_graph;
use crate::configurator::{check_random_seeds, configure_graph_properties, configured_options};
use crate::graph::{LGraph, LNode, LNodeKind, LPoint, LSize, PortSide};
use crate::intermediate::{
    IntermediateError, calculate_layer_sizes_and_graph_height, insert_label_dummies,
    join_long_edges, merge_hyperedge_dummies, position_interactive_external_ports,
    postprocess_end_labels, postprocess_layer_constraints, preprocess_end_labels,
    preprocess_layer_constraints, process_hierarchical_port_constraints,
    process_hierarchical_port_dummy_sizes, process_hierarchical_port_orthogonal_edges,
    process_hierarchical_port_positions, process_inverted_ports, remove_label_dummies,
    restore_reversed_edges, reverse_edges_for_edge_and_layer_constraints, select_label_sides,
    sort_end_labels, split_long_edges, switch_label_dummies,
};
use crate::p1cycles::{
    break_cycles_depth_first, break_cycles_greedy, break_cycles_greedy_model_order,
    break_cycles_interactive, break_cycles_model_order,
};
use crate::p2layers::{LayeringError, layer_network_simplex, layer_with_strategy};
use crate::p3order::{
    process_port_sides, sort_by_input_model, sort_port_lists,
    sweep::{
        CrossMinType, minimize_crossings_layer_sweep_hierarchical_with_type,
        minimize_crossings_layer_sweep_with_type,
    },
};
use crate::p4nodes::{
    calculate_innermost_node_margins, calculate_label_and_node_sizes, place_nodes_brandes_koepf,
    place_nodes_network_simplex, place_nodes_simple, process_in_layer_constraints,
};
use crate::p5edges::route_edges_orthogonal;
use crate::selfloops::{
    postprocess_self_loops, preprocess_self_loops, restore_self_loop_ports, route_self_loops,
};
use crate::transform::{GraphTransformMode, transform_graph_direction};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PipelineError {
    #[error(transparent)]
    Component(#[from] crate::components::ComponentError),
    #[error(transparent)]
    NodeSize(#[from] crate::options::NodeSizeError),
    #[error(transparent)]
    NetworkSimplex(#[from] crate::common::networksimplex::NetworkSimplexError),
    #[error(transparent)]
    RandomSeed(#[from] RandomSeedError),
    #[error("layered processor `{kind:?}` is not ported yet")]
    UnsupportedProcessor { kind: ProcessorKind },
    #[error("source-backed compound ELK layout does not support this graph yet: {reason}")]
    UnsupportedCompoundGraph { reason: &'static str },
    #[error(transparent)]
    Intermediate(#[from] IntermediateError),
    #[error(transparent)]
    Layering(#[from] LayeringError),
}

pub type PipelineResult<T> = Result<T, PipelineError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayeredPhase {
    P1CycleBreaking,
    P2Layering,
    P3NodeOrdering,
    P4NodePlacement,
    P5EdgeRouting,
}

impl LayeredPhase {
    const ALL: [Self; 5] = [
        Self::P1CycleBreaking,
        Self::P2Layering,
        Self::P3NodeOrdering,
        Self::P4NodePlacement,
        Self::P5EdgeRouting,
    ];

    fn ordinal(self) -> usize {
        match self {
            Self::P1CycleBreaking => 0,
            Self::P2Layering => 1,
            Self::P3NodeOrdering => 2,
            Self::P4NodePlacement => 3,
            Self::P5EdgeRouting => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorSlot {
    pub phase: Option<LayeredPhase>,
    pub kind: ProcessorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum ProcessorKind {
    DirectionPreprocessor,
    CommentPreprocessor,
    EdgeAndLayerConstraintEdgeReverser,
    InteractiveExternalPortPositioner,
    PartitionPreprocessor,
    GreedyCycleBreaker,
    DepthFirstCycleBreaker,
    InteractiveCycleBreaker,
    ModelOrderCycleBreaker,
    GreedyModelOrderCycleBreaker,
    LayerConstraintPreprocessor,
    NetworkSimplexLayerer,
    LongestPathLayerer,
    LongestPathSourceLayerer,
    CoffmanGrahamLayerer,
    InteractiveLayerer,
    StretchWidthLayerer,
    MinWidthLayerer,
    BreadthFirstModelOrderLayerer,
    DepthFirstModelOrderLayerer,
    LabelDummyInserter,
    SelfLoopPreProcessor,
    LayerConstraintPostprocessor,
    PartitionMidprocessor,
    LongEdgeSplitter,
    PortSideProcessor,
    InvertedPortProcessor,
    PortListSorter,
    SortByInputModelProcessor,
    NorthSouthPortPreprocessor,
    HighDegreeNodeLayerProcessor,
    NodePromotion,
    PartitionPostprocessor,
    HierarchicalPortConstraintProcessor,
    SemiInteractiveCrossMinProcessor,
    LayerSweepCrossingMinimizerBarycenter,
    LayerSweepCrossingMinimizerOneSidedGreedySwitch,
    LayerSweepCrossingMinimizerTwoSidedGreedySwitch,
    InteractiveCrossingMinimizer,
    NoCrossingMinimizer,
    InLayerConstraintProcessor,
    EndNodePortLabelManagementProcessor,
    LabelAndNodeSizeProcessor,
    InnermostNodeMarginCalculator,
    CommentNodeMarginCalculator,
    EndLabelPreprocessor,
    LabelSideSelector,
    HyperedgeDummyMerger,
    HierarchicalPortDummySizeProcessor,
    BKNodePlacer,
    SimpleNodePlacer,
    InteractiveNodePlacer,
    LinearSegmentsNodePlacer,
    NetworkSimplexPlacer,
    LayerSizeAndGraphHeightCalculator,
    HierarchicalPortPositionProcessor,
    OrthogonalEdgeRouter,
    PolylineEdgeRouter,
    SplineEdgeRouter,
    ConstraintsPostprocessor,
    CommentPostprocessor,
    LongEdgeJoiner,
    NorthSouthPortPostprocessor,
    HorizontalGraphCompactor,
    LabelDummyRemover,
    FinalSplineBendpointsCalculator,
    EndLabelSorter,
    ReversedEdgeRestorer,
    EndLabelPostprocessor,
    HierarchicalNodeResizer,
    DirectionPostprocessor,
    SelfLoopPortRestorer,
    SelfLoopRouter,
    SelfLoopPostProcessor,
    LabelDummySwitcher,
    CenterLabelManagementProcessor,
    HierarchicalPortOrthogonalEdgeRouter,
    HypernodesProcessor,
    BreakingPointInserter,
    BreakingPointProcessor,
    BreakingPointRemover,
    SingleEdgeGraphWrapper,
}

impl ProcessorKind {
    pub fn is_hierarchy_aware(self) -> bool {
        matches!(
            self,
            Self::LayerSweepCrossingMinimizerBarycenter
                | Self::LayerSweepCrossingMinimizerOneSidedGreedySwitch
                | Self::LayerSweepCrossingMinimizerTwoSidedGreedySwitch
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExecution {
    pub graph_id: String,
    pub parent_node_id: Option<String>,
    pub processors: Vec<ProcessorKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GraphAlgorithm {
    next_processor: usize,
    processors: Vec<ProcessorSlot>,
    execution: GraphExecution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraphArenaParent {
    graph: usize,
    node: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraphArenaEntry {
    parent: Option<GraphArenaParent>,
}

#[derive(Debug)]
struct CompoundGraphArena {
    entries: Vec<GraphArenaEntry>,
    algorithms: Vec<GraphAlgorithm>,
    graph_slots: Vec<Option<Box<LGraph>>>,
}

impl CompoundGraphArena {
    fn new(root: &LGraph) -> Self {
        let mut entries = vec![GraphArenaEntry { parent: None }];
        let mut algorithms = vec![graph_algorithm(root)];
        let mut search = vec![(0usize, root)];

        while let Some((graph_index, graph)) = search.pop() {
            for (node_index, node) in graph.layerless_nodes.iter().enumerate() {
                let Some(nested) = node.nested_graph.as_deref() else {
                    continue;
                };
                let child_index = entries.len();
                entries.push(GraphArenaEntry {
                    parent: Some(GraphArenaParent {
                        graph: graph_index,
                        node: node_index,
                    }),
                });
                algorithms.push(graph_algorithm(nested));
                // Match ELK's `ArrayDeque.push` discovery order. The last sibling is searched
                // first, while the collected graph list is later executed in reverse discovery
                // order.
                search.push((child_index, nested));
            }
        }

        let mut graph_slots = Vec::with_capacity(entries.len());
        graph_slots.resize_with(entries.len(), || None);

        Self {
            entries,
            algorithms,
            graph_slots,
        }
    }

    fn into_executions(self) -> Vec<GraphExecution> {
        // ELK's `collectAllGraphsBottomUp` returns the reverse of discovery order. This is not
        // interchangeable with ordinary DFS postorder: sibling subtrees are interleaved in a
        // public, error-observable order.
        self.algorithms
            .into_iter()
            .rev()
            .map(|algorithm| algorithm.execution)
            .collect()
    }
}

fn graph_algorithm(graph: &LGraph) -> GraphAlgorithm {
    GraphAlgorithm {
        next_processor: 0,
        processors: assemble_processors_for_graph(graph),
        execution: GraphExecution {
            graph_id: graph.id.clone(),
            parent_node_id: graph.parent_node_id.clone(),
            processors: Vec::new(),
        },
    }
}

#[derive(Debug)]
struct Config {
    slots: [Vec<ProcessorKind>; 6],
    phases: [Option<ProcessorKind>; 5],
}

impl Config {
    fn add_to_slot(&mut self, slot: usize, kind: ProcessorKind) {
        if !self.slots[slot].contains(&kind) {
            self.slots[slot].push(kind);
        }
    }

    fn add_before(&mut self, phase: LayeredPhase, kind: ProcessorKind) {
        self.add_to_slot(phase.ordinal(), kind);
    }

    fn add_after(&mut self, phase: LayeredPhase, kind: ProcessorKind) {
        self.add_to_slot(phase.ordinal() + 1, kind);
    }

    fn add_phase(&mut self, phase: LayeredPhase, kind: ProcessorKind) {
        self.phases[phase.ordinal()] = Some(kind);
    }

    fn merge(&mut self, other: Config) {
        for (slot, processors) in other.slots.into_iter().enumerate() {
            for kind in processors {
                self.add_to_slot(slot, kind);
            }
        }

        for (phase, kind) in other.phases.into_iter().enumerate() {
            if let Some(kind) = kind {
                self.phases[phase] = Some(kind);
            }
        }
    }

    fn into_slots(mut self) -> Vec<ProcessorSlot> {
        let mut out = Vec::new();

        for phase in LayeredPhase::ALL {
            let phase_index = phase.ordinal();
            push_processors(&mut out, &mut self.slots[phase_index]);

            if let Some(kind) = self.phases[phase_index] {
                out.push(ProcessorSlot {
                    phase: Some(phase),
                    kind,
                });
            }
        }

        push_processors(&mut out, &mut self.slots[LayeredPhase::ALL.len()]);
        out
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| Vec::new()),
            phases: [None; 5],
        }
    }
}

fn push_processors(out: &mut Vec<ProcessorSlot>, processors: &mut [ProcessorKind]) {
    processors.sort_by_key(|kind| intermediate_processor_order(*kind));
    out.extend(processors.iter().map(|kind| ProcessorSlot {
        phase: None,
        kind: *kind,
    }));
}

fn intermediate_processor_order(kind: ProcessorKind) -> usize {
    match kind {
        ProcessorKind::DirectionPreprocessor => 0,
        ProcessorKind::CommentPreprocessor => 1,
        ProcessorKind::EdgeAndLayerConstraintEdgeReverser => 2,
        ProcessorKind::InteractiveExternalPortPositioner => 3,
        ProcessorKind::PartitionPreprocessor => 4,
        ProcessorKind::LabelDummyInserter => 5,
        ProcessorKind::SelfLoopPreProcessor => 6,
        ProcessorKind::LayerConstraintPreprocessor => 7,
        ProcessorKind::PartitionMidprocessor => 8,
        ProcessorKind::HighDegreeNodeLayerProcessor => 9,
        ProcessorKind::NodePromotion => 10,
        ProcessorKind::LayerConstraintPostprocessor => 11,
        ProcessorKind::PartitionPostprocessor => 12,
        ProcessorKind::HierarchicalPortConstraintProcessor => 13,
        ProcessorKind::SemiInteractiveCrossMinProcessor => 14,
        ProcessorKind::BreakingPointInserter => 15,
        ProcessorKind::LongEdgeSplitter => 16,
        ProcessorKind::PortSideProcessor => 17,
        ProcessorKind::InvertedPortProcessor => 18,
        ProcessorKind::PortListSorter => 19,
        ProcessorKind::SortByInputModelProcessor => 20,
        ProcessorKind::NorthSouthPortPreprocessor => 21,
        ProcessorKind::BreakingPointProcessor => 22,
        ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch => 23,
        ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch => 24,
        ProcessorKind::SelfLoopPortRestorer => 25,
        ProcessorKind::SingleEdgeGraphWrapper => 26,
        ProcessorKind::InLayerConstraintProcessor => 27,
        ProcessorKind::EndNodePortLabelManagementProcessor => 28,
        ProcessorKind::LabelAndNodeSizeProcessor => 29,
        ProcessorKind::InnermostNodeMarginCalculator => 30,
        ProcessorKind::SelfLoopRouter => 31,
        ProcessorKind::CommentNodeMarginCalculator => 32,
        ProcessorKind::EndLabelPreprocessor => 33,
        ProcessorKind::LabelDummySwitcher => 34,
        ProcessorKind::CenterLabelManagementProcessor => 35,
        ProcessorKind::LabelSideSelector => 36,
        ProcessorKind::HyperedgeDummyMerger => 37,
        ProcessorKind::HierarchicalPortDummySizeProcessor => 38,
        ProcessorKind::LayerSizeAndGraphHeightCalculator => 39,
        ProcessorKind::HierarchicalPortPositionProcessor => 40,
        ProcessorKind::ConstraintsPostprocessor => 41,
        ProcessorKind::CommentPostprocessor => 42,
        ProcessorKind::HypernodesProcessor => 43,
        ProcessorKind::HierarchicalPortOrthogonalEdgeRouter => 44,
        ProcessorKind::LongEdgeJoiner => 45,
        ProcessorKind::SelfLoopPostProcessor => 46,
        ProcessorKind::BreakingPointRemover => 47,
        ProcessorKind::NorthSouthPortPostprocessor => 48,
        ProcessorKind::HorizontalGraphCompactor => 49,
        ProcessorKind::LabelDummyRemover => 50,
        ProcessorKind::FinalSplineBendpointsCalculator => 51,
        ProcessorKind::EndLabelSorter => 52,
        ProcessorKind::ReversedEdgeRestorer => 53,
        ProcessorKind::EndLabelPostprocessor => 54,
        ProcessorKind::HierarchicalNodeResizer => 55,
        ProcessorKind::DirectionPostprocessor => 56,
        ProcessorKind::GreedyCycleBreaker
        | ProcessorKind::DepthFirstCycleBreaker
        | ProcessorKind::InteractiveCycleBreaker
        | ProcessorKind::ModelOrderCycleBreaker
        | ProcessorKind::GreedyModelOrderCycleBreaker
        | ProcessorKind::NetworkSimplexLayerer
        | ProcessorKind::LongestPathLayerer
        | ProcessorKind::LongestPathSourceLayerer
        | ProcessorKind::CoffmanGrahamLayerer
        | ProcessorKind::InteractiveLayerer
        | ProcessorKind::StretchWidthLayerer
        | ProcessorKind::MinWidthLayerer
        | ProcessorKind::BreadthFirstModelOrderLayerer
        | ProcessorKind::DepthFirstModelOrderLayerer
        | ProcessorKind::LayerSweepCrossingMinimizerBarycenter
        | ProcessorKind::InteractiveCrossingMinimizer
        | ProcessorKind::NoCrossingMinimizer
        | ProcessorKind::BKNodePlacer
        | ProcessorKind::SimpleNodePlacer
        | ProcessorKind::InteractiveNodePlacer
        | ProcessorKind::LinearSegmentsNodePlacer
        | ProcessorKind::NetworkSimplexPlacer
        | ProcessorKind::OrthogonalEdgeRouter
        | ProcessorKind::PolylineEdgeRouter
        | ProcessorKind::SplineEdgeRouter => usize::MAX,
    }
}

/// Assemble the layered processor list for a graph.
///
/// This follows `GraphConfigurator.prepareGraphForLayout(...)` and the selected phases'
/// `getLayoutProcessorConfiguration(...)` methods. It intentionally returns processor kinds rather
/// than executing them so each Java phase can be ported independently.
pub fn assemble_processors(options: &LayeredOptions) -> Vec<ProcessorSlot> {
    assemble_processors_with_graph_size(options, 0, true)
}

pub fn assemble_processors_for_graph(graph: &LGraph) -> Vec<ProcessorSlot> {
    let options = configured_options(graph);
    assemble_processors_with_graph_size(
        &options,
        graph.layerless_nodes.len(),
        graph.parent_node_id.is_none(),
    )
}

fn is_source_ported_processor(kind: ProcessorKind) -> bool {
    matches!(
        kind,
        ProcessorKind::DirectionPreprocessor
            | ProcessorKind::EdgeAndLayerConstraintEdgeReverser
            | ProcessorKind::InteractiveExternalPortPositioner
            | ProcessorKind::GreedyCycleBreaker
            | ProcessorKind::DepthFirstCycleBreaker
            | ProcessorKind::InteractiveCycleBreaker
            | ProcessorKind::ModelOrderCycleBreaker
            | ProcessorKind::GreedyModelOrderCycleBreaker
            | ProcessorKind::LayerConstraintPreprocessor
            | ProcessorKind::NetworkSimplexLayerer
            | ProcessorKind::LongestPathLayerer
            | ProcessorKind::LongestPathSourceLayerer
            | ProcessorKind::CoffmanGrahamLayerer
            | ProcessorKind::InteractiveLayerer
            | ProcessorKind::StretchWidthLayerer
            | ProcessorKind::MinWidthLayerer
            | ProcessorKind::LabelDummyInserter
            | ProcessorKind::SelfLoopPreProcessor
            | ProcessorKind::LayerConstraintPostprocessor
            | ProcessorKind::LongEdgeSplitter
            | ProcessorKind::PortSideProcessor
            | ProcessorKind::InvertedPortProcessor
            | ProcessorKind::PortListSorter
            | ProcessorKind::SortByInputModelProcessor
            | ProcessorKind::HierarchicalPortConstraintProcessor
            | ProcessorKind::LayerSweepCrossingMinimizerBarycenter
            | ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch
            | ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch
            | ProcessorKind::NoCrossingMinimizer
            | ProcessorKind::InLayerConstraintProcessor
            | ProcessorKind::LabelAndNodeSizeProcessor
            | ProcessorKind::InnermostNodeMarginCalculator
            | ProcessorKind::EndLabelPreprocessor
            | ProcessorKind::LabelSideSelector
            | ProcessorKind::HyperedgeDummyMerger
            | ProcessorKind::HierarchicalPortDummySizeProcessor
            | ProcessorKind::BKNodePlacer
            | ProcessorKind::SimpleNodePlacer
            | ProcessorKind::NetworkSimplexPlacer
            | ProcessorKind::LayerSizeAndGraphHeightCalculator
            | ProcessorKind::HierarchicalPortPositionProcessor
            | ProcessorKind::OrthogonalEdgeRouter
            | ProcessorKind::LongEdgeJoiner
            | ProcessorKind::LabelDummyRemover
            | ProcessorKind::EndLabelSorter
            | ProcessorKind::ReversedEdgeRestorer
            | ProcessorKind::EndLabelPostprocessor
            | ProcessorKind::HierarchicalNodeResizer
            | ProcessorKind::DirectionPostprocessor
            | ProcessorKind::SelfLoopPortRestorer
            | ProcessorKind::SelfLoopRouter
            | ProcessorKind::SelfLoopPostProcessor
            | ProcessorKind::LabelDummySwitcher
            | ProcessorKind::HierarchicalPortOrthogonalEdgeRouter
    )
}

fn validate_ported_processors(processors: &[ProcessorSlot]) -> PipelineResult<()> {
    if let Some(slot) = processors
        .iter()
        .find(|slot| !is_source_ported_processor(slot.kind))
    {
        return Err(PipelineError::UnsupportedProcessor { kind: slot.kind });
    }
    Ok(())
}

fn validate_ported_graph_processors(graph: &LGraph) -> PipelineResult<()> {
    let mut collected = vec![graph];
    let mut search = vec![graph];
    while let Some(current) = search.pop() {
        for nested in current
            .layerless_nodes
            .iter()
            .filter_map(|node| node.nested_graph.as_deref())
        {
            collected.push(nested);
            search.push(nested);
        }
    }
    for current in collected.into_iter().rev() {
        validate_ported_processors(&assemble_processors_for_graph(current))?;
    }
    Ok(())
}

/// Run the assembled processors of a flat graph up to and including the first one `is_last`
/// accepts.
///
/// Tests use this to stop at a phase boundary with exactly the state the full runner has there:
/// the configurator, the edge and layer constraint steps and every other scheduled processor.
#[cfg(test)]
fn run_processors_until_slot(
    graph: &mut LGraph,
    is_last: impl Fn(&ProcessorSlot) -> bool,
) -> PipelineResult<Vec<ProcessorKind>> {
    let mut executed = Vec::new();
    for slot in prepare_single_graph_processors(graph)? {
        execute_processor(graph, slot.kind)?;
        executed.push(slot.kind);
        if is_last(&slot) {
            break;
        }
    }
    Ok(executed)
}

/// Run the assembled processors up to and including `target`.
#[cfg(test)]
pub(crate) fn run_processors_until(
    graph: &mut LGraph,
    target: ProcessorKind,
) -> PipelineResult<Vec<ProcessorKind>> {
    run_processors_until_slot(graph, |slot| slot.kind == target)
}

/// Run the assembled processors up to and including the one that carries `phase`.
#[cfg(test)]
pub(crate) fn run_processors_until_phase(
    graph: &mut LGraph,
    phase: LayeredPhase,
) -> PipelineResult<Vec<ProcessorKind>> {
    run_processors_until_slot(graph, |slot| slot.phase == Some(phase))
}

/// Execute all currently source-ported processors assembled for this graph.
///
/// This is the library equivalent of `ElkLayered.layout(...)`: it uses the same assembled
/// processor list as the compound runner and rejects unsupported processors before mutating
/// the graph.
pub fn execute_ported_processors(graph: &mut LGraph) -> PipelineResult<Vec<ProcessorKind>> {
    let mut executed = Vec::new();
    // The full runner promises to reject a zero seed and unsupported processors without mutating
    // the caller's graph. Both checks only read it. `assemble_processors_for_graph` applies the
    // same option rewrites as the configurator, on a copy of the options.
    check_random_seeds(graph)?;
    validate_ported_processors(&assemble_processors_for_graph(graph))?;
    let processors = prepare_single_graph_processors(graph)?;

    if let Some(mut components) = crate::components::split(graph)? {
        // ComponentsProcessor copies the configured properties shallowly. Its Java Random
        // is one shared object, so later components continue the previous component's stream.
        let mut random = graph.random.clone();
        for component in &mut components {
            component.graph.random = random;
            for slot in &processors {
                execute_processor(&mut component.graph, slot.kind)?;
            }
            random = component.graph.random.clone();
        }
        crate::components::combine(graph, components)?;
        graph.random = random;
        executed.extend(processors.iter().map(|slot| slot.kind));
    } else {
        for slot in processors {
            execute_processor(graph, slot.kind)?;
            executed.push(slot.kind);
        }
    }

    // Compound layout resizes through its scheduled HierarchicalNodeResizer. Flat layout
    // applies ElkLayered.resizeGraph after direction postprocessing and component combination,
    // whether or not a minimum size is set: it recomputes the size from the padding either way.
    if graph.options.hierarchy_handling != super::options::HierarchyHandling::IncludeChildren {
        resize_graph_to_minimum(graph)?;
    }

    Ok(executed)
}

fn prepare_single_graph_processors(graph: &mut LGraph) -> PipelineResult<Vec<ProcessorSlot>> {
    // Processor execution below is root-local, but the source-backed GraphConfigurator walks every
    // nested graph to resolve options and random seeds.
    validate_node_size_minima(graph)?;
    configure_graph_properties(graph)?;
    Ok(assemble_processors_for_graph(graph))
}

fn validate_node_size_minima(graph: &LGraph) -> PipelineResult<()> {
    let mut pending = vec![graph];
    while let Some(graph) = pending.pop() {
        graph.options.validate_node_size_minimum()?;
        pending.extend(
            graph
                .layerless_nodes
                .iter()
                .filter_map(|node| node.nested_graph.as_deref()),
        );
    }
    Ok(())
}

/// Execute the source-backed layered pipeline for a compound graph hierarchy.
///
/// This follows Eclipse ELK's `ElkLayered#hierarchicalLayout(...)` execution shape: collect all
/// nested graphs in bottom-up order, keep a processor cursor for each graph, pause non-root graphs
/// at hierarchy-aware processors, execute those hierarchy-aware processors only at the root graph,
/// and then continue from the deepest graph again. Cross-hierarchy edges are represented as
/// hierarchical external port dummies by the importer and routed by the hierarchical port processors
/// in the same schedule.
pub fn execute_ported_compound_processors(
    graph: &mut LGraph,
) -> PipelineResult<Vec<GraphExecution>> {
    validate_ported_graph_processors(graph)?;
    validate_node_size_minima(graph)?;
    // Check every seed before the preprocessor and the configurator change the caller's graph.
    check_random_seeds(graph)?;
    preprocess_source_ported_compound_graph(graph);
    configure_graph_properties(graph)?;
    review_and_correct_hierarchical_processors(graph)?;

    let mut arena = CompoundGraphArena::new(graph);
    const ROOT_GRAPH: usize = 0;
    // Configuration can change the assembled processor list. Validate in the same official
    // bottom-up order used for execution and public result emission.
    for algorithm in arena.algorithms.iter().rev() {
        validate_ported_processors(&algorithm.processors)?;
    }

    while arena.algorithms[ROOT_GRAPH].next_processor
        < arena.algorithms[ROOT_GRAPH].processors.len()
    {
        execute_compound_graph_round(
            graph,
            &arena.entries,
            &mut arena.graph_slots,
            &mut arena.algorithms,
        )?;
    }

    Ok(arena.into_executions())
}

fn execute_compound_algorithm_until_pause(
    graph: &mut LGraph,
    algorithm: &mut GraphAlgorithm,
    is_root: bool,
    mut parent: Option<ParentGraph<'_>>,
) -> PipelineResult<()> {
    while algorithm.next_processor < algorithm.processors.len() {
        let kind = algorithm.processors[algorithm.next_processor].kind;
        algorithm.next_processor += 1;

        let hierarchy_aware = kind.is_hierarchy_aware();
        if hierarchy_aware && !is_root {
            break;
        }

        if hierarchy_aware {
            execute_hierarchy_aware_processor(graph, kind)?;
        } else {
            execute_processor(graph, kind)?;
        }
        let size = actual_graph_size(graph);
        if kind == ProcessorKind::HierarchicalNodeResizer
            && let Some(parent) = parent.as_mut()
        {
            transfer_nested_graph_layout_to_parent_node(graph, parent.graph, parent.node, size);
        }
        algorithm.execution.processors.push(kind);

        if hierarchy_aware {
            break;
        }
    }

    Ok(())
}

struct ParentGraph<'a> {
    graph: &'a mut LGraph,
    node: usize,
}

/// Owns one round's Rust-only detachment of the compound hierarchy.
///
/// Eclipse ELK keeps nested graph objects attached throughout execution. Rust needs disjoint
/// mutable owners to execute a child and update its parent at the same time, so the round stores
/// every nested graph in an arena slot. Completed children are immediately reattached before their
/// parent executes. Dropping the guard restores every still-detached graph in child-before-parent
/// order after an error or unwind.
struct DetachedCompoundGraphs<'a> {
    root: &'a mut LGraph,
    entries: &'a [GraphArenaEntry],
    slots: &'a mut [Option<Box<LGraph>>],
}

impl<'a> DetachedCompoundGraphs<'a> {
    fn new(
        root: &'a mut LGraph,
        entries: &'a [GraphArenaEntry],
        slots: &'a mut [Option<Box<LGraph>>],
    ) -> Self {
        assert_eq!(
            slots.len(),
            entries.len(),
            "the reusable graph slots must match the arena topology"
        );
        assert!(
            slots.iter().all(Option::is_none),
            "all compound graph slots must be empty between rounds"
        );

        Self {
            root,
            entries,
            slots,
        }
    }

    fn detach_all(&mut self) {
        // Arena discovery always assigns a parent before its children. Detach in that order so a
        // nested graph is still reachable from either the root or its parent's occupied slot.
        for graph_index in 1..self.entries.len() {
            let parent = self.entries[graph_index]
                .parent
                .expect("every non-root graph must have an arena parent");
            let graph = if parent.graph == 0 {
                self.root.layerless_nodes[parent.node].nested_graph.take()
            } else {
                self.slots[parent.graph]
                    .as_deref_mut()
                    .expect("a parent graph must remain detached until its children complete")
                    .layerless_nodes[parent.node]
                    .nested_graph
                    .take()
            }
            .expect("the compound graph topology must remain stable during layered execution");
            self.slots[graph_index] = Some(graph);
        }
    }

    fn execute_and_reattach(
        &mut self,
        graph_index: usize,
        algorithm: &mut GraphAlgorithm,
    ) -> PipelineResult<()> {
        let parent = self.entries[graph_index]
            .parent
            .expect("only nested graphs use detached execution");
        if parent.graph == 0 {
            let result = {
                let graph = self.slots[graph_index]
                    .as_deref_mut()
                    .expect("the nested graph must occupy its arena slot");
                execute_compound_algorithm_until_pause(
                    graph,
                    algorithm,
                    false,
                    Some(ParentGraph {
                        graph: self.root,
                        node: parent.node,
                    }),
                )
            };
            let graph = self.slots[graph_index]
                .take()
                .expect("the completed graph must still occupy its arena slot");
            self.root.layerless_nodes[parent.node].nested_graph = Some(graph);
            return result;
        }

        debug_assert!(parent.graph < graph_index);
        let result = {
            let (parents, current_and_later) = self.slots.split_at_mut(graph_index);
            let parent_graph = parents[parent.graph]
                .as_deref_mut()
                .expect("the parent graph must remain detached until its turn");
            let graph = current_and_later[0]
                .as_deref_mut()
                .expect("the nested graph must occupy its arena slot");
            execute_compound_algorithm_until_pause(
                graph,
                algorithm,
                false,
                Some(ParentGraph {
                    graph: parent_graph,
                    node: parent.node,
                }),
            )
        };

        let graph = self.slots[graph_index]
            .take()
            .expect("the completed graph must still occupy its arena slot");
        self.slots[parent.graph]
            .as_deref_mut()
            .expect("the parent graph must remain detached until its turn")
            .layerless_nodes[parent.node]
            .nested_graph = Some(graph);
        result
    }

    fn restore_slot(&mut self, graph_index: usize) {
        let Some(graph) = self.slots[graph_index].take() else {
            return;
        };
        let parent = self.entries[graph_index]
            .parent
            .expect("every non-root graph must have an arena parent");
        if parent.graph == 0 {
            self.root.layerless_nodes[parent.node].nested_graph = Some(graph);
        } else {
            self.slots[parent.graph]
                .as_deref_mut()
                .expect("a detached child must still have a detached parent during restoration")
                .layerless_nodes[parent.node]
                .nested_graph = Some(graph);
        }
    }
}

impl Drop for DetachedCompoundGraphs<'_> {
    fn drop(&mut self) {
        for graph_index in (1..self.entries.len()).rev() {
            self.restore_slot(graph_index);
        }
    }
}

fn execute_compound_graph_round(
    root: &mut LGraph,
    entries: &[GraphArenaEntry],
    graph_slots: &mut [Option<Box<LGraph>>],
    algorithms: &mut [GraphAlgorithm],
) -> PipelineResult<()> {
    let mut detached = DetachedCompoundGraphs::new(root, entries, graph_slots);
    detached.detach_all();
    for graph_index in (1..algorithms.len()).rev() {
        detached.execute_and_reattach(graph_index, &mut algorithms[graph_index])?;
    }
    execute_compound_algorithm_until_pause(detached.root, &mut algorithms[0], true, None)
}

fn execute_hierarchy_aware_processor(
    graph: &mut LGraph,
    kind: ProcessorKind,
) -> PipelineResult<()> {
    match kind {
        ProcessorKind::LayerSweepCrossingMinimizerBarycenter => {
            minimize_crossings_layer_sweep_hierarchical_with_type(graph, CrossMinType::Barycenter);
        }
        ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch => {
            minimize_crossings_layer_sweep_hierarchical_with_type(
                graph,
                CrossMinType::OneSidedGreedySwitch,
            );
        }
        ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch => {
            minimize_crossings_layer_sweep_hierarchical_with_type(
                graph,
                CrossMinType::TwoSidedGreedySwitch,
            );
        }
        _ => return Err(PipelineError::UnsupportedProcessor { kind }),
    }
    Ok(())
}

fn review_and_correct_hierarchical_processors(root: &mut LGraph) -> PipelineResult<()> {
    let root_crossing = root.options.crossing_minimization_strategy;
    let root_greedy_switch = root.options.greedy_switch_hierarchical_type;
    root.try_for_each_graph_mut(|graph| {
        if graph.options.crossing_minimization_strategy != root_crossing {
            return Err(PipelineError::UnsupportedCompoundGraph {
                reason: "child graphs must use the root hierarchy-aware crossing minimizer",
            });
        }
        graph.options.greedy_switch_hierarchical_type = root_greedy_switch;
        Ok(())
    })
}

fn transfer_nested_graph_layout_to_parent_node(
    nested_graph: &mut LGraph,
    parent: &mut LGraph,
    node_index: usize,
    size: LSize,
) {
    let has_external_ports = {
        let node = &mut parent.layerless_nodes[node_index];
        transfer_external_port_dummy_layout_to_parent_node(
            nested_graph,
            node_index,
            &mut node.ports,
        );
        nested_graph.graph_properties.external_ports
    };

    {
        let node = &mut parent.layerless_nodes[node_index];
        if has_external_ports {
            node.port_constraints = PortConstraints::FixedPos;
            resize_layered_node(node, size, false, true);
        } else {
            resize_layered_node(node, size, true, true);
        }
    }

    if has_external_ports {
        parent.graph_properties.non_free_ports = true;
    }
}

fn actual_graph_size(graph: &LGraph) -> LSize {
    LSize {
        width: graph.size.width + graph.padding.left + graph.padding.right,
        height: graph.size.height + graph.padding.top + graph.padding.bottom,
    }
}

fn transfer_external_port_dummy_layout_to_parent_node(
    nested_graph: &mut LGraph,
    parent_node_index: usize,
    parent_ports: &mut [crate::graph::LPort],
) {
    for dummy_index in 0..nested_graph.layerless_nodes.len() {
        if nested_graph.layerless_nodes[dummy_index].kind != LNodeKind::ExternalPort {
            continue;
        }
        let Some(origin_port) = nested_graph.layerless_nodes[dummy_index]
            .origin_port
            .clone()
        else {
            continue;
        };
        if origin_port.port.node != parent_node_index {
            continue;
        }

        let port_position = external_port_position(nested_graph, dummy_index);
        let external_side = nested_graph.layerless_nodes[dummy_index].external_port_side;
        if let Some(parent_port) = parent_ports.get_mut(origin_port.port.port) {
            parent_port.position = port_position;
            parent_port.set_side(external_side);
        }
    }
}

fn external_port_position(graph: &mut LGraph, dummy_index: usize) -> LPoint {
    let dummy_size = graph.layerless_nodes[dummy_index].size;
    let external_size = graph.layerless_nodes[dummy_index].external_port_size;
    let external_side = graph.layerless_nodes[dummy_index].external_port_side;
    let border_offset = graph.layerless_nodes[dummy_index]
        .ports
        .first()
        .and_then(|port| port.border_offset)
        .unwrap_or(0.0);

    let mut port_position = LPoint {
        x: graph.layerless_nodes[dummy_index].position.x + dummy_size.width / 2.0,
        y: graph.layerless_nodes[dummy_index].position.y + dummy_size.height / 2.0,
    };

    match external_side {
        PortSide::North => {
            port_position.x += graph.padding.left + graph.offset.x - external_size.width / 2.0;
            port_position.y = -external_size.height - border_offset;
            graph.layerless_nodes[dummy_index].position.y =
                -(graph.padding.top + border_offset + graph.offset.y);
        }
        PortSide::East => {
            port_position.x =
                graph.size.width + graph.padding.left + graph.padding.right + border_offset;
            port_position.y += graph.padding.top + graph.offset.y - external_size.height / 2.0;
            graph.layerless_nodes[dummy_index].position.x =
                graph.size.width + graph.padding.right + border_offset - graph.offset.x;
        }
        PortSide::South => {
            port_position.x += graph.padding.left + graph.offset.x - external_size.width / 2.0;
            port_position.y =
                graph.size.height + graph.padding.top + graph.padding.bottom + border_offset;
            graph.layerless_nodes[dummy_index].position.y =
                graph.size.height + graph.padding.bottom + border_offset - graph.offset.y;
        }
        PortSide::West => {
            port_position.x = -external_size.width - border_offset;
            port_position.y += graph.padding.top + graph.offset.y - external_size.height / 2.0;
            graph.layerless_nodes[dummy_index].position.x =
                -(graph.padding.left + border_offset + graph.offset.x);
        }
        PortSide::Undefined => {}
    }

    port_position
}

fn execute_processor(graph: &mut LGraph, kind: ProcessorKind) -> PipelineResult<()> {
    match kind {
        ProcessorKind::DirectionPreprocessor => {
            transform_graph_direction(graph, GraphTransformMode::ToInputDirection);
        }
        ProcessorKind::DirectionPostprocessor => {
            transform_graph_direction(graph, GraphTransformMode::ToInternalLeftToRight);
        }
        ProcessorKind::EdgeAndLayerConstraintEdgeReverser => {
            reverse_edges_for_edge_and_layer_constraints(graph);
        }
        ProcessorKind::InteractiveExternalPortPositioner => {
            position_interactive_external_ports(graph);
        }
        ProcessorKind::SelfLoopPreProcessor => preprocess_self_loops(graph),
        ProcessorKind::GreedyCycleBreaker => break_cycles_greedy(graph),
        ProcessorKind::DepthFirstCycleBreaker => break_cycles_depth_first(graph),
        ProcessorKind::InteractiveCycleBreaker => break_cycles_interactive(graph),
        ProcessorKind::ModelOrderCycleBreaker => break_cycles_model_order(graph),
        ProcessorKind::GreedyModelOrderCycleBreaker => break_cycles_greedy_model_order(graph),
        ProcessorKind::LayerConstraintPreprocessor => preprocess_layer_constraints(graph)?,
        ProcessorKind::LabelDummyInserter => insert_label_dummies(graph),
        ProcessorKind::NetworkSimplexLayerer => layer_network_simplex(graph)?,
        ProcessorKind::LongestPathLayerer => {
            layer_with_strategy(graph, LayeringStrategy::LongestPath)?
        }
        ProcessorKind::LongestPathSourceLayerer => {
            layer_with_strategy(graph, LayeringStrategy::LongestPathSource)?
        }
        ProcessorKind::CoffmanGrahamLayerer => {
            layer_with_strategy(graph, LayeringStrategy::CoffmanGraham)?
        }
        ProcessorKind::InteractiveLayerer => {
            layer_with_strategy(graph, LayeringStrategy::Interactive)?
        }
        ProcessorKind::StretchWidthLayerer => {
            layer_with_strategy(graph, LayeringStrategy::StretchWidth)?
        }
        ProcessorKind::MinWidthLayerer => layer_with_strategy(graph, LayeringStrategy::MinWidth)?,
        ProcessorKind::LayerConstraintPostprocessor => postprocess_layer_constraints(graph)?,
        ProcessorKind::HierarchicalPortConstraintProcessor => {
            process_hierarchical_port_constraints(graph);
        }
        ProcessorKind::LongEdgeSplitter => split_long_edges(graph),
        ProcessorKind::PortSideProcessor => process_port_sides(graph),
        ProcessorKind::InvertedPortProcessor => process_inverted_ports(graph),
        ProcessorKind::PortListSorter => sort_port_lists(graph),
        ProcessorKind::SortByInputModelProcessor => sort_by_input_model(graph),
        ProcessorKind::LayerSweepCrossingMinimizerBarycenter => {
            minimize_crossings_layer_sweep_with_type(graph, CrossMinType::Barycenter);
        }
        ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch => {
            minimize_crossings_layer_sweep_with_type(graph, CrossMinType::OneSidedGreedySwitch);
        }
        ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch => {
            minimize_crossings_layer_sweep_with_type(graph, CrossMinType::TwoSidedGreedySwitch);
        }
        ProcessorKind::SelfLoopPortRestorer => restore_self_loop_ports(graph),
        ProcessorKind::InLayerConstraintProcessor => process_in_layer_constraints(graph),
        ProcessorKind::LabelAndNodeSizeProcessor => calculate_label_and_node_sizes(graph),
        ProcessorKind::InnermostNodeMarginCalculator => calculate_innermost_node_margins(graph),
        ProcessorKind::SelfLoopRouter => route_self_loops(graph),
        ProcessorKind::LabelDummySwitcher => switch_label_dummies(graph),
        ProcessorKind::LabelSideSelector => select_label_sides(graph),
        ProcessorKind::HyperedgeDummyMerger => merge_hyperedge_dummies(graph),
        ProcessorKind::EndLabelPreprocessor => preprocess_end_labels(graph),
        ProcessorKind::BKNodePlacer => place_nodes_brandes_koepf(graph),
        ProcessorKind::SimpleNodePlacer => place_nodes_simple(graph),
        ProcessorKind::NetworkSimplexPlacer => place_nodes_network_simplex(graph)?,
        ProcessorKind::LayerSizeAndGraphHeightCalculator => {
            calculate_layer_sizes_and_graph_height(graph);
        }
        ProcessorKind::HierarchicalPortDummySizeProcessor => {
            process_hierarchical_port_dummy_sizes(graph);
        }
        ProcessorKind::HierarchicalPortPositionProcessor => {
            process_hierarchical_port_positions(graph);
        }
        ProcessorKind::OrthogonalEdgeRouter => route_edges_orthogonal(graph),
        ProcessorKind::HierarchicalPortOrthogonalEdgeRouter => {
            process_hierarchical_port_orthogonal_edges(graph);
        }
        ProcessorKind::LongEdgeJoiner => join_long_edges(graph),
        ProcessorKind::SelfLoopPostProcessor => postprocess_self_loops(graph),
        ProcessorKind::LabelDummyRemover => remove_label_dummies(graph),
        ProcessorKind::EndLabelSorter => sort_end_labels(graph),
        ProcessorKind::ReversedEdgeRestorer => restore_reversed_edges(graph),
        ProcessorKind::EndLabelPostprocessor => postprocess_end_labels(graph),
        ProcessorKind::HierarchicalNodeResizer => resize_hierarchical_node_graph(graph)?,
        ProcessorKind::NoCrossingMinimizer => {}
        _ => return Err(PipelineError::UnsupportedProcessor { kind }),
    }

    Ok(())
}

fn resize_hierarchical_node_graph(graph: &mut LGraph) -> PipelineResult<()> {
    let layered_nodes = graph
        .layers
        .iter()
        .flat_map(|layer| layer.nodes.iter().copied())
        .collect::<Vec<_>>();
    for node in layered_nodes {
        graph.layerless_nodes[node].layer_index = None;
    }
    graph.layers.clear();
    resize_graph_to_minimum(graph)
}

fn resize_graph_to_minimum(graph: &mut LGraph) -> PipelineResult<()> {
    graph.options.validate_node_size_minimum()?;
    let old_size = actual_graph_size(graph);
    let mut new_size = old_size;
    if let Some(minimum) = graph.options.effective_node_size_minimum() {
        new_size.width = new_size.width.max(minimum.width);
        new_size.height = new_size.height.max(minimum.height);
    }
    resize_graph_no_really_i_mean_it(graph, old_size, new_size);
    Ok(())
}

fn resize_graph_no_really_i_mean_it(graph: &mut LGraph, old_size: LSize, new_size: LSize) {
    fn alignment_offset(alignment: ContentAlignment, old: f64, new: f64) -> f64 {
        if new <= old {
            return 0.0;
        }
        match alignment {
            ContentAlignment::Start => 0.0,
            ContentAlignment::Center => (new - old) / 2.0,
            ContentAlignment::End => new - old,
        }
    }
    graph.offset.x += alignment_offset(
        graph.options.horizontal_content_alignment,
        old_size.width,
        new_size.width,
    );
    graph.offset.y += alignment_offset(
        graph.options.vertical_content_alignment,
        old_size.height,
        new_size.height,
    );
    if graph.graph_properties.external_ports
        && (new_size.width > old_size.width || new_size.height > old_size.height)
    {
        for node in &mut graph.layerless_nodes {
            if node.kind != LNodeKind::ExternalPort {
                continue;
            }
            match node.external_port_side {
                PortSide::East => node.position.x += new_size.width - old_size.width,
                PortSide::South => node.position.y += new_size.height - old_size.height,
                PortSide::North | PortSide::West | PortSide::Undefined => {}
            }
        }
    }

    graph.size.width = new_size.width - graph.padding.left - graph.padding.right;
    graph.size.height = new_size.height - graph.padding.top - graph.padding.bottom;
}

fn resize_layered_node(node: &mut LNode, new_size: LSize, move_ports: bool, move_labels: bool) {
    let old_size = node.size;
    let width_ratio = ratio_or_one(new_size.width, old_size.width);
    let height_ratio = ratio_or_one(new_size.height, old_size.height);
    let width_diff = new_size.width - old_size.width;
    let height_diff = new_size.height - old_size.height;

    if move_ports {
        let fixed_ports = node.port_constraints == PortConstraints::FixedPos;
        for port in &mut node.ports {
            match port.side {
                PortSide::North => {
                    if !fixed_ports {
                        port.position.x *= width_ratio;
                    }
                }
                PortSide::East => {
                    port.position.x += width_diff;
                    if !fixed_ports {
                        port.position.y *= height_ratio;
                    }
                }
                PortSide::South => {
                    if !fixed_ports {
                        port.position.x *= width_ratio;
                    }
                    port.position.y += height_diff;
                }
                PortSide::West => {
                    if !fixed_ports {
                        port.position.y *= height_ratio;
                    }
                }
                PortSide::Undefined => {}
            }
        }
    }

    if move_labels {
        for label in &mut node.labels {
            let mid_x = label.position.x + label.size.width / 2.0;
            let mid_y = label.position.y + label.size.height / 2.0;
            let width_percent = ratio_or_zero(mid_x, old_size.width);
            let height_percent = ratio_or_zero(mid_y, old_size.height);

            if width_percent + height_percent >= 1.0 {
                if width_percent - height_percent > 0.0 && mid_y >= 0.0 {
                    label.position.x += width_diff;
                    label.position.y += height_diff * height_percent;
                } else if width_percent - height_percent < 0.0 && mid_x >= 0.0 {
                    label.position.x += width_diff * width_percent;
                    label.position.y += height_diff;
                }
            }
        }
    }

    node.size = new_size;
    // `LGraphUtil.resizeNode` fixes the size: later processors keep it.
    node.node_size.constraints = crate::options::SizeConstraints::FIXED;
}

fn ratio_or_one(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        1.0
    } else {
        numerator / denominator
    }
}

fn ratio_or_zero(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

fn assemble_processors_with_graph_size(
    options: &LayeredOptions,
    graph_size: usize,
    is_root_graph: bool,
) -> Vec<ProcessorSlot> {
    let mut config = Config::default();

    add_baseline_processors(&mut config);
    add_graph_configurator_processors(&mut config, options, graph_size, is_root_graph);

    let cycle = cycle_breaking_processor(options.cycle_breaking_strategy);
    config.merge(cycle_breaking_dependencies(cycle));
    config.add_phase(LayeredPhase::P1CycleBreaking, cycle);

    let layerer = layering_processor(options.layering_strategy);
    config.merge(layering_dependencies(layerer));
    config.add_phase(LayeredPhase::P2Layering, layerer);

    let crossing = crossing_minimization_processor(options.crossing_minimization_strategy);
    config.merge(crossing_minimization_dependencies(crossing));
    config.add_phase(LayeredPhase::P3NodeOrdering, crossing);

    let node_placer = node_placement_processor(options.node_placement_strategy);
    config.merge(node_placement_dependencies(node_placer));
    config.add_phase(LayeredPhase::P4NodePlacement, node_placer);

    let edge_router = edge_routing_processor(options.edge_routing);
    config.merge(edge_routing_dependencies(options, edge_router));
    config.add_phase(LayeredPhase::P5EdgeRouting, edge_router);

    config.into_slots()
}

fn add_baseline_processors(config: &mut Config) {
    config.add_before(
        LayeredPhase::P4NodePlacement,
        ProcessorKind::InnermostNodeMarginCalculator,
    );
    config.add_before(
        LayeredPhase::P4NodePlacement,
        ProcessorKind::LabelAndNodeSizeProcessor,
    );
    config.add_before(
        LayeredPhase::P5EdgeRouting,
        ProcessorKind::LayerSizeAndGraphHeightCalculator,
    );
    config.add_after(LayeredPhase::P5EdgeRouting, ProcessorKind::EndLabelSorter);
}

fn add_graph_configurator_processors(
    config: &mut Config,
    options: &LayeredOptions,
    graph_size: usize,
    is_root_graph: bool,
) {
    if options.hierarchy_handling == super::options::HierarchyHandling::IncludeChildren {
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::HierarchicalNodeResizer,
        );
    }

    let port_side_phase = if options.feedback_edges {
        LayeredPhase::P1CycleBreaking
    } else {
        LayeredPhase::P3NodeOrdering
    };
    config.add_before(port_side_phase, ProcessorKind::PortSideProcessor);

    match options.direction {
        ElkDirection::Left | ElkDirection::Down | ElkDirection::Up => {
            config.add_before(
                LayeredPhase::P1CycleBreaking,
                ProcessorKind::DirectionPreprocessor,
            );
            config.add_after(
                LayeredPhase::P5EdgeRouting,
                ProcessorKind::DirectionPostprocessor,
            );
        }
        ElkDirection::Right | ElkDirection::Undefined => {}
    }

    if activate_greedy_switch_for(options, graph_size, is_root_graph) {
        let kind = if options.is_hierarchical_layout() {
            match options.greedy_switch_hierarchical_type {
                GreedySwitchType::OneSided => {
                    ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch
                }
                GreedySwitchType::TwoSided => {
                    ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch
                }
                GreedySwitchType::Off => unreachable!("checked by activate_greedy_switch_for"),
            }
        } else {
            match options.greedy_switch_type {
                GreedySwitchType::OneSided => {
                    ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch
                }
                GreedySwitchType::TwoSided => {
                    ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch
                }
                GreedySwitchType::Off => unreachable!("checked by activate_greedy_switch_for"),
            }
        };
        config.add_before(LayeredPhase::P4NodePlacement, kind);
    }

    match options.wrapping_strategy {
        WrappingStrategy::SingleEdge => {
            config.add_before(
                LayeredPhase::P4NodePlacement,
                ProcessorKind::SingleEdgeGraphWrapper,
            );
        }
        WrappingStrategy::MultiEdge => {
            config.add_before(
                LayeredPhase::P3NodeOrdering,
                ProcessorKind::BreakingPointInserter,
            );
            config.add_before(
                LayeredPhase::P4NodePlacement,
                ProcessorKind::BreakingPointProcessor,
            );
            config.add_after(
                LayeredPhase::P5EdgeRouting,
                ProcessorKind::BreakingPointRemover,
            );
        }
        WrappingStrategy::Off => {}
    }

    if options.consider_model_order_strategy != OrderingStrategy::None {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            ProcessorKind::SortByInputModelProcessor,
        );
    }
}

fn activate_greedy_switch_for(
    options: &LayeredOptions,
    graph_size: usize,
    is_root_graph: bool,
) -> bool {
    if options.is_hierarchical_layout() {
        return is_root_graph && options.greedy_switch_hierarchical_type != GreedySwitchType::Off;
    }

    let interactive_cross_min =
        options.crossing_minimization_strategy == CrossingMinimizationStrategy::Interactive;
    !interactive_cross_min
        && options.greedy_switch_type != GreedySwitchType::Off
        && (options.greedy_switch_activation_threshold == 0
            || options.greedy_switch_activation_threshold > graph_size)
}

fn cycle_breaking_processor(strategy: CycleBreakingStrategy) -> ProcessorKind {
    match strategy {
        CycleBreakingStrategy::Greedy => ProcessorKind::GreedyCycleBreaker,
        CycleBreakingStrategy::DepthFirst => ProcessorKind::DepthFirstCycleBreaker,
        CycleBreakingStrategy::Interactive => ProcessorKind::InteractiveCycleBreaker,
        CycleBreakingStrategy::ModelOrder => ProcessorKind::ModelOrderCycleBreaker,
        CycleBreakingStrategy::GreedyModelOrder => ProcessorKind::GreedyModelOrderCycleBreaker,
    }
}

fn cycle_breaking_dependencies(processor: ProcessorKind) -> Config {
    let mut config = Config::default();
    if processor == ProcessorKind::InteractiveCycleBreaker {
        config.add_before(
            LayeredPhase::P1CycleBreaking,
            ProcessorKind::InteractiveExternalPortPositioner,
        );
    }
    config.add_after(
        LayeredPhase::P5EdgeRouting,
        ProcessorKind::ReversedEdgeRestorer,
    );
    config
}

fn layering_processor(strategy: LayeringStrategy) -> ProcessorKind {
    match strategy {
        LayeringStrategy::NetworkSimplex => ProcessorKind::NetworkSimplexLayerer,
        LayeringStrategy::LongestPath => ProcessorKind::LongestPathLayerer,
        LayeringStrategy::LongestPathSource => ProcessorKind::LongestPathSourceLayerer,
        LayeringStrategy::CoffmanGraham => ProcessorKind::CoffmanGrahamLayerer,
        LayeringStrategy::Interactive => ProcessorKind::InteractiveLayerer,
        LayeringStrategy::StretchWidth => ProcessorKind::StretchWidthLayerer,
        LayeringStrategy::MinWidth => ProcessorKind::MinWidthLayerer,
        LayeringStrategy::BreadthFirstModelOrder => ProcessorKind::BreadthFirstModelOrderLayerer,
        LayeringStrategy::DepthFirstModelOrder => ProcessorKind::DepthFirstModelOrderLayerer,
    }
}

fn layering_dependencies(processor: ProcessorKind) -> Config {
    let mut config = Config::default();
    config.add_before(
        LayeredPhase::P1CycleBreaking,
        if processor == ProcessorKind::InteractiveLayerer {
            ProcessorKind::InteractiveExternalPortPositioner
        } else {
            ProcessorKind::EdgeAndLayerConstraintEdgeReverser
        },
    );
    config.add_before(
        LayeredPhase::P2Layering,
        ProcessorKind::LayerConstraintPreprocessor,
    );
    config.add_before(
        LayeredPhase::P3NodeOrdering,
        ProcessorKind::LayerConstraintPostprocessor,
    );
    config
}

fn crossing_minimization_processor(strategy: CrossingMinimizationStrategy) -> ProcessorKind {
    match strategy {
        CrossingMinimizationStrategy::LayerSweep => {
            ProcessorKind::LayerSweepCrossingMinimizerBarycenter
        }
        CrossingMinimizationStrategy::Interactive => ProcessorKind::InteractiveCrossingMinimizer,
        CrossingMinimizationStrategy::None => ProcessorKind::NoCrossingMinimizer,
    }
}

fn crossing_minimization_dependencies(_processor: ProcessorKind) -> Config {
    let mut config = Config::default();
    config.add_before(
        LayeredPhase::P3NodeOrdering,
        ProcessorKind::LongEdgeSplitter,
    );
    config.add_before(LayeredPhase::P3NodeOrdering, ProcessorKind::PortListSorter);
    config.add_before(
        LayeredPhase::P4NodePlacement,
        ProcessorKind::InLayerConstraintProcessor,
    );
    config.add_after(LayeredPhase::P5EdgeRouting, ProcessorKind::LongEdgeJoiner);
    config
}

fn node_placement_processor(strategy: NodePlacementStrategy) -> ProcessorKind {
    match strategy {
        NodePlacementStrategy::Simple => ProcessorKind::SimpleNodePlacer,
        NodePlacementStrategy::Interactive => ProcessorKind::InteractiveNodePlacer,
        NodePlacementStrategy::LinearSegments => ProcessorKind::LinearSegmentsNodePlacer,
        NodePlacementStrategy::BrandesKoepf => ProcessorKind::BKNodePlacer,
        NodePlacementStrategy::NetworkSimplex => ProcessorKind::NetworkSimplexPlacer,
    }
}

fn node_placement_dependencies(_processor: ProcessorKind) -> Config {
    Config::default()
}

fn edge_routing_processor(strategy: EdgeRouting) -> ProcessorKind {
    match strategy {
        EdgeRouting::Polyline => ProcessorKind::PolylineEdgeRouter,
        EdgeRouting::Orthogonal => ProcessorKind::OrthogonalEdgeRouter,
        EdgeRouting::Splines => ProcessorKind::SplineEdgeRouter,
    }
}

fn edge_routing_dependencies(options: &LayeredOptions, _processor: ProcessorKind) -> Config {
    let mut config = Config::default();
    if options.graph_has_hyperedges {
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::HyperedgeDummyMerger,
        );
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            ProcessorKind::InvertedPortProcessor,
        );
    }
    if options.graph_has_non_free_ports || options.feedback_edges {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            ProcessorKind::InvertedPortProcessor,
        );
        if options.graph_has_north_south_ports {
            config.add_before(
                LayeredPhase::P3NodeOrdering,
                ProcessorKind::NorthSouthPortPreprocessor,
            );
            config.add_after(
                LayeredPhase::P5EdgeRouting,
                ProcessorKind::NorthSouthPortPostprocessor,
            );
        }
    }
    if options.graph_has_external_ports {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            ProcessorKind::HierarchicalPortConstraintProcessor,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::HierarchicalPortDummySizeProcessor,
        );
        config.add_before(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::HierarchicalPortPositionProcessor,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::HierarchicalPortOrthogonalEdgeRouter,
        );
    }
    if options.graph_has_self_loops {
        config.add_before(
            LayeredPhase::P1CycleBreaking,
            ProcessorKind::SelfLoopPreProcessor,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::SelfLoopPortRestorer,
        );
        config.add_before(LayeredPhase::P4NodePlacement, ProcessorKind::SelfLoopRouter);
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::SelfLoopPostProcessor,
        );
    }
    if options.graph_has_hypernodes {
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::HypernodesProcessor,
        );
    }
    if options.graph_has_center_labels {
        config.add_before(LayeredPhase::P2Layering, ProcessorKind::LabelDummyInserter);
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::LabelDummySwitcher,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::LabelSideSelector,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::LabelDummyRemover,
        );
    }
    if options.graph_has_end_labels {
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::LabelSideSelector,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            ProcessorKind::EndLabelPreprocessor,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            ProcessorKind::EndLabelPostprocessor,
        );
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LNode, LNodeKind, PortSide, PortType};
    use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputLabel, ElkInputNode, import_graph};
    use crate::options::{
        CrossingMinimizationStrategy, CycleBreakingStrategy, EdgeRouting, ElkDirection,
        FixedAlignment, GreedySwitchType, LayeredOptions, LayeringStrategy, NodePlacementStrategy,
        OrderingStrategy, PortConstraints, WrappingStrategy,
    };

    fn kinds(options: &LayeredOptions) -> Vec<ProcessorKind> {
        assemble_processors(options)
            .into_iter()
            .map(|slot| slot.kind)
            .collect()
    }

    fn graph_kinds(graph: &LGraph) -> Vec<ProcessorKind> {
        assemble_processors_for_graph(graph)
            .into_iter()
            .map(|slot| slot.kind)
            .collect()
    }

    fn assert_processors_are_source_ported(case: &str, processors: Vec<ProcessorKind>) {
        let unsupported = processors
            .into_iter()
            .filter(|kind| !is_source_ported_processor(*kind))
            .collect::<Vec<_>>();

        assert!(
            unsupported.is_empty(),
            "{case} reached unported ELK processors: {unsupported:?}"
        );
    }

    fn node(id: &str) -> ElkInputNode {
        ElkInputNode {
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
        }
    }

    fn p3_options() -> LayeredOptions {
        LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        }
    }

    /// The processors the assembler schedules up to and including the end of `phase`.
    fn kinds_through_phase(options: &LayeredOptions, phase: LayeredPhase) -> Vec<ProcessorKind> {
        let slots = assemble_processors(options);
        let end = slots
            .iter()
            .position(|slot| slot.phase == Some(phase))
            .unwrap();
        slots[..=end].iter().map(|slot| slot.kind).collect()
    }

    fn deep_compound_chain(graph_count: usize) -> LGraph {
        assert!(graph_count > 0);
        let options = LayeredOptions {
            hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
            greedy_switch_type: GreedySwitchType::Off,
            greedy_switch_hierarchical_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        };
        let mut nested = None;
        for depth in (0..graph_count).rev() {
            let mut graph = LGraph::new(format!("graph-{depth}"), options.clone());
            if depth > 0 {
                graph.parent_node_id = Some(format!("node-{}", depth - 1));
            }
            if let Some(child) = nested.take() {
                let mut holder = LNode::new(format!("node-{depth}"), 1.0, 1.0, None);
                holder.compound = true;
                holder.nested_graph = Some(child);
                graph.layerless_nodes.push(holder);
            }
            nested = Some(Box::new(graph));
        }
        *nested.expect("a positive graph count creates the root graph")
    }

    fn branched_compound_graph() -> LGraph {
        let options = LayeredOptions {
            hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
            greedy_switch_type: GreedySwitchType::Off,
            greedy_switch_hierarchical_type: GreedySwitchType::Off,
            ..LayeredOptions::default()
        };

        let mut first_grandchild = LGraph::new("first-grandchild", options.clone());
        first_grandchild.parent_node_id = Some("first-grandchild-holder".to_string());
        let mut first_child = LGraph::new("first-child", options.clone());
        first_child.parent_node_id = Some("first-holder".to_string());
        let mut first_grandchild_holder = LNode::new("first-grandchild-holder", 1.0, 1.0, None);
        first_grandchild_holder.compound = true;
        first_grandchild_holder.nested_graph = Some(Box::new(first_grandchild));
        first_child.layerless_nodes.push(first_grandchild_holder);

        let mut second_grandchild = LGraph::new("second-grandchild", options.clone());
        second_grandchild.parent_node_id = Some("second-grandchild-holder".to_string());
        let mut second_child = LGraph::new("second-child", options.clone());
        second_child.parent_node_id = Some("second-holder".to_string());
        let mut second_grandchild_holder = LNode::new("second-grandchild-holder", 1.0, 1.0, None);
        second_grandchild_holder.compound = true;
        second_grandchild_holder.nested_graph = Some(Box::new(second_grandchild));
        second_child.layerless_nodes.push(second_grandchild_holder);

        let mut root = LGraph::new("root", options);
        let mut first_holder = LNode::new("first-holder", 1.0, 1.0, None);
        first_holder.compound = true;
        first_holder.nested_graph = Some(Box::new(first_child));
        let mut second_holder = LNode::new("second-holder", 1.0, 1.0, None);
        second_holder.compound = true;
        second_holder.nested_graph = Some(Box::new(second_child));
        root.layerless_nodes.extend([first_holder, second_holder]);
        root
    }

    #[test]
    fn compound_arena_stores_one_parent_and_reusable_slot_per_graph() {
        for graph_count in [8usize, 16, 32, 64, 128] {
            let graph = deep_compound_chain(graph_count);
            let arena = CompoundGraphArena::new(&graph);
            let parent_links = arena
                .entries
                .iter()
                .filter(|entry| entry.parent.is_some())
                .count();

            assert_eq!(arena.entries.len(), graph_count);
            assert_eq!(arena.algorithms.len(), graph_count);
            assert_eq!(arena.graph_slots.len(), graph_count);
            assert!(arena.graph_slots.iter().all(Option::is_none));
            assert_eq!(parent_links, graph_count - 1);
        }
    }

    #[test]
    fn compound_arena_preserves_official_reverse_discovery_order() {
        let root = branched_compound_graph();
        let arena = CompoundGraphArena::new(&root);
        let graph_ids = arena
            .algorithms
            .iter()
            .rev()
            .map(|algorithm| algorithm.execution.graph_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            graph_ids,
            [
                "first-grandchild",
                "second-grandchild",
                "second-child",
                "first-child",
                "root",
            ]
        );
    }

    #[test]
    fn public_compound_execution_reports_official_reverse_discovery_order() {
        let mut root = branched_compound_graph();
        let executions = execute_ported_compound_processors(&mut root).unwrap();
        let graph_ids = executions
            .iter()
            .map(|execution| execution.graph_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            graph_ids,
            [
                "first-grandchild",
                "second-grandchild",
                "second-child",
                "first-child",
                "root",
            ]
        );
    }

    #[test]
    fn compound_round_reattaches_the_hierarchy_when_a_child_processor_fails() {
        let options = LayeredOptions::default();
        let child = LGraph::new("child", options.clone());
        let mut holder = LNode::new("holder", 1.0, 1.0, None);
        holder.nested_graph = Some(Box::new(child));
        let mut root = LGraph::new("root", options);
        root.layerless_nodes.push(holder);
        let mut arena = CompoundGraphArena::new(&root);
        let child_index = arena
            .algorithms
            .iter()
            .position(|algorithm| algorithm.execution.graph_id == "child")
            .unwrap();
        arena.algorithms[child_index].processors = vec![ProcessorSlot {
            phase: None,
            kind: ProcessorKind::CommentPreprocessor,
        }];

        assert_eq!(
            execute_compound_graph_round(
                &mut root,
                &arena.entries,
                &mut arena.graph_slots,
                &mut arena.algorithms,
            ),
            Err(PipelineError::UnsupportedProcessor {
                kind: ProcessorKind::CommentPreprocessor,
            })
        );
        assert_eq!(
            root.layerless_nodes[0]
                .nested_graph
                .as_deref()
                .map(|graph| graph.id.as_str()),
            Some("child")
        );
        assert!(arena.graph_slots.iter().all(Option::is_none));
    }

    #[test]
    fn compound_round_restores_already_detached_graphs_when_detachment_panics() {
        let mut root = deep_compound_chain(3);
        let mut arena = CompoundGraphArena::new(&root);
        let grandchild_index = arena
            .algorithms
            .iter()
            .position(|algorithm| algorithm.execution.graph_id == "graph-2")
            .unwrap();
        arena.entries[grandchild_index]
            .parent
            .as_mut()
            .expect("the grandchild must have an arena parent")
            .node = usize::MAX;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = execute_compound_graph_round(
                &mut root,
                &arena.entries,
                &mut arena.graph_slots,
                &mut arena.algorithms,
            );
        }));

        assert!(result.is_err());
        let child = root.layerless_nodes[0]
            .nested_graph
            .as_deref()
            .expect("the graph detached before the panic must be restored");
        assert_eq!(child.id, "graph-1");
        assert_eq!(
            child.layerless_nodes[0]
                .nested_graph
                .as_deref()
                .map(|graph| graph.id.as_str()),
            Some("graph-2")
        );
        assert!(arena.graph_slots.iter().all(Option::is_none));
    }

    #[test]
    fn compound_round_reuses_the_arena_graph_slots() {
        let mut root = deep_compound_chain(8);
        let mut arena = CompoundGraphArena::new(&root);
        for algorithm in &mut arena.algorithms {
            algorithm.processors.clear();
        }
        let slots_ptr = arena.graph_slots.as_ptr();
        let slots_capacity = arena.graph_slots.capacity();

        for _ in 0..2 {
            execute_compound_graph_round(
                &mut root,
                &arena.entries,
                &mut arena.graph_slots,
                &mut arena.algorithms,
            )
            .unwrap();
            assert!(arena.graph_slots.iter().all(Option::is_none));
            assert_eq!(arena.graph_slots.as_ptr(), slots_ptr);
            assert_eq!(arena.graph_slots.capacity(), slots_capacity);
        }
    }

    #[test]
    fn full_runner_rejects_unported_strategies_before_mutating_graph() {
        let cases = [
            (
                LayeredOptions {
                    layering_strategy: LayeringStrategy::BreadthFirstModelOrder,
                    ..LayeredOptions::default()
                },
                ProcessorKind::BreadthFirstModelOrderLayerer,
            ),
            (
                LayeredOptions {
                    crossing_minimization_strategy: CrossingMinimizationStrategy::Interactive,
                    ..LayeredOptions::default()
                },
                ProcessorKind::InteractiveCrossingMinimizer,
            ),
            (
                LayeredOptions {
                    node_placement_strategy: NodePlacementStrategy::Interactive,
                    ..LayeredOptions::default()
                },
                ProcessorKind::InteractiveNodePlacer,
            ),
            (
                LayeredOptions {
                    node_placement_strategy: NodePlacementStrategy::LinearSegments,
                    ..LayeredOptions::default()
                },
                ProcessorKind::LinearSegmentsNodePlacer,
            ),
            (
                LayeredOptions {
                    edge_routing: EdgeRouting::Polyline,
                    ..LayeredOptions::default()
                },
                ProcessorKind::PolylineEdgeRouter,
            ),
            (
                LayeredOptions {
                    wrapping_strategy: WrappingStrategy::SingleEdge,
                    ..LayeredOptions::default()
                },
                ProcessorKind::SingleEdgeGraphWrapper,
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

    #[test]
    fn compound_full_runner_rejects_child_unported_processor_before_mutation() {
        let mut graph = deep_compound_chain(2);
        graph.layerless_nodes[0]
            .nested_graph
            .as_deref_mut()
            .expect("the fixture should contain one child graph")
            .options
            .layering_strategy = LayeringStrategy::BreadthFirstModelOrder;
        let before = graph.clone();

        assert_eq!(
            execute_ported_compound_processors(&mut graph),
            Err(PipelineError::UnsupportedProcessor {
                kind: ProcessorKind::BreadthFirstModelOrderLayerer,
            })
        );
        assert_eq!(graph, before);
    }

    /// A root that the configurator would rewrite (undefined direction, edge spacing below the
    /// minimum) and a child with seed 0, so a runner that configures before it checks the seed
    /// leaves a visible trace on the root.
    fn root_to_rewrite_with_zero_seed_child() -> LGraph {
        let mut graph = deep_compound_chain(2);
        graph.options.direction = ElkDirection::Undefined;
        graph.options.spacing.edge_edge = 0.0;
        graph.layerless_nodes[0]
            .nested_graph
            .as_deref_mut()
            .expect("the fixture should contain one child graph")
            .options
            .random_seed = 0;
        graph
    }

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
                kind: ProcessorKind::BreadthFirstModelOrderLayerer,
            })
        );
    }

    #[test]
    fn compound_ported_processor_preflight_is_small_stack_safe() {
        const GRAPH_COUNT: usize = 4_096;

        std::thread::Builder::new()
            .name("elk-ported-processor-preflight-small-stack".to_string())
            .stack_size(64 * 1024)
            .spawn(|| {
                let mut graph = deep_compound_chain(GRAPH_COUNT);
                let mut leaf = &mut graph;
                for _ in 1..GRAPH_COUNT {
                    leaf = leaf.layerless_nodes[0]
                        .nested_graph
                        .as_deref_mut()
                        .expect("the deep compound fixture should retain every child graph");
                }
                leaf.options.layering_strategy = LayeringStrategy::BreadthFirstModelOrder;

                let graph = std::mem::ManuallyDrop::new(graph);
                assert_eq!(
                    validate_ported_graph_processors(&graph),
                    Err(PipelineError::UnsupportedProcessor {
                        kind: ProcessorKind::BreadthFirstModelOrderLayerer,
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
                ProcessorKind::EdgeAndLayerConstraintEdgeReverser,
                ProcessorKind::GreedyCycleBreaker,
                ProcessorKind::LayerConstraintPreprocessor,
                ProcessorKind::NetworkSimplexLayerer,
                ProcessorKind::LayerConstraintPostprocessor,
                ProcessorKind::LongEdgeSplitter,
                ProcessorKind::PortSideProcessor,
                ProcessorKind::PortListSorter,
                ProcessorKind::LayerSweepCrossingMinimizerBarycenter,
                ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch,
                ProcessorKind::InLayerConstraintProcessor,
                ProcessorKind::LabelAndNodeSizeProcessor,
                ProcessorKind::InnermostNodeMarginCalculator,
                ProcessorKind::BKNodePlacer,
                ProcessorKind::LayerSizeAndGraphHeightCalculator,
                ProcessorKind::OrthogonalEdgeRouter,
                ProcessorKind::LongEdgeJoiner,
                ProcessorKind::EndLabelSorter,
                ProcessorKind::ReversedEdgeRestorer,
            ]
        );
    }

    #[test]
    fn flowchart_test_defaults_insert_direction_model_order_and_hierarchy_processors() {
        let options = LayeredOptions::flowchart_test_defaults(ElkDirection::Down);
        assert_eq!(
            kinds(&options),
            vec![
                ProcessorKind::DirectionPreprocessor,
                ProcessorKind::EdgeAndLayerConstraintEdgeReverser,
                ProcessorKind::GreedyCycleBreaker,
                ProcessorKind::LayerConstraintPreprocessor,
                ProcessorKind::NetworkSimplexLayerer,
                ProcessorKind::LayerConstraintPostprocessor,
                ProcessorKind::LongEdgeSplitter,
                ProcessorKind::PortSideProcessor,
                ProcessorKind::PortListSorter,
                ProcessorKind::SortByInputModelProcessor,
                ProcessorKind::LayerSweepCrossingMinimizerBarycenter,
                ProcessorKind::InLayerConstraintProcessor,
                ProcessorKind::LabelAndNodeSizeProcessor,
                ProcessorKind::InnermostNodeMarginCalculator,
                ProcessorKind::BKNodePlacer,
                ProcessorKind::LayerSizeAndGraphHeightCalculator,
                ProcessorKind::OrthogonalEdgeRouter,
                ProcessorKind::LongEdgeJoiner,
                ProcessorKind::EndLabelSorter,
                ProcessorKind::ReversedEdgeRestorer,
                ProcessorKind::HierarchicalNodeResizer,
                ProcessorKind::DirectionPostprocessor,
            ]
        );
    }

    #[test]
    fn mermaid_wrapping_flags_do_not_enable_wrapping_processors_without_strategy() {
        let options = LayeredOptions::flowchart_test_defaults(ElkDirection::Down);
        let processors = kinds(&options);
        assert!(!processors.contains(&ProcessorKind::BreakingPointInserter));
        assert!(!processors.contains(&ProcessorKind::BreakingPointProcessor));
        assert!(!processors.contains(&ProcessorKind::BreakingPointRemover));
    }

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
                kinds(&LayeredOptions::flowchart_test_defaults(direction)),
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
                kinds(&LayeredOptions {
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
                kinds(&LayeredOptions {
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
                kinds(&LayeredOptions {
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
                kinds(&LayeredOptions {
                    consider_model_order_strategy,
                    ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
                }),
            );
        }

        assert_processors_are_source_ported(
            "public forceNodeModelOrder",
            kinds(&LayeredOptions {
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
            graph_kinds(&merge_edges_graph),
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
            graph_kinds(&keep_entry_graph),
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
                    labels: vec![ElkInputLabel::center("center", 28.0, 12.0)],
                    ..edge("A-A", "A", "A")
                },
                end_label_edge,
            ],
        })
        .unwrap();
        assert_processors_are_source_ported(
            "Mermaid flowchart labels and self loops",
            graph_kinds(&label_and_self_loop_graph),
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
            graph_kinds(&compound_graph),
        );
        let nested_graph = compound_graph.layerless_nodes[0]
            .nested_graph
            .as_ref()
            .expect("compound fixture should create a nested graph");
        assert_processors_are_source_ported(
            "Mermaid flowchart compound child graph",
            graph_kinds(nested_graph),
        );
    }

    #[test]
    fn greedy_model_order_cycle_breaking_strategy_assembles_processor() {
        let options = LayeredOptions {
            cycle_breaking_strategy: CycleBreakingStrategy::GreedyModelOrder,
            ..LayeredOptions::flowchart_test_defaults(ElkDirection::Down)
        };
        assert!(kinds(&options).contains(&ProcessorKind::GreedyModelOrderCycleBreaker));
    }

    #[test]
    fn documented_node_placement_strategies_assemble_public_processors() {
        let cases = [
            (
                NodePlacementStrategy::BrandesKoepf,
                ProcessorKind::BKNodePlacer,
            ),
            (
                NodePlacementStrategy::Simple,
                ProcessorKind::SimpleNodePlacer,
            ),
            (
                NodePlacementStrategy::LinearSegments,
                ProcessorKind::LinearSegmentsNodePlacer,
            ),
            (
                NodePlacementStrategy::NetworkSimplex,
                ProcessorKind::NetworkSimplexPlacer,
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
            graph_kinds(&graph).contains(&ProcessorKind::HyperedgeDummyMerger),
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
                    labels: Vec::new(),
                    node_size: Default::default(),
                    self_loop_distribution: Default::default(),
                    self_loop_ordering: Default::default(),
                },
                ElkInputNode {
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
                    labels: Vec::new(),
                    node_size: Default::default(),
                    self_loop_distribution: Default::default(),
                    self_loop_ordering: Default::default(),
                },
            ],
            edges: vec![
                ElkInputEdge {
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
                },
                ElkInputEdge {
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
                },
            ],
        })
        .unwrap();
        preprocess_source_ported_compound_graph(&mut graph);

        let nested = graph.layerless_nodes[0].nested_graph.as_ref().unwrap();
        let processors = graph_kinds(nested);
        assert!(processors.contains(&ProcessorKind::HierarchicalPortConstraintProcessor));
        assert!(processors.contains(&ProcessorKind::HierarchicalPortDummySizeProcessor));
        assert!(processors.contains(&ProcessorKind::HierarchicalPortPositionProcessor));
        assert!(processors.contains(&ProcessorKind::HierarchicalPortOrthogonalEdgeRouter));
        assert!(processors.contains(&ProcessorKind::LabelDummyInserter));
        assert!(processors.contains(&ProcessorKind::LabelDummySwitcher));
        assert!(processors.contains(&ProcessorKind::LabelDummyRemover));
        assert!(processors.contains(&ProcessorKind::SelfLoopPreProcessor));
        assert!(processors.contains(&ProcessorKind::SelfLoopRouter));
        assert!(processors.contains(&ProcessorKind::SelfLoopPostProcessor));
    }

    #[test]
    fn inside_self_loops_enable_self_loop_processors_on_nested_graphs() {
        let mut input = ElkInputGraph {
            id: "root".to_string(),
            options: LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            nodes: vec![ElkInputNode {
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
                labels: Vec::new(),
                node_size: Default::default(),
                self_loop_distribution: Default::default(),
                self_loop_ordering: Default::default(),
            }],
            edges: vec![ElkInputEdge {
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
            }],
        };
        input.options.inside_self_loops_activate = true;

        let mut graph = import_graph(&input).unwrap();

        preprocess_source_ported_compound_graph(&mut graph);

        let nested = graph.layerless_nodes[0]
            .nested_graph
            .as_ref()
            .expect("inside self-loop should create a nested graph");
        let processors = graph_kinds(nested);

        assert!(processors.contains(&ProcessorKind::SelfLoopPreProcessor));
        assert!(processors.contains(&ProcessorKind::SelfLoopPortRestorer));
        assert!(processors.contains(&ProcessorKind::SelfLoopRouter));
        assert!(processors.contains(&ProcessorKind::SelfLoopPostProcessor));
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
        graph.layerless_nodes[port].port_ratio_or_position = 20.0;
        graph.set_node_layer(port, 0);

        execute_processor(
            &mut graph,
            ProcessorKind::HierarchicalPortOrthogonalEdgeRouter,
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
            ProcessorKind::HierarchicalPortOrthogonalEdgeRouter,
        )
        .unwrap();

        assert_eq!(graph.layerless_nodes[port].position.y, 0.0);
    }

    #[test]
    fn assembler_schedules_the_source_ported_processors_up_to_p3() {
        assert_eq!(
            kinds_through_phase(&p3_options(), LayeredPhase::P3NodeOrdering),
            vec![
                ProcessorKind::EdgeAndLayerConstraintEdgeReverser,
                ProcessorKind::GreedyCycleBreaker,
                ProcessorKind::LayerConstraintPreprocessor,
                ProcessorKind::NetworkSimplexLayerer,
                ProcessorKind::LayerConstraintPostprocessor,
                ProcessorKind::LongEdgeSplitter,
                ProcessorKind::PortSideProcessor,
                ProcessorKind::PortListSorter,
                ProcessorKind::LayerSweepCrossingMinimizerBarycenter,
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
                let flips_order =
                    (y_of(first.0) < y_of(second.0)) != (y_of(first.1) < y_of(second.1));
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

        assert!(executed.contains(&ProcessorKind::LayerSweepCrossingMinimizerBarycenter));
        assert!(executed.contains(&ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch));
        assert_eq!(executed.last(), Some(&ProcessorKind::BKNodePlacer));
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

        assert!(executed.contains(&ProcessorKind::LayerSweepCrossingMinimizerOneSidedGreedySwitch));
        assert_eq!(executed.last(), Some(&ProcessorKind::BKNodePlacer));
    }

    #[test]
    fn assembler_schedules_the_direction_preprocessor_for_down_layouts() {
        let executed = kinds_through_phase(
            &LayeredOptions::flowchart_test_defaults(ElkDirection::Down),
            LayeredPhase::P3NodeOrdering,
        );

        assert_eq!(executed[0], ProcessorKind::DirectionPreprocessor);
        assert!(executed.contains(&ProcessorKind::SortByInputModelProcessor));
        assert_eq!(
            executed.last(),
            Some(&ProcessorKind::LayerSweepCrossingMinimizerBarycenter)
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
        assert!(executed.contains(&ProcessorKind::LabelAndNodeSizeProcessor));
        assert!(executed.contains(&ProcessorKind::InnermostNodeMarginCalculator));
        assert_eq!(executed.last(), Some(&ProcessorKind::BKNodePlacer));

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
        assert_eq!(executed.last(), Some(&ProcessorKind::OrthogonalEdgeRouter));

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

        assert!(executed.contains(&ProcessorKind::SelfLoopPreProcessor));
        assert!(executed.contains(&ProcessorKind::SelfLoopPortRestorer));
        assert!(executed.contains(&ProcessorKind::SelfLoopRouter));
        assert!(executed.contains(&ProcessorKind::SelfLoopPostProcessor));
        assert!(graph.edge_source_attached(0));
        assert!(graph.edge_target_attached(0));
        assert_eq!(graph.edges[0].source.node, graph.edges[0].target.node);
        assert!(graph.edges[0].bend_points.len() >= 2);
        assert!(
            graph.edges[0]
                .bend_points
                .iter()
                .all(|point| point.x.is_finite() && point.y.is_finite())
        );
        assert!(graph.layerless_nodes[0].margin.top > 0.0);
    }

    #[test]
    fn assembled_processors_keep_long_edge_joiner_after_p5_router() {
        let processors = kinds(&p3_options());
        let router = processors
            .iter()
            .position(|kind| *kind == ProcessorKind::OrthogonalEdgeRouter)
            .unwrap();
        let joiner = processors
            .iter()
            .position(|kind| *kind == ProcessorKind::LongEdgeJoiner)
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

        assert!(executed.contains(&ProcessorKind::EndLabelSorter));
        assert!(executed.contains(&ProcessorKind::ReversedEdgeRestorer));
        assert_eq!(executed.last(), Some(&ProcessorKind::ReversedEdgeRestorer));
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

        assert!(executed.contains(&ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch));
        assert_eq!(executed.last(), Some(&ProcessorKind::ReversedEdgeRestorer));
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

        assert!(executed.contains(&ProcessorKind::LabelDummyInserter));
        assert!(executed.contains(&ProcessorKind::LabelDummySwitcher));
        assert!(executed.contains(&ProcessorKind::LabelDummyRemover));
        assert!(
            !graph
                .layers
                .iter()
                .flat_map(|layer| layer.nodes.iter().copied())
                .any(|node| graph.layerless_nodes[node].kind == crate::graph::LNodeKind::Label)
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
                crate::HierarchyHandling::SeparateChildren,
                (160.0, 215.0),
            ),
            (
                false,
                crate::HierarchyHandling::SeparateChildren,
                (512.0, 61.0),
            ),
            (
                true,
                crate::HierarchyHandling::IncludeChildren,
                (512.0, 61.0),
            ),
        ] {
            let options = LayeredOptions {
                direction: ElkDirection::Down,
                spacing: crate::SpacingOptions::layered_base_value(40.0),
                separate_connected_components: separate,
                hierarchy_handling: hierarchy,
                padding: crate::ElkPadding::uniform(12.0),
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
            if separate && hierarchy == crate::HierarchyHandling::SeparateChildren {
                for (index, node) in graph.layerless_nodes.iter().enumerate() {
                    assert_eq!(
                        node.id,
                        ["A", "B", "C"][index],
                        "stable input arena indices"
                    );
                    assert_eq!(
                        (
                            node.position.x + graph.offset.x,
                            node.position.y + graph.offset.y
                        ),
                        (0.0, index as f64 * 77.0)
                    );
                }
            }
        }
    }

    #[test]
    fn connected_components_translate_nodes_and_edges_together() {
        let options = LayeredOptions {
            direction: ElkDirection::Down,
            spacing: crate::SpacingOptions::layered_base_value(40.0),
            padding: crate::ElkPadding::uniform(12.0),
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
            crate::LSize {
                width: 294.0,
                height: 246.0
            }
        );
        for (index, expected) in [(12.0, 66.0), (167.0, 66.0), (12.0, 197.0)]
            .into_iter()
            .enumerate()
        {
            let node = &graph.layerless_nodes[index * 2 + 1];
            assert_eq!(
                (
                    node.position.x + graph.padding.left + graph.offset.x,
                    node.position.y + graph.padding.top + graph.offset.y
                ),
                expected
            );
        }
        let edge = &graph.edges[2];
        for (port_ref, expected) in [(edge.source, (69.5, 157.0)), (edge.target, (69.5, 197.0))] {
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
        let minimum = crate::LSize {
            width: 900.0,
            height: 800.0,
        };
        with_minimum.options.node_size = crate::options::NodeSizing::with_minimum(minimum);
        execute_ported_processors(&mut graph).unwrap();
        execute_ported_processors(&mut with_minimum).unwrap();
        assert_eq!(with_minimum.options.node_size.minimum, minimum);
        assert_eq!(with_minimum.exported_root_size(), minimum);
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(&graph.layerless_nodes[index].id, id);
            assert_eq!(
                graph.layerless_nodes[index].position, with_minimum.layerless_nodes[index].position,
                "root minimum is applied after packing, through the root offset"
            );
        }
        for (index, node) in graph.layerless_nodes.iter().enumerate() {
            for (port_index, port) in node.ports.iter().enumerate() {
                assert_eq!(port.node, index);
                for &edge in &port.incoming_edges {
                    assert_eq!(
                        graph.edges[edge].target,
                        crate::PortRef {
                            node: index,
                            port: port_index
                        }
                    );
                }
                for &edge in &port.outgoing_edges {
                    assert_eq!(
                        graph.edges[edge].source,
                        crate::PortRef {
                            node: index,
                            port: port_index
                        }
                    );
                }
            }
        }
        for edge in &graph.edges {
            for endpoint in [edge.source, edge.target] {
                assert!(endpoint.port < graph.layerless_nodes[endpoint.node].ports.len());
            }
            for label in &edge.labels {
                assert!(
                    label
                        .end_label_edge
                        .is_none_or(|index| index < graph.edges.len())
                );
            }
        }
        for component in 0..2 {
            for suffix in ["AC", "BB"] {
                let edge = &graph.edges[component * 4 + if suffix == "AC" { 2 } else { 3 }];
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
            let nodes = &graph.layerless_nodes[component * 3..component * 3 + 3];
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

        assert!(executed.contains(&ProcessorKind::HierarchicalNodeResizer));
        assert_eq!(
            executed.last(),
            Some(&ProcessorKind::DirectionPostprocessor)
        );
        assert!(graph.layers.is_empty());
        assert!(
            graph
                .layerless_nodes
                .iter()
                .all(|node| node.layer_index.is_none())
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
        assert_eq!(executed[0].graph_id, "cluster");
        assert_eq!(executed[1].graph_id, "root");
        assert!(
            executed[0]
                .processors
                .contains(&ProcessorKind::HierarchicalNodeResizer)
        );
        assert!(
            !executed[0]
                .processors
                .iter()
                .any(|kind| kind.is_hierarchy_aware())
        );
        assert!(
            executed[1]
                .processors
                .iter()
                .any(|kind| kind.is_hierarchy_aware())
        );
        let cluster = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        let nested = cluster.nested_graph.as_ref().unwrap();
        assert!(cluster.size.width >= nested.size.width);
        assert!(cluster.size.height >= nested.size.height);
        assert!(nested.layers.is_empty());
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
                .filter(|kind| kind.is_hierarchy_aware())
                .count()
                > 1
        );
        assert!(child_execution.contains(&ProcessorKind::HierarchicalNodeResizer));
        assert_eq!(
            child_execution.last(),
            Some(&ProcessorKind::DirectionPostprocessor)
        );
        let cluster = graph
            .layerless_nodes
            .iter()
            .find(|node| node.id == "cluster")
            .unwrap();
        assert!(cluster.size.width > 1.0);
        assert!(cluster.size.height > 1.0);
        assert!(cluster.nested_graph.as_ref().unwrap().layers.is_empty());
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
                .filter(|node| node.kind == crate::graph::LNodeKind::ExternalPort)
                .all(|node| node.layer_index.is_none())
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
            let child = &graph.layerless_nodes[0];
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
        let group = &graph.layerless_nodes[0];
        let nested = group.nested_graph.as_ref().unwrap();
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
            nested.layerless_nodes[0].position.y + nested.offset.y + nested.padding.top,
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
        east_node.kind = LNodeKind::ExternalPort;
        east_node.external_port_side = PortSide::East;
        east_node.position.x = 12.0;
        let east = graph.layerless_nodes.len();
        graph.layerless_nodes.push(east_node);

        let mut south_node = LNode::new("south", 0.0, 0.0, None);
        south_node.kind = LNodeKind::ExternalPort;
        south_node.external_port_side = PortSide::South;
        south_node.position.y = 25.0;
        let south = graph.layerless_nodes.len();
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
            0,
            crate::graph::PortType::Output,
        ));
        node.ports[0].set_side(PortSide::East);
        node.ports[0].position = LPoint { x: 100.0, y: 25.0 };
        node.ports.push(crate::graph::LPort::new(
            "south".to_string(),
            0,
            crate::graph::PortType::Output,
        ));
        node.ports[1].set_side(PortSide::South);
        node.ports[1].position = LPoint { x: 50.0, y: 50.0 };

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
        assert_eq!(node.ports[0].position, LPoint { x: 150.0, y: 50.0 });
        assert_eq!(node.ports[1].position, LPoint { x: 75.0, y: 100.0 });
    }

    #[test]
    fn layered_node_resize_keeps_external_port_positions_fixed() {
        let mut node = LNode::new("cluster", 100.0, 50.0, None);
        node.port_constraints = PortConstraints::FixedPos;
        node.ports.push(crate::graph::LPort::new(
            "external".to_string(),
            0,
            crate::graph::PortType::Input,
        ));
        node.ports[0].set_side(PortSide::West);
        node.ports[0].position = LPoint { x: -2.0, y: 20.0 };

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
        assert_eq!(node.ports[0].position, LPoint { x: -2.0, y: 20.0 });
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

        assert!(executed.contains(&ProcessorKind::ReversedEdgeRestorer));
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

        assert!(executed.contains(&ProcessorKind::LabelSideSelector));
        assert!(executed.contains(&ProcessorKind::EndLabelPreprocessor));
        assert!(executed.contains(&ProcessorKind::EndLabelSorter));
        assert!(executed.contains(&ProcessorKind::EndLabelPostprocessor));
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
        assert_eq!(label.end_label_edge, Some(0));
        assert!(
            graph
                .layerless_nodes
                .iter()
                .flat_map(|node| node.ports.iter())
                .all(|port| port.labels.is_empty() && port.end_label_cell.is_none())
        );
    }

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
        assert!(
            !graph_kinds(&graph)
                .contains(&ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch)
        );

        options.greedy_switch_activation_threshold = 0;
        graph.options = options;
        assert!(
            graph_kinds(&graph)
                .contains(&ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch)
        );

        let mut nested = LGraph::new(
            "cluster",
            LayeredOptions {
                hierarchy_handling: crate::options::HierarchyHandling::IncludeChildren,
                greedy_switch_hierarchical_type: crate::options::GreedySwitchType::TwoSided,
                ..LayeredOptions::default()
            },
        );
        nested.parent_node_id = Some("cluster".to_string());
        assert!(
            !graph_kinds(&nested)
                .contains(&ProcessorKind::LayerSweepCrossingMinimizerTwoSidedGreedySwitch)
        );
    }

    fn push_test_external_dummy(graph: &mut LGraph, id: &str, side: PortSide) -> usize {
        let node = graph.layerless_nodes.len();
        let mut dummy = LNode::new(id, 0.0, 0.0, None);
        dummy.kind = LNodeKind::ExternalPort;
        dummy.external_port_side = side;
        graph.layerless_nodes.push(dummy);
        graph
            .add_port(node, PortType::Input, side.opposed(), Default::default())
            .unwrap();
        node
    }
}
