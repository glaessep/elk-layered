//! The compound run: every graph of a hierarchy, deepest first, in one schedule.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/ElkLayered.java

use super::assembler::{IntermediateProcessor, Processor, Strategies, assemble};
use super::execute::{execute_processor, resize_layered_node};
use super::{
    PipelineError, PipelineResult, actual_graph_size, check_external_port_sides,
    validate_node_size_minima,
};
use crate::compound::preprocess_source_ported_compound_graph;
use crate::configurator::{check_random_seeds, configure_graph_properties};
use crate::graph::{
    GraphArena, GraphId, GraphParent, LGraph, LNodeKind, LPoint, LSize, NodeId, PortSide,
};
use crate::options::PortConstraints;
use crate::p3order::sweep::minimize_crossings_layer_sweep_hierarchical_with_type;

/// What ran on one graph of a compound run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphExecution {
    /// The graph the processors run on.
    pub graph: GraphId,
    /// The caller's id of the graph, as `LGraph::name` keeps it.
    pub graph_name: String,
    /// The node the graph lies in. `None` for the root.
    pub parent: Option<GraphParent>,
    /// The processors that ran on the graph, in the order they ran.
    pub processors: Vec<Processor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GraphAlgorithm {
    pub(super) next_processor: usize,
    pub(super) processors: Vec<Processor>,
    pub(super) execution: GraphExecution,
}

/// The algorithms of a compound run, one per graph, in the order ELK finds the graphs.
#[derive(Debug)]
pub(super) struct CompoundRun {
    pub(super) algorithms: Vec<GraphAlgorithm>,
}

