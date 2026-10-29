//! The processors of one layered run, chosen and put in order.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/IntermediateProcessorStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredPhases.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/CycleBreakingStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/LayeringStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/CrossingMinimizationStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/NodePlacementStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/EdgeRouterFactory.java
//!
//! Every processor that runs is a variant of one enum: [`IntermediateProcessor`] for the steps
//! between the phases, and one small enum for the strategies of each phase. A processor that ELK
//! has and this crate has not ported is no variant. [`Strategies::select`] refuses the option that
//! asks for it, with a typed error, before the run changes anything.

use std::collections::BTreeSet;

use super::{PipelineError, PipelineResult};
use crate::graph::GraphProperties;
use crate::options::{
    CrossingMinimizationStrategy, CycleBreakingStrategy, EdgeRouting, ElkDirection,
    GreedySwitchType, HierarchyHandling, LayeredOptions, LayeringStrategy, NodePlacementStrategy,
    OrderingStrategy, WrappingStrategy,
};
use crate::p3order::sweep::CrossMinType;

/// The five phases of ELK's layered algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum LayeredPhase {
    P1CycleBreaking,
    P2Layering,
    P3NodeOrdering,
    P4NodePlacement,
    P5EdgeRouting,
}

impl LayeredPhase {
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

/// `IntermediateProcessorStrategy`, the processors this crate has ported.
///
/// The variants stand in the order of the Java enum, and the derived order is the order the
/// processors run in within one slot, as `EnumSet` gives it in Java. Do not sort the variants by
/// name. The two greedy switches are the crossing minimizer's greedy-switch types of ELK's
/// `LayerSweepCrossingMinimizer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IntermediateProcessor {
    DirectionPreprocessor,
    EdgeAndLayerConstraintEdgeReverser,
    InteractiveExternalPortPositioner,
    LabelDummyInserter,
    SelfLoopPreProcessor,
    LayerConstraintPreprocessor,
    LayerConstraintPostprocessor,
    HierarchicalPortConstraintProcessor,
    LongEdgeSplitter,
    PortSideProcessor,
    InvertedPortProcessor,
    PortListSorter,
    SortByInputModelProcessor,
    NorthSouthPortPreprocessor,
    OneSidedGreedySwitch,
    TwoSidedGreedySwitch,
    SelfLoopPortRestorer,
    InLayerConstraintProcessor,
    LabelAndNodeSizeProcessor,
    InnermostNodeMarginCalculator,
    SelfLoopRouter,
    EndLabelPreprocessor,
    LabelDummySwitcher,
    LabelSideSelector,
    HyperedgeDummyMerger,
    HierarchicalPortDummySizeProcessor,
    LayerSizeAndGraphHeightCalculator,
    HierarchicalPortPositionProcessor,
    HierarchicalPortOrthogonalEdgeRouter,
    LongEdgeJoiner,
    SelfLoopPostProcessor,
    NorthSouthPortPostprocessor,
    LabelDummyRemover,
    EndLabelSorter,
    ReversedEdgeRestorer,
    EndLabelPostprocessor,
    HierarchicalNodeResizer,
    DirectionPostprocessor,
}

/// `CycleBreakingStrategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CycleBreaker {
    Greedy,
    DepthFirst,
    Interactive,
    ModelOrder,
    GreedyModelOrder,
}

/// `LayeringStrategy`, without the two model-order layerers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layerer {
    NetworkSimplex,
    LongestPath,
    LongestPathSource,
    CoffmanGraham,
    Interactive,
    StretchWidth,
    MinWidth,
}

/// `CrossingMinimizationStrategy`, without the interactive minimizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrossingMinimizer {
    /// `LayerSweepCrossingMinimizer` with the barycenter heuristic.
    LayerSweep,
    /// `NONE`: the node order stays as the earlier steps left it.
    NoMinimizer,
}

/// `NodePlacementStrategy`, without the interactive and the linear-segments placer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodePlacer {
    Simple,
    BrandesKoepf,
    NetworkSimplex,
}

/// `EdgeRouting`, without polyline and splines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeRouter {
    Orthogonal,
}

/// A processor of ELK that this crate has not ported, and that an option or a graph property can
/// ask for. The names are ELK's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnportedProcessor {
    BreadthFirstModelOrderLayerer,
    DepthFirstModelOrderLayerer,
    InteractiveCrossingMinimizer,
    InteractiveNodePlacer,
    LinearSegmentsNodePlacer,
    PolylineEdgeRouter,
    SplineEdgeRouter,
    SingleEdgeGraphWrapper,
    BreakingPointInserter,
    HypernodesProcessor,
}

