use super::*;

#[test]
fn node_coordinates_do_not_allocate_a_dense_layer_count() {
    let mut graph = NGraph::new();
    let top = graph.add_node(None);
    let bottom = graph.add_node(None);
    graph
        .add_edge(None, top, bottom, 10_000.0, i32::MAX)
        .unwrap();

    NetworkSimplex::for_graph(&mut graph)
        .with_balancing(false)
        .execute()
        .unwrap();

    assert_eq!(graph.nodes[top].layer, 0);
    assert_eq!(graph.nodes[bottom].layer, i32::MAX);
}

#[test]
fn coordinate_overflow_is_an_error_instead_of_wrapped_geometry() {
    let mut graph = NGraph::new();
    let a = graph.add_node(None);
    let b = graph.add_node(None);
    let c = graph.add_node(None);
    graph.add_edge(None, a, b, 1.0, i32::MAX).unwrap();
    graph.add_edge(None, b, c, 1.0, 1).unwrap();

    assert_eq!(
        NetworkSimplex::for_graph(&mut graph).execute(),
        Err(NetworkSimplexError::CoordinateOutOfRange),
    );
}

// Mutation: add_node drops the origin it is given (stores None).
#[test]
fn nodes_and_edges_keep_the_origin_ids_they_are_given() {
    let mut graph = NGraph::new();
    let a = graph.add_node(Some(NodeId::new(7)));
    let b = graph.add_node(None);
    let edge = graph.add_edge(Some(EdgeId::new(3)), a, b, 1.0, 1).unwrap();

    assert_eq!(graph.nodes[a].origin, Some(NodeId::new(7)));
    assert_eq!(graph.nodes[b].origin, None);
    assert_eq!(graph.edges[edge].origin, Some(EdgeId::new(3)));
}

// Mutation: `normalize` shifts the layers by the highest layer instead of the lowest.
#[test]
fn network_simplex_assigns_layers_to_dag() {
    let mut graph = NGraph::new();
    let a = graph.add_node(None);
    let b = graph.add_node(None);
    let c = graph.add_node(None);
    graph.add_edge(None, a, b, 1.0, 1).unwrap();
    graph.add_edge(None, b, c, 1.0, 1).unwrap();

    NetworkSimplex::for_graph(&mut graph)
        .with_iteration_limit(28)
        .with_balancing(true)
        .execute()
        .unwrap();

    assert_eq!(graph.nodes[a].layer, 0);
    assert!(graph.nodes[b].layer > graph.nodes[a].layer);
    assert!(graph.nodes[c].layer > graph.nodes[b].layer);
}

// Mutation: `balance` returns at once, so it never moves a node to a less filled layer.
#[test]
fn network_simplex_balances_equal_degree_node() {
    // `b` has one edge in and one out, and may sit in layer 1 or 2. The long path a, d, e, c
    // puts `d` in layer 1 and `e` in layer 2, so balancing has a choice to make.
    let mut graph = NGraph::new();
    let a = graph.add_node(None);
    let b = graph.add_node(None);
    let c = graph.add_node(None);
    let d = graph.add_node(None);
    let e = graph.add_node(None);
    graph.add_edge(None, a, b, 1.0, 1).unwrap();
    graph.add_edge(None, b, c, 1.0, 1).unwrap();
    graph.add_edge(None, a, d, 1.0, 1).unwrap();
    graph.add_edge(None, d, e, 1.0, 1).unwrap();
    graph.add_edge(None, e, c, 1.0, 1).unwrap();

    NetworkSimplex::for_graph(&mut graph)
        .with_iteration_limit(28)
        .with_balancing(true)
        .execute()
        .unwrap();

    assert_eq!(graph.nodes[a].layer, 0);
    assert_eq!(graph.nodes[d].layer, 1);
    assert_eq!(graph.nodes[e].layer, 2);
    assert_eq!(graph.nodes[b].layer, 2);
    assert_eq!(graph.nodes[c].layer, 3);
}

// Mutation: `reattach_subtrees` does not call `add_active_node` for the node it puts back.
#[test]
fn network_simplex_reattaches_removed_subtrees() {
    let mut graph = NGraph::new();
    let mut nodes = Vec::new();
    for _ in 0..41 {
        nodes.push(graph.add_node(None));
    }
    for index in 0..40 {
        graph
            .add_edge(None, nodes[index], nodes[index + 1], 1.0, 1)
            .unwrap();
    }

    NetworkSimplex::for_graph(&mut graph)
        .with_iteration_limit(28)
        .with_balancing(true)
        .execute()
        .unwrap();

    assert_eq!(graph.active_nodes().len(), 41);
    for edge in &graph.edges {
        assert!(graph.nodes[edge.target].layer > graph.nodes[edge.source].layer);
    }
}
