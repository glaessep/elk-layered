//! Port of ELK's `BKCompactor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/BKCompactor.java

use std::collections::{BTreeMap, VecDeque};

use crate::graph::{LGraph, NodeId};
use crate::phase::placement::bk::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use crate::phase::placement::bk::neighborhood_information::NeighborhoodInformation;
use crate::phase::placement::bk::threshold_strategy::ThresholdStrategy;
use crate::phase::placement::vertical_spacing;

pub(crate) fn horizontal_compaction(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &mut BKAlignedLayout,
) {
    for node in ni.iter_nodes() {
        let id = ni.node_id[node];
        layout.sink[id] = id;
        layout.shift[id] = match layout.vdir {
            VDirection::Up => f64::NEG_INFINITY,
            VDirection::Down => f64::INFINITY,
        };
        layout.y[id] = None;
    }

    let mut class_graph = ClassGraph::default();
    let layers = layer_indices(graph, layout.hdir);
    let mut threshold = ThresholdStrategy::for_graph(graph);

    for layer_index in layers {
        let nodes = node_order_for_direction(graph.layers()[layer_index].nodes(), layout.vdir);
        for v in nodes {
            let id = ni.node_id[v];
            if layout.root[id] == id {
                place_block(graph, ni, layout, &mut class_graph, &mut threshold, id);
            }
        }
    }

    place_classes(layout, class_graph);

    let layers = layer_indices(graph, layout.hdir);
    for layer_index in layers {
        for v in graph.layers()[layer_index].nodes() {
            let id = ni.node_id[*v];
            let root = layout.root[id];
            layout.y[id] = layout.y[root];
            if id == root {
                let sink_shift = layout.shift[layout.sink[id]];
                let apply_shift = match layout.vdir {
                    VDirection::Up => sink_shift > f64::NEG_INFINITY,
                    VDirection::Down => sink_shift < f64::INFINITY,
                };
                if apply_shift {
                    let y = layout.y[id].unwrap_or(0.0) + sink_shift;
                    layout.y[id] = Some(y);
                }
            }
        }
    }

    threshold.post_process(graph, ni, layout);
}

/// One block that `place_block` is placing: Java's local variables of one recursive call.
struct PlaceBlockFrame {
    root: usize,
    current: usize,
    initial_assignment: bool,
    thresh: f64,
}

impl PlaceBlockFrame {
    fn start(layout: &mut BKAlignedLayout, root: usize) -> Self {
        layout.y[root] = Some(0.0);
        Self {
            root,
            current: root,
            initial_assignment: true,
            thresh: match layout.vdir {
                VDirection::Down => f64::NEG_INFINITY,
                VDirection::Up => f64::INFINITY,
            },
        }
    }
}

// Java: `BKCompactor.placeBlock`, which recurses into the block of each neighbour. The explicit
// stack places the blocks in the same order, and a wide layer cannot overflow it.
fn place_block(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &mut BKAlignedLayout,
    class_graph: &mut ClassGraph,
    threshold: &mut ThresholdStrategy,
    root: usize,
) {
    if layout.y[root].is_some() {
        return;
    }

    let mut stack = vec![PlaceBlockFrame::start(layout, root)];
    'frames: while let Some(mut frame) = stack.pop() {
        loop {
            let current_node = ni.id_to_node[frame.current];
            let current_index = ni.node_index[frame.current];
            let current_layer_size = graph.layers()
                [graph.layerless_nodes[current_node].layer_index().unwrap()]
            .nodes()
            .len();

            let needs_neighbor = match layout.vdir {
                VDirection::Down => current_index > 0,
                VDirection::Up => current_index < current_layer_size - 1,
            };

            if needs_neighbor {
                let layer =
                    &graph.layers()[graph.layerless_nodes[current_node].layer_index().unwrap()];
                let neighbor_node = match layout.vdir {
                    VDirection::Up => layer.nodes()[current_index + 1],
                    VDirection::Down => layer.nodes()[current_index - 1],
                };
                let neighbor_root = layout.root[ni.node_id[neighbor_node]];
                if layout.y[neighbor_root].is_none() {
                    // Java recurses into the neighbour's block here. Place it first, then come back
                    // and run this step again; the block is placed then, so the step goes on.
                    stack.push(frame);
                    stack.push(PlaceBlockFrame::start(layout, neighbor_root));
                    continue 'frames;
                }

                frame.thresh = threshold.calculate_threshold(
                    graph,
                    ni,
                    layout,
                    frame.thresh,
                    frame.root,
                    frame.current,
                );

                if layout.sink[frame.root] == frame.root {
                    layout.sink[frame.root] = layout.sink[neighbor_root];
                }

                if layout.sink[frame.root] == layout.sink[neighbor_root] {
                    let spacing = vertical_spacing(graph, current_node, neighbor_node);
                    if layout.vdir == VDirection::Up {
                        let current_block_position = layout.y[frame.root].unwrap_or(0.0);
                        let neighbor = &graph.layerless_nodes[neighbor_node];
                        let current_lnode = &graph.layerless_nodes[current_node];
                        let new_position = layout.y[neighbor_root].unwrap_or(0.0)
                            + layout.inner_shift[ni.node_id[neighbor_node]]
                            - neighbor.margin.top
                            - spacing
                            - current_lnode.margin.bottom
                            - current_lnode.size.height
                            - layout.inner_shift[frame.current];
                        let candidate = new_position.min(frame.thresh);
                        layout.y[frame.root] = Some(if frame.initial_assignment {
                            frame.initial_assignment = false;
                            candidate
                        } else {
                            current_block_position.min(candidate)
                        });
                    } else {
                        let current_block_position = layout.y[frame.root].unwrap_or(0.0);
                        let neighbor = &graph.layerless_nodes[neighbor_node];
                        let current_lnode = &graph.layerless_nodes[current_node];
                        let new_position = layout.y[neighbor_root].unwrap_or(0.0)
                            + layout.inner_shift[ni.node_id[neighbor_node]]
                            + neighbor.size.height
                            + neighbor.margin.bottom
                            + spacing
                            + current_lnode.margin.top
                            - layout.inner_shift[frame.current];
                        let candidate = new_position.max(frame.thresh);
                        layout.y[frame.root] = Some(if frame.initial_assignment {
                            frame.initial_assignment = false;
                            candidate
                        } else {
                            current_block_position.max(candidate)
                        });
                    }
                } else {
                    let spacing = graph.options.spacing.node_node;
                    let current_lnode = &graph.layerless_nodes[current_node];
                    let neighbor = &graph.layerless_nodes[neighbor_node];
                    let separation = if layout.vdir == VDirection::Up {
                        layout.y[frame.root].unwrap_or(0.0)
                            + layout.inner_shift[frame.current]
                            + current_lnode.size.height
                            + current_lnode.margin.bottom
                            + spacing
                            - (layout.y[neighbor_root].unwrap_or(0.0)
                                + layout.inner_shift[ni.node_id[neighbor_node]]
                                - neighbor.margin.top)
                    } else {
                        layout.y[frame.root].unwrap_or(0.0) + layout.inner_shift[frame.current]
                            - current_lnode.margin.top
                            - layout.y[neighbor_root].unwrap_or(0.0)
                            - layout.inner_shift[ni.node_id[neighbor_node]]
                            - neighbor.size.height
                            - neighbor.margin.bottom
                            - spacing
                    };
                    class_graph.add_edge(
                        layout.sink[frame.root],
                        layout.sink[neighbor_root],
                        separation,
                    );
                }
            } else {
                frame.thresh = threshold.calculate_threshold(
                    graph,
                    ni,
                    layout,
                    frame.thresh,
                    frame.root,
                    frame.current,
                );
            }

            frame.current = layout.align[frame.current];
            if frame.current == frame.root {
                threshold.finish_block(frame.root);
                break;
            }
        }
    }
}

