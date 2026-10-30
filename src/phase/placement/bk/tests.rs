//! Tests of the classes in this module.

use crate::graph::{LGraph, LNode};
use crate::importer::{ElkInputEdge, ElkInputGraph, ElkInputNode, import_graph};
use crate::options::EdgeStraighteningStrategy;
use crate::options::{ElkDirection, GreedySwitchType, LayeredOptions};
use crate::phase::placement::bk::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use crate::phase::placement::bk::bk_compactor::horizontal_compaction;
use crate::phase::placement::bk::neighborhood_information::NeighborhoodInformation;
use crate::pipeline::{LayeredPhase, run_processors_until_phase};

fn node(id: &str) -> ElkInputNode {
    ElkInputNode {
        ports: Vec::new(),
        parent_settings: Default::default(),
        id: id.to_string(),
        width: 80.0,
        height: 40.0,
        parent: None,
        direction: None,
        hierarchy_handling: None,
        layer_constraint: None,
        port_alignment: None,
        port_constraints: None,
        node_label_placement: crate::options::NodeLabelPlacement::FIXED,
        node_flexibility: crate::options::NodeFlexibility::None,
        ports_surrounding: None,
        nested_options: None,
        compound_node: false,
        labels: Vec::new(),
        node_size: Default::default(),
        self_loop_distribution: Default::default(),
        self_loop_ordering: Default::default(),
        port_label_placement: Default::default(),
        port_labels_treat_as_group: true,
    }
}

fn edge(id: &str, source: &str, target: &str) -> ElkInputEdge {
    ElkInputEdge {
        source_port: None,
        target_port: None,
        id: id.to_string(),
        source: source.to_string(),
        target: target.to_string(),
        labels: Vec::new(),
        minlen: 1,
        inside_self_loop: false,
        model_order: None,
        priority_direction: 0,
        priority_shortness: 0,
        priority_straightness: 0,
        container: None,
    }
}

#[test]
fn bk_node_placer_assigns_order_preserving_y_coordinates() {
    let mut graph = import_graph(&ElkInputGraph {
        id: "root".to_string(),
        options: LayeredOptions {
            direction: ElkDirection::Right,
            greedy_switch_type: GreedySwitchType::Off,
            node_placement_bk_edge_straightening: EdgeStraighteningStrategy::None,
            ..LayeredOptions::default()
        },
        nodes: vec![node("A"), node("B"), node("C")],
        edges: vec![edge("A-C", "A", "C"), edge("B-C", "B", "C")],
    })
    .unwrap();

    run_processors_until_phase(&mut graph, LayeredPhase::P4NodePlacement).unwrap();

    // Every layer must keep its nodes apart in y, margins included.
    assert!(!graph.layers().is_empty());
    for layer in graph.layers() {
        let mut bottom = f64::NEG_INFINITY;
        for node in layer.nodes() {
            let lnode = &graph.layerless_nodes[*node];
            assert!(lnode.position.y - lnode.margin.top > bottom);
            bottom = lnode.position.y + lnode.size.height + lnode.margin.bottom;
        }
    }
}

/// Blocks in the chain of `placing_a_deep_chain_of_blocks_needs_no_deep_stack`.
const CHAIN: usize = 4_000;

/// Far less than the 2 MiB a Rust thread gets by default. A `place_block` that recurses into the
/// block of each neighbour needs more than this for `CHAIN` blocks.
const STACK: usize = 256 * 1024;

/// Layer `i` holds `top[i]` and `bottom[i]`. Block `i` is `top[i]` and `bottom[i + 1]`, with root
/// `top[i]`. The neighbour above `bottom[i + 1]` is `top[i + 1]`, the root of block `i + 1`, a
/// layer later. So placing block `i` first places block `i + 1`, and that block `i + 2`, down to
/// the last one. `bottom[0]` is a block of its own.
fn chain_of_blocks() -> (LGraph, BKAlignedLayout, NeighborhoodInformation) {
    let mut graph = LGraph::new(
        "root",
        LayeredOptions {
            node_placement_bk_edge_straightening: EdgeStraighteningStrategy::None,
            ..LayeredOptions::default()
        },
    );
    for layer in 0..CHAIN {
        let top = graph.push_node_with(|_| LNode::new(format!("t{layer}"), 20.0, 20.0, None));
        let bottom = graph.push_node_with(|_| LNode::new(format!("b{layer}"), 20.0, 20.0, None));
        graph.set_layer_nodes(layer, vec![top, bottom]);
    }
    let ni = NeighborhoodInformation::build_for(&graph);
    // The ids run layer by layer, and a layer holds its top node, then its bottom node.
    let top = |layer: usize| 2 * layer;
    let bottom = |layer: usize| 2 * layer + 1;
    let mut layout = BKAlignedLayout::new(2 * CHAIN, VDirection::Down, HDirection::Right);
    for id in 0..2 * CHAIN {
        layout.root[id] = id;
        layout.align[id] = id;
    }
    for layer in 0..CHAIN - 1 {
        layout.root[bottom(layer + 1)] = top(layer);
        layout.align[top(layer)] = bottom(layer + 1);
        layout.align[bottom(layer + 1)] = top(layer);
    }
    (graph, layout, ni)
}

// Mutation: in `place_block` (src/phase/placement/bk/bk_compactor.rs), put the recursive version
// of commit `6554427^` back, which calls itself for the block of each neighbour. The test then
// overflows its 256 KiB stack and aborts the test run.
// Mutation: in `place_block`, leave `neighbor.size.height` out of `new_position` in the `Down`
// branch. Each block then needs 20 less than it should, so every y differs.
#[test]
fn placing_a_deep_chain_of_blocks_needs_no_deep_stack() {
    let (tops, bottoms) = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(|| {
            let (graph, mut layout, ni) = chain_of_blocks();
            horizontal_compaction(&graph, &ni, &mut layout);
            let y = |id: usize| layout.y[id].unwrap();
            let tops = (0..CHAIN).map(|layer| y(2 * layer)).collect::<Vec<_>>();
            let bottoms = (0..CHAIN).map(|layer| y(2 * layer + 1)).collect::<Vec<_>>();
            (tops, bottoms)
        })
        .unwrap()
        .join()
        .unwrap();
    // The last block rests at 0. Each block above it needs 40 more: a node of 20 and a gap of 20.
    // A bottom node lies in the block of the top node one layer before it, and below the top node
    // of its own layer.
    for layer in 0..CHAIN {
        let steps = (CHAIN - 1 - layer) as f64;
        assert_eq!(tops[layer], 40.0 * steps, "top node of layer {layer}");
        assert_eq!(
            bottoms[layer],
            40.0 * (steps + 1.0),
            "bottom node of layer {layer}"
        );
    }
}
