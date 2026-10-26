//! Layered graph configuration before processor assembly.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/GraphConfigurator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/options/Spacings.java

use crate::graph::LGraph;
use crate::options::{EdgeRouting, ElkDirection, LayeredOptions};
use crate::random::{JavaRandom, RandomSeedError};

const MIN_EDGE_SPACING: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayeredSpacings {
    pub edge_edge: f64,
}

impl Default for LayeredSpacings {
    fn default() -> Self {
        Self {
            edge_edge: MIN_EDGE_SPACING,
        }
    }
}

/// Configure graph properties before assembling processors.
///
/// This mirrors the non-mutating Rust subset of `GraphConfigurator.configureGraphProperties(...)`:
/// keep discovered graph properties synchronized with options, normalize undefined direction, enforce
/// minimum edge-edge spacing, and set the default straight-edge preference based on edge routing.
///
/// Every graph in the hierarchy seeds its own `JavaRandom` with `random_seed`, the way ELK does.
/// A seed of 0 is an error: ELK reads it as a time-based seed, and this crate stays deterministic.
pub fn configure_graph_properties(graph: &mut LGraph) -> Result<(), RandomSeedError> {
    graph.try_for_each_graph_mut(|graph| {
        graph.graph_properties.apply_to_options(&mut graph.options);
        if graph.options.direction == ElkDirection::Undefined {
            graph.options.direction = ElkDirection::Right;
        }
        if graph.options.spacing.edge_edge < MIN_EDGE_SPACING {
            graph.options.spacing.edge_edge = MIN_EDGE_SPACING;
        }
        if graph.options.node_placement_favor_straight_edges.is_none() {
            graph.options.node_placement_favor_straight_edges =
                Some(graph.options.edge_routing == EdgeRouting::Orthogonal);
        }
        if graph.options.random_seed == 0 {
            return Err(RandomSeedError::ZeroSeed {
                graph_id: graph.id.clone(),
            });
        }
        graph.random = JavaRandom::new(i64::from(graph.options.random_seed));
        Ok(())
    })
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
mod tests {
    use super::*;
    use crate::LGraph;

    #[test]
    fn upstream_zero_seed_is_an_error_at_the_configurator_boundary() {
        let options = LayeredOptions {
            random_seed: 0,
            ..Default::default()
        };
        let mut graph = LGraph::new("root", options);

        assert_eq!(
            configure_graph_properties(&mut graph),
            Err(RandomSeedError::ZeroSeed {
                graph_id: "root".to_string(),
            })
        );
    }

    #[test]
    fn each_graph_seeds_its_random_with_its_own_signed_seed() {
        let mut root = LGraph::new(
            "root",
            LayeredOptions {
                random_seed: i32::MIN,
                ..Default::default()
            },
        );
        let mut holder = crate::LNode::new("holder", 10.0, 10.0, None);
        holder.nested_graph = Some(Box::new(LGraph::new(
            "child",
            LayeredOptions {
                random_seed: -7,
                ..Default::default()
            },
        )));
        root.layerless_nodes.push(holder);

        configure_graph_properties(&mut root).unwrap();

        assert_eq!(root.random, JavaRandom::new(i64::from(i32::MIN)));
        let child = root.layerless_nodes[0].nested_graph.as_deref().unwrap();
        assert_eq!(child.random, JavaRandom::new(-7));
    }

    #[test]
    fn nested_graph_with_zero_seed_names_itself_in_the_error() {
        let mut root = LGraph::new("root", LayeredOptions::default());
        let mut holder = crate::LNode::new("holder", 10.0, 10.0, None);
        holder.nested_graph = Some(Box::new(LGraph::new(
            "child",
            LayeredOptions {
                random_seed: 0,
                ..Default::default()
            },
        )));
        root.layerless_nodes.push(holder);

        assert_eq!(
            configure_graph_properties(&mut root),
            Err(RandomSeedError::ZeroSeed {
                graph_id: "child".to_string(),
            })
        );
    }
}
