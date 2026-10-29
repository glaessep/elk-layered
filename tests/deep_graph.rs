//! A graph 10,000 nodes deep lays out without a stack overflow.
//!
//! The layout runs on a thread with a small stack, so a step that recurses once per node fails
//! here on every platform. A stack overflow aborts the whole test run; it cannot be caught.

use std::collections::BTreeMap;

use elk_layered::{ElkEdge, ElkNode, layout};

const DEPTH: usize = 10_000;

/// Far less than the 2 MiB a Rust thread gets by default. A step that recurses once per node of
/// the chain needs more than this, in a debug and in a release build.
const STACK: usize = 256 * 1024;

/// The chain `n0 -> n1 -> ... -> n9999`, with an edge `n9999 -> n0` that closes a cycle through
/// every node, so the cycle breaker and the network simplex both walk the whole chain.
fn closed_chain() -> ElkNode {
    let edge = |id: String, source: usize, target: usize| ElkEdge {
        id,
        sources: vec![format!("n{source}")],
        targets: vec![format!("n{target}")],
        ..ElkEdge::default()
    };
    let mut edges: Vec<ElkEdge> = (1..DEPTH)
        .map(|i| edge(format!("e{i}"), i - 1, i))
        .collect();
    edges.push(edge("back".into(), DEPTH - 1, 0));
    ElkNode {
        id: "root".to_string(),
        layout_options: BTreeMap::from([
            ("elk.algorithm".to_string(), "layered".to_string()),
            (
                "elk.layered.cycleBreaking.strategy".to_string(),
                "DEPTH_FIRST".to_string(),
            ),
        ]),
        children: (0..DEPTH)
            .map(|i| ElkNode {
                id: format!("n{i}"),
                width: 20.0,
                height: 20.0,
                ..ElkNode::default()
            })
            .collect(),
        edges,
        ..ElkNode::default()
    }
}

// Mutation: let `connected_components_dfs` of the network simplex layerer, `tight_tree_dfs` or
// `postorder_traversal` of the network simplex, or `collect_depth_first_back_edges` recurse again.
#[test]
fn a_graph_10000_nodes_deep_lays_out_on_a_small_stack() {
    let graph = closed_chain();
    let laid_out = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || layout(&graph).map(|out| out.children.len()))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(laid_out, Ok(DEPTH));
}