fn unported(kind: UnportedProcessor) -> PipelineError {
    PipelineError::UnsupportedProcessor { kind }
}

impl CycleBreaker {
    fn select(strategy: CycleBreakingStrategy) -> Self {
        match strategy {
            CycleBreakingStrategy::Greedy => Self::Greedy,
            CycleBreakingStrategy::DepthFirst => Self::DepthFirst,
            CycleBreakingStrategy::Interactive => Self::Interactive,
            CycleBreakingStrategy::ModelOrder => Self::ModelOrder,
            CycleBreakingStrategy::GreedyModelOrder => Self::GreedyModelOrder,
        }
    }
}

impl Layerer {
    fn select(strategy: LayeringStrategy) -> PipelineResult<Self> {
        match strategy {
            LayeringStrategy::NetworkSimplex => Ok(Self::NetworkSimplex),
            LayeringStrategy::LongestPath => Ok(Self::LongestPath),
            LayeringStrategy::LongestPathSource => Ok(Self::LongestPathSource),
            LayeringStrategy::CoffmanGraham => Ok(Self::CoffmanGraham),
            LayeringStrategy::Interactive => Ok(Self::Interactive),
            LayeringStrategy::StretchWidth => Ok(Self::StretchWidth),
            LayeringStrategy::MinWidth => Ok(Self::MinWidth),
            LayeringStrategy::BreadthFirstModelOrder => {
                Err(unported(UnportedProcessor::BreadthFirstModelOrderLayerer))
            }
            LayeringStrategy::DepthFirstModelOrder => {
                Err(unported(UnportedProcessor::DepthFirstModelOrderLayerer))
            }
        }
    }
}

impl CrossingMinimizer {
    fn select(strategy: CrossingMinimizationStrategy) -> PipelineResult<Self> {
        match strategy {
            CrossingMinimizationStrategy::LayerSweep => Ok(Self::LayerSweep),
            CrossingMinimizationStrategy::None => Ok(Self::NoMinimizer),
            CrossingMinimizationStrategy::Interactive => {
                Err(unported(UnportedProcessor::InteractiveCrossingMinimizer))
            }
        }
    }
}

impl NodePlacer {
    fn select(strategy: NodePlacementStrategy) -> PipelineResult<Self> {
        match strategy {
            NodePlacementStrategy::Simple => Ok(Self::Simple),
            NodePlacementStrategy::BrandesKoepf => Ok(Self::BrandesKoepf),
            NodePlacementStrategy::NetworkSimplex => Ok(Self::NetworkSimplex),
            NodePlacementStrategy::Interactive => {
                Err(unported(UnportedProcessor::InteractiveNodePlacer))
            }
            NodePlacementStrategy::LinearSegments => {
                Err(unported(UnportedProcessor::LinearSegmentsNodePlacer))
            }
        }
    }
}

impl EdgeRouter {
    fn select(strategy: EdgeRouting) -> PipelineResult<Self> {
        match strategy {
            EdgeRouting::Orthogonal => Ok(Self::Orthogonal),
            EdgeRouting::Polyline => Err(unported(UnportedProcessor::PolylineEdgeRouter)),
            EdgeRouting::Splines => Err(unported(UnportedProcessor::SplineEdgeRouter)),
        }
    }
}

/// One step of a run: a processor of a phase or an intermediate processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Processor {
    CycleBreaker(CycleBreaker),
    Layerer(Layerer),
    CrossingMinimizer(CrossingMinimizer),
    NodePlacer(NodePlacer),
    EdgeRouter(EdgeRouter),
    Intermediate(IntermediateProcessor),
}

impl Processor {
    /// The phase this processor is the algorithm of. `None` for an intermediate processor.
    #[cfg(test)]
    pub(crate) fn phase(self) -> Option<LayeredPhase> {
        match self {
            Self::CycleBreaker(_) => Some(LayeredPhase::P1CycleBreaking),
            Self::Layerer(_) => Some(LayeredPhase::P2Layering),
            Self::CrossingMinimizer(_) => Some(LayeredPhase::P3NodeOrdering),
            Self::NodePlacer(_) => Some(LayeredPhase::P4NodePlacement),
            Self::EdgeRouter(_) => Some(LayeredPhase::P5EdgeRouting),
            Self::Intermediate(_) => None,
        }
    }

