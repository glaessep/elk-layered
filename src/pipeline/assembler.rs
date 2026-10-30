//! The processors of one layered run, chosen and put in order.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/IntermediateProcessorStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/LayeredPhases.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/CycleBreakingStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/GreedyCycleBreaker.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p1cycles/InteractiveCycleBreaker.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/LayeringStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/NetworkSimplexLayerer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/InteractiveLayerer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/CrossingMinimizationStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/LayerSweepCrossingMinimizer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p3order/NoCrossingMinimizer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/NodePlacementStrategy.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/EdgeRouterFactory.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/OrthogonalEdgeRouter.java
//!
//! Every processor that runs is a variant of one enum: [`IntermediateProcessor`] for the steps
//! between the phases, and one small enum for the strategies of each phase. A processor that ELK
//! has and this crate has not ported is no variant. [`Strategies::select`] refuses the option that
//! asks for it, with a typed error, before the run changes anything.

use std::collections::BTreeSet;

use super::{PipelineError, PipelineResult};
use crate::graph::{GraphProperties, LGraph};
use crate::options::{
    CrossingMinimizationStrategy, CycleBreakingStrategy, EdgeRouting, ElkDirection,
    GreedySwitchType, HierarchyHandling, LayeredOptions, LayeringStrategy, NodePlacementStrategy,
    OrderingStrategy, WrappingStrategy,
};
use crate::phase::ordering::CrossMinType;

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
    /// The slot in front of the phase: the variants stand in the order of the phases.
    fn slot_before(self) -> usize {
        self as usize
    }

    /// The slot after the phase. It is the slot in front of the next phase.
    fn slot_after(self) -> usize {
        self as usize + 1
    }
}

/// A point in the schedule: in front of a phase, in the phase, or after it.
///
/// The slots of [`Config`] and the places of the refusals count the same way. `After(p)` and
/// `Before(p + 1)` are one slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Before(LayeredPhase),
    /// The algorithm of the phase itself.
    In(LayeredPhase),
    After(LayeredPhase),
}

impl Place {
    /// Where the place stands on the line of the whole run: slot, phase, slot, phase, and so on.
    /// A smaller number runs first. A phase sits between its two slots.
    pub(super) fn position(self) -> usize {
        match self {
            Self::Before(phase) => 2 * phase.slot_before(),
            Self::In(phase) => 2 * phase.slot_before() + 1,
            Self::After(phase) => 2 * phase.slot_after(),
        }
    }
}

/// An option or graph property that needs a processor this crate has not ported, and the place in
/// the schedule where a run would reach that processor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Refusal {
    place: Place,
    processor: UnportedProcessor,
}

