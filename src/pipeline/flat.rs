//! The run of a graph without a hierarchy, or of a hierarchy laid out one graph at a time.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/ElkLayered.java

use super::assembler::{Processor, Strategies, assemble_graph};
use super::execute::execute_in_arena;
use super::{PipelineResult, check_external_port_sides, validate_node_size_minima};
use crate::components::ComponentSplit;
use crate::configurator::{check_random_seeds, configure_graph_properties};
use crate::intermediate::hierarchical_node_resizing_processor::resize_graph_to_minimum;

use crate::graph::{GraphArena, GraphId};

/// Execute all currently source-ported processors assembled for this graph.
///
/// This is the library equivalent of `ElkLayered.layout(...)`. It rejects a zero seed and an
/// option that needs a processor that is not ported before it changes the graph.
///
/// After an error from a later step, `graphs` may be half changed, so drop it. An error between
/// the component split and its combine, for one, leaves the references of the nested graphs half
/// renumbered.
pub(crate) fn execute_ported_processors(graphs: &mut GraphArena) -> PipelineResult<Vec<Processor>> {
    let mut executed = Vec::new();
    // Each of these checks only reads the graph.
    check_random_seeds(graphs)?;
    check_external_port_sides(graphs)?;
    let processors = prepare_single_graph_processors(graphs)?;

    if let Some(mut components) = crate::components::split(graphs)? {
        // ComponentsProcessor copies the configured properties shallowly. Its Java Random
        // is one shared object, so later components continue the previous component's stream.
        let mut random = graphs.root().random.clone();
        // The components stand in for the root: each takes the root's place while it runs, so
        // the graphs below the root are the others it reads.
        for component in &mut components {
            component.graph.random = random;
            std::mem::swap(&mut graphs[GraphId::ROOT], &mut component.graph);
            let result = processors.iter().try_for_each(|&processor| {
                execute_in_arena(
                    graphs,
                    GraphId::ROOT,
                    false,
                    ComponentSplit::Active,
                    processor,
                )
            });
            std::mem::swap(&mut graphs[GraphId::ROOT], &mut component.graph);
            result?;
            random = component.graph.random.clone();
        }
        crate::components::combine(graphs, components)?;
        graphs.root_mut().random = random;
        executed.extend(processors);
    } else {
        for processor in processors {
            execute_in_arena(
                graphs,
                GraphId::ROOT,
                false,
                ComponentSplit::Inactive,
                processor,
            )?;
            executed.push(processor);
        }
    }

    // Compound layout resizes through its scheduled HierarchicalNodeResizer. Flat layout
    // applies ElkLayered.resizeGraph after direction postprocessing and component combination,
    // whether or not a minimum size is set: it recomputes the size from the padding either way.
    let graph = graphs.root_mut();
    if graph.options.hierarchy_handling != crate::options::HierarchyHandling::IncludeChildren {
        resize_graph_to_minimum(graph)?;
    }

    Ok(executed)
}

/// The processor list of the root graph. It checks the strategies first, so a processor that is
/// not ported refuses the run while the graph is still as the caller gave it, and builds the list
/// after the configurator has run, from the graph as it stands then.
fn prepare_single_graph_processors(graphs: &mut GraphArena) -> PipelineResult<Vec<Processor>> {
    // The options decide whether a processor is ported, so this check needs no list yet.
    Strategies::for_graph(graphs.root())?;
    // Processor execution below is root-local, but the source-backed GraphConfigurator walks every
    // nested graph to resolve options and random seeds.
    validate_node_size_minima(graphs)?;
    configure_graph_properties(graphs)?;
    assemble_graph(graphs.root())
}

/// Run the assembled processors of the root graph up to and including the first one `is_last`
/// accepts.
///
/// Tests use this to stop at a phase boundary with exactly the state the full runner has there:
/// the configurator, the edge and layer constraint steps and every other scheduled processor.
/// The graphs inside nodes stay as they are, in `graphs`.
#[cfg(test)]
fn run_processors_until_processor(
    graphs: &mut GraphArena,
    is_last: impl Fn(Processor) -> bool,
) -> PipelineResult<Vec<Processor>> {
    let mut executed = Vec::new();
    prepare_single_graph_processors(graphs).and_then(|processors| {
        for processor in processors {
            execute_in_arena(
                graphs,
                GraphId::ROOT,
                false,
                ComponentSplit::Inactive,
                processor,
            )?;
            executed.push(processor);
            if is_last(processor) {
                break;
            }
        }
        Ok(())
    })?;
    Ok(executed)
}

/// Run the assembled processors up to and including `target`.
#[cfg(test)]
pub(crate) fn run_processors_until(
    graph: &mut GraphArena,
    target: Processor,
) -> PipelineResult<Vec<Processor>> {
    run_processors_until_processor(graph, |processor| processor == target)
}

/// Run the assembled processors up to and including the one that carries `phase`.
#[cfg(test)]
pub(crate) fn run_processors_until_phase(
    graph: &mut GraphArena,
    phase: super::LayeredPhase,
) -> PipelineResult<Vec<Processor>> {
    run_processors_until_processor(graph, |processor| processor.phase() == Some(phase))
}