    /// The sweep a processor runs with, if it is one of ELK's hierarchy-aware layer sweeps. A
    /// compound run does these once for the whole hierarchy, at the root.
    pub(crate) fn hierarchy_aware_sweep(self) -> Option<CrossMinType> {
        match self {
            Self::CrossingMinimizer(CrossingMinimizer::LayerSweep) => {
                Some(CrossMinType::Barycenter)
            }
            Self::Intermediate(IntermediateProcessor::OneSidedGreedySwitch) => {
                Some(CrossMinType::OneSidedGreedySwitch)
            }
            Self::Intermediate(IntermediateProcessor::TwoSidedGreedySwitch) => {
                Some(CrossMinType::TwoSidedGreedySwitch)
            }
            _ => None,
        }
    }
}

/// The processor each phase runs with.
///
/// This is the part of the assembly that can fail, and it reads only the options and the graph
/// properties as the importer left them. A run selects it before it changes the graph, and builds
/// the list from it after the configurator has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Strategies {
    cycle_breaker: CycleBreaker,
    layerer: Layerer,
    crossing_minimizer: CrossingMinimizer,
    node_placer: NodePlacer,
    edge_router: EdgeRouter,
}

impl Strategies {
    /// Picks the processor of each phase, then refuses the options and graph properties that
    /// need a processor this crate has not ported.
    ///
    /// The first refusal wins in this order: phases one to five, the wrapping strategy, then
    /// hypernodes. ELK's own slot order decides which of two unported processors comes first in a
    /// run, but the run does not start, so it does not matter which of them the error names.
    pub(crate) fn select(
        options: &LayeredOptions,
        properties: &GraphProperties,
    ) -> PipelineResult<Self> {
        let strategies = Self {
            cycle_breaker: CycleBreaker::select(options.cycle_breaking_strategy),
            layerer: Layerer::select(options.layering_strategy)?,
            crossing_minimizer: CrossingMinimizer::select(options.crossing_minimization_strategy)?,
            node_placer: NodePlacer::select(options.node_placement_strategy)?,
            edge_router: EdgeRouter::select(options.edge_routing)?,
        };
        match options.wrapping_strategy {
            WrappingStrategy::Off => {}
            WrappingStrategy::SingleEdge => {
                return Err(unported(UnportedProcessor::SingleEdgeGraphWrapper));
            }
            WrappingStrategy::MultiEdge => {
                return Err(unported(UnportedProcessor::BreakingPointInserter));
            }
        }
        if properties.hypernodes {
            return Err(unported(UnportedProcessor::HypernodesProcessor));
        }
        Ok(strategies)
    }

    fn phase_processors(self) -> [Processor; 5] {
        [
            Processor::CycleBreaker(self.cycle_breaker),
            Processor::Layerer(self.layerer),
            Processor::CrossingMinimizer(self.crossing_minimizer),
            Processor::NodePlacer(self.node_placer),
            Processor::EdgeRouter(self.edge_router),
        ]
    }
}

/// `LayoutProcessorConfiguration`: the intermediate processors in front of each phase and after
/// the last one. Slot `n` runs before phase `n` and after phase `n - 1`. A set keeps each
/// processor once and in declaration order, as the `EnumSet`s of Java do.
#[derive(Debug, Default)]
struct Config {
    slots: [BTreeSet<IntermediateProcessor>; 6],
}

impl Config {
    fn add_before(&mut self, phase: LayeredPhase, processor: IntermediateProcessor) {
        self.slots[phase.ordinal()].insert(processor);
    }

    fn add_after(&mut self, phase: LayeredPhase, processor: IntermediateProcessor) {
        self.slots[phase.ordinal() + 1].insert(processor);
    }

    fn merge(&mut self, other: Config) {
        for (slot, processors) in self.slots.iter_mut().zip(other.slots) {
            slot.extend(processors);
        }
    }

    fn into_processors(self, strategies: Strategies) -> Vec<Processor> {
        let mut out = Vec::new();
        let mut slots = self.slots.into_iter();
        for phase_processor in strategies.phase_processors() {
            out.extend(
                slots
                    .next()
                    .into_iter()
                    .flatten()
                    .map(Processor::Intermediate),
            );
            out.push(phase_processor);
        }
        out.extend(
            slots
                .next()
                .into_iter()
                .flatten()
                .map(Processor::Intermediate),
        );
        out
    }
}