#[derive(Debug, Default)]
struct ClassGraph {
    nodes: BTreeMap<usize, ClassNode>,
}

impl ClassGraph {
    fn add_edge(&mut self, source: usize, target: usize, separation: f64) {
        self.nodes.entry(source).or_insert_with(|| ClassNode {
            node: source,
            ..ClassNode::default()
        });
        self.nodes.entry(target).or_insert_with(|| ClassNode {
            node: target,
            ..ClassNode::default()
        });
        self.nodes.get_mut(&target).unwrap().indegree += 1;
        self.nodes
            .get_mut(&source)
            .unwrap()
            .outgoing
            .push(ClassEdge { target, separation });
    }
}

#[derive(Debug, Default)]
struct ClassNode {
    node: usize,
    class_shift: Option<f64>,
    outgoing: Vec<ClassEdge>,
    indegree: usize,
}

#[derive(Debug)]
struct ClassEdge {
    target: usize,
    separation: f64,
}

fn place_classes(layout: &mut BKAlignedLayout, mut class_graph: ClassGraph) {
    let mut sinks = class_graph
        .nodes
        .values()
        .filter_map(|node| (node.indegree == 0).then_some(node.node))
        .collect::<VecDeque<_>>();

    while let Some(node_id) = sinks.pop_front() {
        if class_graph.nodes[&node_id].class_shift.is_none() {
            class_graph.nodes.get_mut(&node_id).unwrap().class_shift = Some(0.0);
        }
        let class_shift = class_graph.nodes[&node_id].class_shift.unwrap_or(0.0);
        let outgoing = std::mem::take(&mut class_graph.nodes.get_mut(&node_id).unwrap().outgoing);
        for edge in outgoing {
            let candidate = class_shift + edge.separation;
            let target = class_graph.nodes.get_mut(&edge.target).unwrap();
            target.class_shift = match target.class_shift {
                None => Some(candidate),
                Some(current) if layout.vdir == VDirection::Down => Some(current.min(candidate)),
                Some(current) => Some(current.max(candidate)),
            };
            target.indegree = target.indegree.saturating_sub(1);
            if target.indegree == 0 {
                sinks.push_back(edge.target);
            }
        }
    }

    for node in class_graph.nodes.values() {
        layout.shift[node.node] = node.class_shift.unwrap_or(0.0);
    }
}

fn layer_indices(graph: &LGraph, hdir: HDirection) -> Vec<usize> {
    match hdir {
        HDirection::Left => (0..graph.layers().len()).rev().collect(),
        HDirection::Right => (0..graph.layers().len()).collect(),
    }
}

fn node_order_for_direction(nodes: &[NodeId], vdir: VDirection) -> Vec<NodeId> {
    match vdir {
        VDirection::Up => nodes.iter().copied().rev().collect(),
        VDirection::Down => nodes.to_vec(),
    }
}
