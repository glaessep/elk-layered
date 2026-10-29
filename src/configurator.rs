//! Layered graph configuration before processor assembly.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/options/Spacings.java

use crate::graph::{GraphArena, GraphId, LGraph};
use crate::options::{EdgeRouting, ElkDirection, LayeredOptions};
use crate::random::{JavaRandom, RandomSeedError};

const MIN_EDGE_SPACING: f64 = 2.0;

/// Configure graph properties before assembling processors.
///
/// This mirrors the non-mutating Rust subset of `GraphConfigurator.configureGraphProperties(...)`:
/// keep discovered graph properties synchronized with options, normalize undefined direction, enforce
/// minimum edge-edge spacing, and set the default straight-edge preference based on edge routing.
///
/// Every graph in the hierarchy seeds its own `JavaRandom` with `random_seed`, the way ELK does.
/// A seed of 0 is an error: ELK reads it as a time-based seed, and this crate stays deterministic.
pub fn configure_graph_properties(arena: &mut GraphArena) -> Result<(), RandomSeedError> {
    arena.try_for_each_graph_mut(|graph| {
        // Runners call `check_random_seeds` before they change anything, so this check does not
        // fire for them. It stays as the configurator's own guard against a seed of 0.
        if graph.options.random_seed == 0 {
            return Err(RandomSeedError::ZeroSeed {
                graph_id: graph.name.clone(),
            });
        }
        graph.options = configured_options(graph);
        graph.random = JavaRandom::new(i64::from(graph.options.random_seed));
        Ok(())
    })
}

/// Reject a seed of 0 anywhere in the hierarchy without changing the graph.
///
/// The runners call this first, so a rejected graph stays exactly as the caller gave it. It visits
/// graphs in the order `configure_graph_properties` does, so both name the same graph.
pub(crate) fn check_random_seeds(arena: &GraphArena) -> Result<(), RandomSeedError> {
    for id in arena.preorder(GraphId::ROOT) {
        let current = &arena[id];
        if current.options.random_seed == 0 {
            return Err(RandomSeedError::ZeroSeed {
                graph_id: current.name.clone(),
            });
        }
    }
    Ok(())
}

pub fn configured_options(graph: &LGraph) -> LayeredOptions {
    let mut options = graph.options.clone();
    graph.graph_properties.apply_to_options(&mut options);
    if options.direction == ElkDirection::Undefined {
        options.direction = ElkDirection::Right;
    }
    if options.spacing.edge_edge < MIN_EDGE_SPACING {
        options.spacing.edge_edge = MIN_EDGE_SPACING;
    }
    if options.node_placement_favor_straight_edges.is_none() {
        options.node_placement_favor_straight_edges =
            Some(options.edge_routing == EdgeRouting::Orthogonal);
    }
    options
}

#[cfg(test)]
mod tests;