/// The processors of one graph, in the order they run.
///
/// This follows `GraphConfigurator.prepareGraphForLayout(...)` and the phases'
/// `getLayoutProcessorConfiguration(...)`. It reads the graph properties, not the
/// `graph_has_*` copies in the options, so it gives the same list before and after the
/// configurator has run over the graph.
pub(crate) fn assemble(
    strategies: Strategies,
    options: &LayeredOptions,
    properties: &GraphProperties,
    graph_size: usize,
    is_root_graph: bool,
) -> Vec<Processor> {
    let mut config = baseline_configuration();
    config.merge(graph_configurator_configuration(
        options,
        graph_size,
        is_root_graph,
    ));
    config.merge(cycle_breaking_configuration(strategies.cycle_breaker));
    config.merge(layering_configuration(strategies.layerer));
    config.merge(crossing_minimization_configuration());
    config.merge(edge_routing_configuration(properties, options));
    config.into_processors(strategies)
}

/// `GraphConfigurator.BASELINE_PROCESSING_CONFIGURATION`.
fn baseline_configuration() -> Config {
    let mut config = Config::default();
    config.add_before(
        LayeredPhase::P4NodePlacement,
        IntermediateProcessor::InnermostNodeMarginCalculator,
    );
    config.add_before(
        LayeredPhase::P4NodePlacement,
        IntermediateProcessor::LabelAndNodeSizeProcessor,
    );
    config.add_before(
        LayeredPhase::P5EdgeRouting,
        IntermediateProcessor::LayerSizeAndGraphHeightCalculator,
    );
    config.add_after(
        LayeredPhase::P5EdgeRouting,
        IntermediateProcessor::EndLabelSorter,
    );
    config
}

/// The option-driven part of `GraphConfigurator.getPhaseIndependentLayoutProcessorConfiguration`.
fn graph_configurator_configuration(
    options: &LayeredOptions,
    graph_size: usize,
    is_root_graph: bool,
) -> Config {
    let mut config = Config::default();
    if options.hierarchy_handling == HierarchyHandling::IncludeChildren {
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::HierarchicalNodeResizer,
        );
    }

    let port_side_phase = if options.feedback_edges {
        LayeredPhase::P1CycleBreaking
    } else {
        LayeredPhase::P3NodeOrdering
    };
    config.add_before(port_side_phase, IntermediateProcessor::PortSideProcessor);

    match options.direction {
        ElkDirection::Left | ElkDirection::Down | ElkDirection::Up => {
            config.add_before(
                LayeredPhase::P1CycleBreaking,
                IntermediateProcessor::DirectionPreprocessor,
            );
            config.add_after(
                LayeredPhase::P5EdgeRouting,
                IntermediateProcessor::DirectionPostprocessor,
            );
        }
        ElkDirection::Right | ElkDirection::Undefined => {}
    }

    if let Some(greedy_switch) = greedy_switch_for(options, graph_size, is_root_graph) {
        config.add_before(LayeredPhase::P4NodePlacement, greedy_switch);
    }

    if options.consider_model_order_strategy != OrderingStrategy::None {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            IntermediateProcessor::SortByInputModelProcessor,
        );
    }
    config
}

/// The greedy switch a graph gets, if any. A hierarchical layout switches once, at the root, with
/// the hierarchical type. Any other graph switches with its own type, unless interactive
/// crossing minimization is on or the graph has as many nodes as the activation threshold.
fn greedy_switch_for(
    options: &LayeredOptions,
    graph_size: usize,
    is_root_graph: bool,
) -> Option<IntermediateProcessor> {
    let switch_type = if options.is_hierarchical_layout() {
        if !is_root_graph {
            return None;
        }
        options.greedy_switch_hierarchical_type
    } else {
        let interactive_cross_min =
            options.crossing_minimization_strategy == CrossingMinimizationStrategy::Interactive;
        let below_threshold = options.greedy_switch_activation_threshold == 0
            || options.greedy_switch_activation_threshold > graph_size;
        if interactive_cross_min || !below_threshold {
            return None;
        }
        options.greedy_switch_type
    };
    match switch_type {
        GreedySwitchType::OneSided => Some(IntermediateProcessor::OneSidedGreedySwitch),
        GreedySwitchType::TwoSided => Some(IntermediateProcessor::TwoSidedGreedySwitch),
        GreedySwitchType::Off => None,
    }
}

