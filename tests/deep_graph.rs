//! Deep graphs lay out without a stack overflow: a chain 10,000 nodes deep, and a staircase of
//! blocks for the Brandes-Koepf placement.
//!
//! The layout runs on a thread with a small stack, so a step that recurses once per node fails
//! here on every platform. A stack overflow aborts the whole test run; it cannot be caught.

use std::collections::BTreeMap;

use elk_layered::{ElkEdge, ElkNode, Error, layout};

const DEPTH: usize = 10_000;

/// Far less than the 2 MiB a Rust thread gets by default. A step that recurses once per node of
/// the chain, or once per block of the staircase, needs more than this, in a debug and in a
/// release build. Both builds were checked against the recursive `place_block`.
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

/// Lays `graph` out on a thread with the small `STACK`. A stack overflow aborts the whole test
/// run, so a graph that gets this far has not recursed too deep.
fn lay_out_on_small_stack(graph: ElkNode) -> Result<ElkNode, Error> {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || layout(&graph))
        .unwrap()
        .join()
        .unwrap()
}

/// Steps in the staircase. Each step is one block that `place_block` waits on.
const STEPS: usize = 4_000;

/// A staircase of blocks. Layer `i` holds `n{i}` and `m{i}`, and the edges are `m{i} -> m{i+1}`
/// and `n{i} -> m{i+1}`. The last layer holds only `m{STEPS - 1}`. One Brandes-Koepf direction
/// joins `n{i}` and `m{i+1}` into one block, whose root `n{i}` sits one layer earlier than the
/// root of the block beside it. Placing block `i` needs block `i + 1` first, then block `i + 2`,
/// and so on. That needs the same order of `n{i}` and `m{i}` in every layer. The crossing
/// minimiser picks the order, so the test checks it.
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
        // The component packer would put loose nodes in a row whose spacing can match the layer
        // spacing, and hide a lost edge from the shape test.
        layout_options: BTreeMap::from([
            ("elk.algorithm".to_string(), "layered".to_string()),
            (
                "elk.separateConnectedComponents".to_string(),
                "false".to_string(),
            ),
        ]),
        children,
        edges,
        ..ElkNode::default()
    }
}

// Mutation: put back the recursive `place_block` that commit `6554427` replaced in
// `bk_compactor.rs`. It then calls itself once per block of the staircase and overflows the stack.
//
// `placing_a_deep_chain_of_blocks_needs_no_deep_stack` in `src/phase/placement/bk/tests.rs` holds
// `place_block` alone, on blocks set up by hand. This test stays for what that one cannot see:
// the other phases on a graph of 4,000 layers, and the staircase the real pipeline builds from
// it. The shape checks guard the graph, so the overflow check keeps something to guard. Each has
// a mutation:
// - In `place_nodes_brandes_koepf`, reverse the nodes of every odd layer. The order of `n{i}` and
//   `m{i}` then changes from layer to layer.
// - Drop the edges `n{i} -> m{i+1}` from `staircase()`. The `n{i}` then leave their layers.
// - In `apply_layout`, set every `y` to zero. `n{i}` and `m{i}` then share one `y`, and no order
//   is left to check.
#[test]
fn a_staircase_of_blocks_is_placed_on_a_small_stack() {
    let laid_out = lay_out_on_small_stack(staircase()).unwrap();
    let at: BTreeMap<&str, (f64, f64)> = laid_out
        .children
        .iter()
        .map(|node| (node.id.as_str(), (node.x, node.y)))
        .collect();
    let at = |id: &str| at[id];

    // With `elk.direction` RIGHT, a layer sits at one x. `n{i}` shares the layer of `m{i}` and
    // lies before `m{i+1}`, so the block `n{i}, m{i+1}` spans two layers.
    let n_above_m = at("n0").1 < at("m0").1;
    for i in 0..STEPS - 1 {
        let (n, m, next_m) = (format!("n{i}"), format!("m{i}"), format!("m{}", i + 1));
        assert_eq!(at(&n).0, at(&m).0);
        assert!(at(&n).0 < at(&next_m).0);
        assert_ne!(at(&n).1, at(&m).1);
        assert_eq!(at(&n).1 < at(&m).1, n_above_m);
    }
}

// Mutation: let `connected_components_dfs` of the network simplex layerer, `tight_tree_dfs` or
// `postorder_traversal` of the network simplex, or `collect_depth_first_back_edges` recurse again.
#[test]
fn a_graph_10000_nodes_deep_lays_out_on_a_small_stack() {
    let laid_out = lay_out_on_small_stack(closed_chain()).map(|out| out.children.len());
    assert_eq!(laid_out, Ok(DEPTH));
}