impl Refusal {
    fn new(place: Place, processor: UnportedProcessor) -> Self {
        Self { place, processor }
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
    /// `LayeringStrategy::BreadthFirstModelOrder`.
    BreadthFirstModelOrderLayerer,
    /// `LayeringStrategy::DepthFirstModelOrder`.
    DepthFirstModelOrderLayerer,
    /// `CrossingMinimizationStrategy::Interactive`.
    InteractiveCrossingMinimizer,
    /// `NodePlacementStrategy::Interactive`.
    InteractiveNodePlacer,
    /// `NodePlacementStrategy::LinearSegments`.
    LinearSegmentsNodePlacer,
    /// `EdgeRouting::Polyline`.
    PolylineEdgeRouter,
    /// `EdgeRouting::Splines`.
    SplineEdgeRouter,
    /// `WrappingStrategy::SingleEdge`.
    SingleEdgeGraphWrapper,
    /// `WrappingStrategy::MultiEdge`: the first of its three breaking-point processors.
    BreakingPointInserter,
    /// A graph with hypernodes.
    HypernodesProcessor,
}

fn refused(refusal: Refusal) -> PipelineError {
    PipelineError::UnsupportedProcessor {
        kind: refusal.processor,
    }
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
    fn select(strategy: LayeringStrategy) -> Result<Self, Refusal> {
        match strategy {
            LayeringStrategy::NetworkSimplex => Ok(Self::NetworkSimplex),
            LayeringStrategy::LongestPath => Ok(Self::LongestPath),
            LayeringStrategy::LongestPathSource => Ok(Self::LongestPathSource),
            LayeringStrategy::CoffmanGraham => Ok(Self::CoffmanGraham),
            LayeringStrategy::Interactive => Ok(Self::Interactive),
            LayeringStrategy::StretchWidth => Ok(Self::StretchWidth),
            LayeringStrategy::MinWidth => Ok(Self::MinWidth),
            LayeringStrategy::BreadthFirstModelOrder => Err(Refusal::new(
                Place::In(LayeredPhase::P2Layering),
                UnportedProcessor::BreadthFirstModelOrderLayerer,
            )),
            LayeringStrategy::DepthFirstModelOrder => Err(Refusal::new(
                Place::In(LayeredPhase::P2Layering),
                UnportedProcessor::DepthFirstModelOrderLayerer,
            )),
        }
    }
}

impl CrossingMinimizer {
    fn select(strategy: CrossingMinimizationStrategy) -> Result<Self, Refusal> {
        match strategy {
            CrossingMinimizationStrategy::LayerSweep => Ok(Self::LayerSweep),
            CrossingMinimizationStrategy::None => Ok(Self::NoMinimizer),
            CrossingMinimizationStrategy::Interactive => Err(Refusal::new(
                Place::In(LayeredPhase::P3NodeOrdering),
                UnportedProcessor::InteractiveCrossingMinimizer,
            )),
        }
    }
}

impl NodePlacer {
    fn select(strategy: NodePlacementStrategy) -> Result<Self, Refusal> {
        match strategy {
            NodePlacementStrategy::Simple => Ok(Self::Simple),
            NodePlacementStrategy::BrandesKoepf => Ok(Self::BrandesKoepf),
            NodePlacementStrategy::NetworkSimplex => Ok(Self::NetworkSimplex),
            NodePlacementStrategy::Interactive => Err(Refusal::new(
                Place::In(LayeredPhase::P4NodePlacement),
                UnportedProcessor::InteractiveNodePlacer,
            )),
            NodePlacementStrategy::LinearSegments => Err(Refusal::new(
                Place::In(LayeredPhase::P4NodePlacement),
                UnportedProcessor::LinearSegmentsNodePlacer,
            )),
        }
    }
}

impl EdgeRouter {
    fn select(strategy: EdgeRouting) -> Result<Self, Refusal> {
        let place = Place::In(LayeredPhase::P5EdgeRouting);
        match strategy {
            EdgeRouting::Orthogonal => Ok(Self::Orthogonal),
            EdgeRouting::Polyline => {
                Err(Refusal::new(place, UnportedProcessor::PolylineEdgeRouter))
            }
            EdgeRouting::Splines => Err(Refusal::new(place, UnportedProcessor::SplineEdgeRouter)),
        }
    }
}

/// The refusal of a wrapping strategy. Wrapping adds processors in front of a phase, not a phase.
fn wrapping_refusal(strategy: WrappingStrategy) -> Option<Refusal> {
    match strategy {
        WrappingStrategy::Off => None,
        WrappingStrategy::SingleEdge => Some(Refusal::new(
            Place::Before(LayeredPhase::P4NodePlacement),
            UnportedProcessor::SingleEdgeGraphWrapper,
        )),
        WrappingStrategy::MultiEdge => Some(Refusal::new(
            Place::Before(LayeredPhase::P3NodeOrdering),
            UnportedProcessor::BreakingPointInserter,
        )),
    }
}

/// The refusal of a graph with hypernodes.
fn hypernodes_refusal(properties: &GraphProperties) -> Option<Refusal> {
    properties.hypernodes.then(|| {
        Refusal::new(
            Place::After(LayeredPhase::P5EdgeRouting),
            UnportedProcessor::HypernodesProcessor,
        )
    })
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
    /// The refusal a run would reach first is the one it reports. Each refusal carries its
    /// [`Place`] in the schedule, so the order comes from there. Two refusals in one slot keep
    /// the order in which they are gathered below, so gather them in the order ELK's `EnumSet`
    /// runs them.
    pub(crate) fn select(
        options: &LayeredOptions,
        properties: &GraphProperties,
    ) -> PipelineResult<Self> {
        let cycle_breaker = CycleBreaker::select(options.cycle_breaking_strategy);
        let layerer = Layerer::select(options.layering_strategy);
        let crossing_minimizer = CrossingMinimizer::select(options.crossing_minimization_strategy);
        let node_placer = NodePlacer::select(options.node_placement_strategy);
        let edge_router = EdgeRouter::select(options.edge_routing);

        let refusals = [
            layerer.err(),
            wrapping_refusal(options.wrapping_strategy),
            crossing_minimizer.err(),
            node_placer.err(),
            edge_router.err(),
            hypernodes_refusal(properties),
        ];
        if let Some(first) = refusals
            .into_iter()
            .flatten()
            .min_by_key(|refusal| refusal.place.position())
        {
            return Err(refused(first));
        }
        Ok(Self {
            cycle_breaker,
            layerer: layerer.map_err(refused)?,
            crossing_minimizer: crossing_minimizer.map_err(refused)?,
            node_placer: node_placer.map_err(refused)?,
            edge_router: edge_router.map_err(refused)?,
        })
    }

    /// [`Strategies::select`] for a graph as its options and graph properties stand now.
    pub(crate) fn for_graph(graph: &LGraph) -> PipelineResult<Self> {
        Self::select(&graph.options, &graph.graph_properties)
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
        self.slots[phase.slot_before()].insert(processor);
    }

    fn add_after(&mut self, phase: LayeredPhase, processor: IntermediateProcessor) {
        self.slots[phase.slot_after()].insert(processor);
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

/// The processors of one graph as it stands now, in the order they run. The preprocessor and the
/// configurator change the node count and the graph properties, so a run calls this after them.
pub(crate) fn assemble_graph(graph: &LGraph) -> PipelineResult<Vec<Processor>> {
    let strategies = Strategies::for_graph(graph)?;
    Ok(assemble(
        strategies,
        &graph.options,
        &graph.graph_properties,
        graph.layerless_nodes.len(),
        graph.parent.is_none(),
    ))
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

/// Ports `GraphConfigurator.activateGreedySwitchFor` and the choice of the switch that follows it.
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

/// Ports `getLayoutProcessorConfiguration` of the cycle breakers (`GreedyCycleBreaker` and the
/// others). Only `InteractiveCycleBreaker` differs: it asks for the external port positioner.
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

/// Ports `getLayoutProcessorConfiguration` of the layerers (`NetworkSimplexLayerer` and the
/// others). Only `InteractiveLayerer` differs: it asks for the external port positioner, not the
/// edge and layer constraint reverser.
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

/// Ports `getLayoutProcessorConfiguration` of `LayerSweepCrossingMinimizer` and
/// `NoCrossingMinimizer`, which are the same.
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
