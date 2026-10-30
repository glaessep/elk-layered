//! Deep graphs lay out without a stack overflow: a chain 10,000 nodes deep, and a staircase of
//! blocks for the Brandes-Koepf placement.
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

/// Steps in the staircase. Each step is one block that `place_block` waits on.
const STEPS: usize = 4_000;

/// A staircase of blocks. Layer `i` holds `n{i}` above `m{i}`, and the edges are
/// `m{i} -> m{i+1}` and `n{i} -> m{i+1}`. The Brandes-Koepf aligner joins `n{i}` and `m{i+1}` into
/// one block, whose root `n{i}` sits one layer earlier than the root of the block below it.
/// Placing block `i` needs block `i + 1` first, then block `i + 2`, and so on down the staircase.
fn staircase() -> ElkNode {
    let edge = |id: String, source: String, target: String| ElkEdge {
        id,
        sources: vec![source],
        targets: vec![target],
        ..ElkEdge::default()
    };
    let node = |id: String| ElkNode {
        id,
        width: 20.0,
        height: 20.0,
        ..ElkNode::default()
    };
    let mut children = vec![node("m0".into())];
    let mut edges = Vec::new();
    for i in 1..STEPS {
        children.push(node(format!("m{i}")));
        children.push(node(format!("n{}", i - 1)));
        edges.push(edge(
            format!("em{i}"),
            format!("m{}", i - 1),
            format!("m{i}"),
        ));
        edges.push(edge(
            format!("en{i}"),
            format!("n{}", i - 1),
            format!("m{i}"),
        ));
    }
    ElkNode {
        id: "root".to_string(),
        layout_options: BTreeMap::from([("elk.algorithm".to_string(), "layered".to_string())]),
        children,
        edges,
        ..ElkNode::default()
    }
}

// Mutation: put the recursive `place_block` of commit `7bb277d` back into `bk_compactor.rs`, so
// it calls itself once per block of the staircase.
#[test]
fn a_staircase_of_blocks_is_placed_on_a_small_stack() {
    let graph = staircase();
    let laid_out = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || layout(&graph).map(|out| out.children.len()))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(laid_out, Ok(2 * STEPS - 1));
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