/// `CycleBreakingStrategy.getLayoutProcessorConfiguration`.
fn cycle_breaking_configuration(cycle_breaker: CycleBreaker) -> Config {
    let mut config = Config::default();
    if cycle_breaker == CycleBreaker::Interactive {
        config.add_before(
            LayeredPhase::P1CycleBreaking,
            IntermediateProcessor::InteractiveExternalPortPositioner,
        );
    }
    config.add_after(
        LayeredPhase::P5EdgeRouting,
        IntermediateProcessor::ReversedEdgeRestorer,
    );
    config
}

/// `LayeringStrategy.getLayoutProcessorConfiguration`.
fn layering_configuration(layerer: Layerer) -> Config {
    let mut config = Config::default();
    config.add_before(
        LayeredPhase::P1CycleBreaking,
        if layerer == Layerer::Interactive {
            IntermediateProcessor::InteractiveExternalPortPositioner
        } else {
            IntermediateProcessor::EdgeAndLayerConstraintEdgeReverser
        },
    );
    config.add_before(
        LayeredPhase::P2Layering,
        IntermediateProcessor::LayerConstraintPreprocessor,
    );
    config.add_before(
        LayeredPhase::P3NodeOrdering,
        IntermediateProcessor::LayerConstraintPostprocessor,
    );
    config
}

/// `CrossingMinimizationStrategy.getLayoutProcessorConfiguration`: the same for every strategy.
fn crossing_minimization_configuration() -> Config {
    let mut config = Config::default();
    config.add_before(
        LayeredPhase::P3NodeOrdering,
        IntermediateProcessor::LongEdgeSplitter,
    );
    config.add_before(
        LayeredPhase::P3NodeOrdering,
        IntermediateProcessor::PortListSorter,
    );
    config.add_before(
        LayeredPhase::P4NodePlacement,
        IntermediateProcessor::InLayerConstraintProcessor,
    );
    config.add_after(
        LayeredPhase::P5EdgeRouting,
        IntermediateProcessor::LongEdgeJoiner,
    );
    config
}

/// `OrthogonalEdgeRouter.getLayoutProcessorConfiguration`: the router asks for the processors
/// that the graph properties and feedback edges need.
fn edge_routing_configuration(properties: &GraphProperties, options: &LayeredOptions) -> Config {
    let mut config = Config::default();
    if properties.hyperedges {
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::HyperedgeDummyMerger,
        );
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            IntermediateProcessor::InvertedPortProcessor,
        );
    }
    if properties.non_free_ports || options.feedback_edges {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            IntermediateProcessor::InvertedPortProcessor,
        );
        if properties.north_south_ports {
            config.add_before(
                LayeredPhase::P3NodeOrdering,
                IntermediateProcessor::NorthSouthPortPreprocessor,
            );
            config.add_after(
                LayeredPhase::P5EdgeRouting,
                IntermediateProcessor::NorthSouthPortPostprocessor,
            );
        }
    }
    if properties.external_ports {
        config.add_before(
            LayeredPhase::P3NodeOrdering,
            IntermediateProcessor::HierarchicalPortConstraintProcessor,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::HierarchicalPortDummySizeProcessor,
        );
        config.add_before(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::HierarchicalPortPositionProcessor,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::HierarchicalPortOrthogonalEdgeRouter,
        );
    }
    if properties.self_loops {
        config.add_before(
            LayeredPhase::P1CycleBreaking,
            IntermediateProcessor::SelfLoopPreProcessor,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::SelfLoopPortRestorer,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::SelfLoopRouter,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::SelfLoopPostProcessor,
        );
    }
    if properties.center_labels {
        config.add_before(
            LayeredPhase::P2Layering,
            IntermediateProcessor::LabelDummyInserter,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::LabelDummySwitcher,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::LabelSideSelector,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::LabelDummyRemover,
        );
    }
    if properties.end_labels {
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::LabelSideSelector,
        );
        config.add_before(
            LayeredPhase::P4NodePlacement,
            IntermediateProcessor::EndLabelPreprocessor,
        );
        config.add_after(
            LayeredPhase::P5EdgeRouting,
            IntermediateProcessor::EndLabelPostprocessor,
        );
    }
    config
}
