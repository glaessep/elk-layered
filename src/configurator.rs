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
mod tests {
    use super::*;
    use crate::graph::NodeId;

    fn arena(graph: LGraph) -> GraphArena {
        GraphArena::new(graph)
    }

    // Mutation: leave out the zero-seed check in `configure_graph_properties`. Seed 0 is then
    // accepted.
    #[test]
    fn upstream_zero_seed_is_an_error_at_the_configurator_boundary() {
        let options = LayeredOptions {
            random_seed: 0,
            ..Default::default()
        };
        let mut graphs = arena(LGraph::new("root", options));

        assert_eq!(
            configure_graph_properties(&mut graphs),
            Err(RandomSeedError::ZeroSeed {
                graph_id: "root".to_string(),
            })
        );
    }

    // Mutation: move the zero-seed check in `configure_graph_properties` below
    // `configured_options`. The graph is then rewritten before the error.
    #[test]
    fn zero_seed_graph_is_not_rewritten_before_the_error() {
        let mut graphs = arena(LGraph::new(
            "root",
            LayeredOptions {
                random_seed: 0,
                direction: ElkDirection::Undefined,
                ..Default::default()
            },
        ));
        let before = graphs.clone();

        assert!(configure_graph_properties(&mut graphs).is_err());

        assert_eq!(graphs, before);
    }

    // Mutation: seed the random with the unsigned seed (`random_seed as u32`). A negative seed then
    // gives another stream.
    #[test]
    fn each_graph_seeds_its_random_with_its_own_signed_seed() {
        let mut graphs = arena(LGraph::new(
            "root",
            LayeredOptions {
                random_seed: i32::MIN,
                ..Default::default()
            },
        ));
        graphs
            .root_mut()
            .layerless_nodes
            .push(crate::graph::LNode::new("holder", 10.0, 10.0, None));
        let child = graphs.nest(
            GraphId::ROOT,
            NodeId::new(0),
            LGraph::new(
                "child",
                LayeredOptions {
                    random_seed: -7,
                    ..Default::default()
                },
            ),
        );

        configure_graph_properties(&mut graphs).unwrap();

        assert_eq!(graphs.root().random, JavaRandom::new(i64::from(i32::MIN)));
        assert_eq!(graphs[child].random, JavaRandom::new(-7));
    }

    // Mutation: leave the graph's name out of the error (`String::new()`). The error then names
    // no graph.
    #[test]
    fn nested_graph_with_zero_seed_names_itself_in_the_error() {
        let mut graphs = arena(LGraph::new("root", LayeredOptions::default()));
        graphs
            .root_mut()
            .layerless_nodes
            .push(crate::graph::LNode::new("holder", 10.0, 10.0, None));
        graphs.nest(
            GraphId::ROOT,
            NodeId::new(0),
            LGraph::new(
                "child",
                LayeredOptions {
                    random_seed: 0,
                    ..Default::default()
                },
            ),
        );

        assert_eq!(
            configure_graph_properties(&mut graphs),
            Err(RandomSeedError::ZeroSeed {
                graph_id: "child".to_string(),
            })
        );
    }

    // Mutation: walk the graphs breadth-first in `check_random_seeds`. It then names "child-b",
    // while the configurator names "grandchild-a".
    #[test]
    fn seed_check_names_the_same_graph_as_the_configurator_and_changes_nothing() {
        // "grandchild-a" and "child-b" both have seed 0. Depth-first order reaches the grandchild
        // first, a breadth-first walk would name "child-b".
        let mut graphs = arena(LGraph::new("root", LayeredOptions::default()));
        for (position, (holder, child, seed)) in [("h0", "child-a", 3), ("h1", "child-b", 0)]
            .into_iter()
            .enumerate()
        {
            graphs
                .root_mut()
                .layerless_nodes
                .push(crate::graph::LNode::new(holder, 10.0, 10.0, None));
            let nested = graphs.nest(
                GraphId::ROOT,
                NodeId::new(position),
                LGraph::new(
                    child,
                    LayeredOptions {
                        random_seed: seed,
                        ..Default::default()
                    },
                ),
            );
            graphs[nested]
                .layerless_nodes
                .push(crate::graph::LNode::new(
                    format!("{holder}-inner"),
                    10.0,
                    10.0,
                    None,
                ));
            graphs.nest(
                nested,
                NodeId::new(0),
                LGraph::new(
                    format!("grandchild-{}", &child[6..]),
                    LayeredOptions {
                        random_seed: if seed == 3 { 0 } else { 5 },
                        ..Default::default()
                    },
                ),
            );
        }
        let before = graphs.clone();

        let checked = check_random_seeds(&graphs);

        assert_eq!(graphs, before);
        assert_eq!(
            checked,
            Err(RandomSeedError::ZeroSeed {
                graph_id: "grandchild-a".to_string(),
            })
        );
        assert_eq!(configure_graph_properties(&mut graphs), checked);
    }
}
