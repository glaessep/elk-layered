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