impl CompoundRun {
    /// `selected` holds the strategies of every graph, in discovery order, from
    /// [`select_strategies`]. The preprocessor and the configurator have run since, so each list
    /// is built from the graph as it is now. ELK finds the graphs again after them, so this does
    /// too, and refuses a hierarchy whose graphs are not the ones that were selected.
    pub(super) fn new(
        graphs: &GraphArena,
        selected: Vec<(GraphId, Strategies)>,
    ) -> PipelineResult<Self> {
        let found = graphs.discovery_order(GraphId::ROOT);
        if found.len() != selected.len()
            || found
                .iter()
                .zip(&selected)
                .any(|(id, (chosen, _))| id != chosen)
        {
            return Err(PipelineError::UnsupportedCompoundGraph {
                reason: "the graphs of the hierarchy changed between the choice of the strategies and the run",
            });
        }
        let algorithms = selected
            .into_iter()
            .map(|(graph, strategies)| graph_algorithm(&graphs[graph], strategies))
            .collect();
        Ok(Self { algorithms })
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

/// The strategies of every graph of the hierarchy, in discovery order.
///
/// ELK finds the graphs with `ArrayDeque.push`: the last sibling is searched first, and the
/// collected list is later executed in reverse discovery order. The first refusal is the one of
/// the graph that runs first, so the graphs are selected in that reverse order.
pub(super) fn select_strategies(graphs: &GraphArena) -> PipelineResult<Vec<(GraphId, Strategies)>> {
    let mut selected = graphs
        .discovery_order(GraphId::ROOT)
        .into_iter()
        .rev()
        .map(|id| {
            let graph = &graphs[id];
            Strategies::select(&graph.options, &graph.graph_properties)
                .map(|strategies| (id, strategies))
        })
        .collect::<PipelineResult<Vec<_>>>()?;
    selected.reverse();
    Ok(selected)
}

fn graph_algorithm(graph: &LGraph, strategies: Strategies) -> GraphAlgorithm {
    GraphAlgorithm {
        next_processor: 0,
        processors: assemble(
            strategies,
            &graph.options,
            &graph.graph_properties,
            graph.layerless_nodes.len(),
            graph.parent.is_none(),
        ),
        execution: GraphExecution {
            graph: graph.id,
            graph_name: graph.name.clone(),
            parent: graph.parent,
            processors: Vec::new(),
        },
    }
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
    graphs: &mut GraphArena,
) -> PipelineResult<Vec<GraphExecution>> {
    // Every graph names its strategies first, so a processor that is not ported refuses the run
    // before anything changes. The lists are built later, from the configured graphs.
    let selected = select_strategies(graphs)?;
    validate_node_size_minima(graphs)?;
    // Check every seed before the preprocessor and the configurator change the caller's graph.
    check_random_seeds(graphs)?;
    preprocess_source_ported_compound_graph(graphs);
    check_external_port_sides(graphs)?;
    configure_graph_properties(graphs)?;
    review_and_correct_hierarchical_processors(graphs)?;

    let mut run = CompoundRun::new(graphs, selected)?;
    const ROOT_ALGORITHM: usize = 0;

    while run.algorithms[ROOT_ALGORITHM].next_processor
        < run.algorithms[ROOT_ALGORITHM].processors.len()
    {
        execute_compound_graph_round(graphs, &mut run.algorithms)?;
    }

    Ok(run.into_executions())
}

/// One round: every nested graph, deepest first, runs until it reaches a hierarchy-aware
/// processor or the end of its list. Then the root does the same.
pub(super) fn execute_compound_graph_round(
    graphs: &mut GraphArena,
    algorithms: &mut [GraphAlgorithm],
) -> PipelineResult<()> {
    for index in (1..algorithms.len()).rev() {
        execute_compound_algorithm_until_pause(graphs, &mut algorithms[index])?;
    }
    execute_compound_algorithm_until_pause(graphs, &mut algorithms[0])
}

fn execute_compound_algorithm_until_pause(
    graphs: &mut GraphArena,
    algorithm: &mut GraphAlgorithm,
) -> PipelineResult<()> {
    let is_root = algorithm.execution.parent.is_none();
    while algorithm.next_processor < algorithm.processors.len() {
        let processor = algorithm.processors[algorithm.next_processor];
        algorithm.next_processor += 1;

        let hierarchy_aware = processor.hierarchy_aware_sweep();
        if hierarchy_aware.is_some() && !is_root {
            break;
        }

        if let Some(sweep) = hierarchy_aware {
            minimize_crossings_layer_sweep_hierarchical_with_type(graphs, sweep);
        } else {
            let (graph, others) = graphs.split_mut(algorithm.execution.graph);
            execute_processor(graph, others, processor)?;
        }
        let size = actual_graph_size(&graphs[algorithm.execution.graph]);
        if processor == Processor::Intermediate(IntermediateProcessor::HierarchicalNodeResizer)
            && let Some(parent) = algorithm.execution.parent
        {
            let (nested, parent_graph) = graphs.pair_mut(algorithm.execution.graph, parent.graph);
            transfer_nested_graph_layout_to_parent_node(nested, parent_graph, parent.node, size);
        }
        algorithm.execution.processors.push(processor);

        if hierarchy_aware.is_some() {
            break;
        }
    }

    Ok(())
}

fn review_and_correct_hierarchical_processors(graphs: &mut GraphArena) -> PipelineResult<()> {
    let root_crossing = graphs.root().options.crossing_minimization_strategy;
    let root_greedy_switch = graphs.root().options.greedy_switch_hierarchical_type;
    graphs.try_for_each_graph_mut(|graph| {
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
    node_index: NodeId,
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

fn transfer_external_port_dummy_layout_to_parent_node(
    nested_graph: &mut LGraph,
    parent_node_index: NodeId,
    parent_ports: &mut crate::graph::NodePorts,
) {
    for dummy_index in nested_graph.node_ids() {
        if nested_graph.layerless_nodes[dummy_index].kind() != LNodeKind::ExternalPort {
            continue;
        }
        let Some(external) = nested_graph.layerless_nodes[dummy_index].external_port() else {
            continue;
        };
        let Some(origin_port) = external.origin_port else {
            continue;
        };
        if origin_port.port.node != parent_node_index {
            continue;
        }

        let external_side = external.side;
        let external_size = external.size;
        // The size of the port as the parent graph holds it now: the parent graph has turned
        // to the internal direction already, and the dummy is still in that direction. The
        // dummy moves even without its port, as `LGraphUtil.getExternalPortPosition` moves it
        // for every dummy, so its edges leave from where it ends up.
        let size = parent_ports
            .get(origin_port.port.port)
            .map_or(external_size, |port| port.size);
        let port_position = external_port_position(nested_graph, dummy_index, size);
        if let Some(parent_port) = parent_ports.get_mut(origin_port.port.port) {
            parent_port.position = port_position;
            parent_port.set_side(external_side);
        }
    }
}

/// Ports `LGraphUtil.getExternalPortPosition`: the position of the port a dummy stands for,
/// relative to the graph's node. It moves the dummy as well.
pub(crate) fn external_port_position(
    graph: &mut LGraph,
    dummy_index: NodeId,
    external_size: LSize,
) -> LPoint {
    let dummy_size = graph.layerless_nodes[dummy_index].size;
    // A node that is no external port dummy has no side to place it on.
    let external_side = graph.layerless_nodes[dummy_index].external_port_side();
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
