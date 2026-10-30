use super::*;

fn node(id: &str) -> ElkNode {
    ElkNode {
        id: id.into(),
        width: 10.0,
        height: 10.0,
        ..Default::default()
    }
}

fn port(id: &str) -> ElkPort {
    ElkPort {
        id: id.into(),
        width: 4.0,
        height: 4.0,
        ..Default::default()
    }
}

fn edge(id: &str, source: &str, target: &str) -> ElkEdge {
    ElkEdge {
        id: id.into(),
        sources: vec![source.into()],
        targets: vec![target.into()],
        ..Default::default()
    }
}

/// A group `g` with the ports `p`, `q` and `r` and the child `c`, which holds the grandchild `d`. The
/// node `o` lies beside `g`.
fn root_with(edges: Vec<ElkEdge>) -> ElkNode {
    let mut c = node("c");
    c.children = vec![node("d")];
    let mut g = node("g");
    g.ports = vec![port("p"), port("q"), port("r")];
    g.children = vec![c];
    ElkNode {
        id: "root".into(),
        children: vec![g, node("o")],
        edges,
        ..Default::default()
    }
}

fn facts_of_g(edges: Vec<ElkEdge>) -> Vec<PortFacts> {
    let root = root_with(edges);
    let tree = Tree::new(&root).unwrap();
    let g = tree.by_id["g"];
    tree.nodes[g].port_facts.clone()
}

// Mutation: leave out `facts.has_edges = true` for the source end or for the target end.
#[test]
fn a_port_has_edges_when_an_edge_starts_or_ends_there() {
    let facts = facts_of_g(vec![edge("e1", "p", "o"), edge("e2", "o", "q")]);
    assert!(facts[0].has_edges);
    assert!(facts[1].has_edges);
    let facts = facts_of_g(vec![edge("e1", "p", "o")]);
    assert!(facts[0].has_edges);
    assert!(!facts[1].has_edges);
}

// Mutation: set `inside_connections` for an edge to a node outside, or only for a direct child.
#[test]
fn a_port_has_inside_connections_when_a_descendant_is_at_the_other_end() {
    // A grandchild counts, in either direction. An outside node does not.
    let facts = facts_of_g(vec![edge("e1", "p", "d"), edge("e2", "d", "q")]);
    assert!(facts[0].inside_connections);
    assert!(facts[1].inside_connections);
    let facts = facts_of_g(vec![edge("e1", "p", "o"), edge("e2", "o", "q")]);
    assert!(!facts[0].inside_connections);
    assert!(!facts[1].inside_connections);
}

// Mutation: count the grandchild as a child in `child_edges`. ELK counts edges to children only,
// and the checks for external ports read that count.
#[test]
fn only_an_edge_to_a_direct_child_counts_as_a_child_edge() {
    let facts = facts_of_g(vec![
        edge("e1", "p", "c"),
        edge("e2", "d", "q"),
        edge("e3", "r", "d"),
    ]);
    assert_eq!(facts[0].child_edges, 1);
    assert_eq!(facts[1].child_edges, 0);
    assert_eq!(facts[2].child_edges, 0);
}

// Mutation: set `connected_to_external_nodes` for an edge to a descendant.
#[test]
fn a_port_is_connected_to_external_nodes_when_a_node_outside_is_at_the_other_end() {
    let facts = facts_of_g(vec![edge("e1", "p", "c"), edge("e2", "o", "q")]);
    assert!(!facts[0].connected_to_external_nodes);
    assert!(facts[1].connected_to_external_nodes);
}

// Mutation, one per row: drop `source == target` (row 3), drop `yo` (row 2) or drop `activate`
// (row 1) from `is_inside_self_loop`. The row's answer then flips to true.
#[test]
fn an_edge_is_an_inside_self_loop_only_when_all_three_inputs_hold() {
    // (source, target, yo, activate, expected)
    let rows = [
        (1, 1, true, false, false),
        (1, 1, false, true, false),
        (1, 2, true, true, false),
        (1, 1, true, true, true),
    ];
    for (source, target, yo, activate, expected) in rows {
        assert_eq!(
            is_inside_self_loop(source, target, yo, activate),
            expected,
            "source {source}, target {target}, yo {yo}, activate {activate}"
        );
    }
}

// Mutation: build `EdgeInfo` with `settings.inside_self_loops_yo` as `inside_self_loop`. A `yo`
// on an edge between two nodes then reaches the importer as an inside self loop.
#[test]
fn an_edge_between_two_activating_nodes_ignores_yo() {
    let mut g = node("g");
    let mut h = node("h");
    for n in [&mut g, &mut h] {
        n.layout_options
            .insert("elk.insideSelfLoops.activate".into(), "true".into());
    }
    let mut between = edge("gh", "g", "h");
    between
        .layout_options
        .insert("elk.insideSelfLoops.yo".into(), "true".into());
    let root = |edge: ElkEdge| ElkNode {
        id: "root".into(),
        children: vec![g.clone(), h.clone()],
        edges: vec![edge],
        ..Default::default()
    };
    let tree_root = root(between);
    let tree = Tree::new(&tree_root).unwrap();
    assert!(!tree.edges[0].inside_self_loop);
}
