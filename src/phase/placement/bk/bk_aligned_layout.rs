//! Port of ELK's `BKAlignedLayout`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/BKAlignedLayout.java

use crate::graph::{LGraph, NodeId, PortRef};
use crate::phase::placement::bk::neighborhood_information::NeighborhoodInformation;
use crate::phase::placement::vertical_spacing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VDirection {
    Down,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HDirection {
    Right,
    Left,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BKAlignedLayout {
    pub(crate) root: Vec<usize>,
    pub(crate) block_size: Vec<f64>,
    pub(crate) align: Vec<usize>,
    pub(crate) inner_shift: Vec<f64>,
    pub(crate) sink: Vec<usize>,
    pub(crate) shift: Vec<f64>,
    pub(crate) y: Vec<Option<f64>>,
    pub(crate) vdir: VDirection,
    pub(crate) hdir: HDirection,
    pub(crate) straightened: Vec<bool>,
    pub(crate) only_dummies: Vec<bool>,
}

impl BKAlignedLayout {
    pub(crate) fn new(node_count: usize, vdir: VDirection, hdir: HDirection) -> Self {
        Self {
            root: vec![usize::MAX; node_count],
            block_size: vec![0.0; node_count],
            align: vec![usize::MAX; node_count],
            inner_shift: vec![0.0; node_count],
            sink: vec![usize::MAX; node_count],
            shift: vec![0.0; node_count],
            y: vec![None; node_count],
            vdir,
            hdir,
            straightened: vec![false; node_count],
            only_dummies: vec![true; node_count],
        }
    }

    pub(crate) fn layout_size(&self, graph: &LGraph, ni: &NeighborhoodInformation) -> f64 {
        let mut min = f64::INFINITY;
        let mut max = f64::NEG_INFINITY;

        for node in ni.iter_nodes() {
            let id = ni.node_id[node];
            let root = self.root[id];
            let Some(y_min) = self.y[id] else {
                continue;
            };
            let y_max = y_min + self.block_size[root];
            min = min.min(y_min);
            max = max.max(y_max);
        }

        if min.is_finite() && max.is_finite() {
            max - min
        } else {
            graph
                .layers()
                .iter()
                .flat_map(|layer| layer.nodes().iter().copied())
                .map(|node| graph.layerless_nodes[node].size.height)
                .fold(0.0, f64::max)
        }
    }
}

pub(crate) fn calculate_delta(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
    src: PortRef,
    tgt: PortRef,
) -> f64 {
    let src_id = ni.node_id[src.node];
    let tgt_id = ni.node_id[tgt.node];
    let src_root = layout.root[src_id];
    let tgt_root = layout.root[tgt_id];
    let src_pos = layout.y[src_root].unwrap_or(0.0)
        + layout.inner_shift[src_id]
        + port_position_y(graph, src)
        + port_anchor_y(graph, src);
    let tgt_pos = layout.y[tgt_root].unwrap_or(0.0)
        + layout.inner_shift[tgt_id]
        + port_position_y(graph, tgt)
        + port_anchor_y(graph, tgt);
    tgt_pos - src_pos
}

pub(crate) fn shift_block(
    ni: &NeighborhoodInformation,
    layout: &mut BKAlignedLayout,
    root_node: NodeId,
    delta: f64,
) {
    let root = ni.node_id[root_node];
    let mut current = root;
    loop {
        let new_pos = layout.y[current].unwrap_or(0.0) + delta;
        layout.y[current] = Some(new_pos);
        current = layout.align[current];
        if current == root {
            break;
        }
    }
}

pub(crate) fn check_space_above(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
    block_root: NodeId,
    delta: f64,
) -> f64 {
    let root = ni.node_id[block_root];
    let mut available_space = delta;
    let mut current = root;
    loop {
        current = layout.align[current];
        let current_node = ni.id_to_node[current];
        let min_y_current = min_y(graph, ni, layout, current_node);

        if let Some(neighbor) = upper_neighbor(graph, ni, current_node) {
            let max_y_neighbor = max_y(graph, ni, layout, neighbor);
            available_space = available_space.min(
                min_y_current - (max_y_neighbor + vertical_spacing(graph, current_node, neighbor)),
            );
        }

        if current == root {
            break;
        }
    }
    available_space
}

pub(crate) fn check_space_below(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
    block_root: NodeId,
    delta: f64,
) -> f64 {
    let root = ni.node_id[block_root];
    let mut available_space = delta;
    let mut current = root;
    loop {
        current = layout.align[current];
        let current_node = ni.id_to_node[current];
        let max_y_current = max_y(graph, ni, layout, current_node);

        if let Some(neighbor) = lower_neighbor(graph, ni, current_node) {
            let min_y_neighbor = min_y(graph, ni, layout, neighbor);
            available_space = available_space.min(
                min_y_neighbor - (max_y_current + vertical_spacing(graph, current_node, neighbor)),
            );
        }

        if current == root {
            break;
        }
    }
    available_space
}

fn min_y(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
    node: NodeId,
) -> f64 {
    let node_id = ni.node_id[node];
    let root = layout.root[node_id];
    layout.y[root].unwrap_or(0.0) + layout.inner_shift[node_id]
        - graph.layerless_nodes[node].margin.top
}

fn max_y(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
    node: NodeId,
) -> f64 {
    let node_id = ni.node_id[node];
    let root = layout.root[node_id];
    layout.y[root].unwrap_or(0.0)
        + layout.inner_shift[node_id]
        + graph.layerless_nodes[node].size.height
        + graph.layerless_nodes[node].margin.bottom
}

fn lower_neighbor(graph: &LGraph, ni: &NeighborhoodInformation, node: NodeId) -> Option<NodeId> {
    let layer = graph.layerless_nodes[node].layer_index()?;
    let index = ni.node_index[ni.node_id[node]];
    graph.layers()[layer].nodes().get(index + 1).copied()
}

pub(crate) fn upper_neighbor(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    node: NodeId,
) -> Option<NodeId> {
    let layer = graph.layerless_nodes[node].layer_index()?;
    let index = ni.node_index[ni.node_id[node]];
    if index > 0 {
        graph.layers()[layer].nodes().get(index - 1).copied()
    } else {
        None
    }
}

// Java adds `getPosition().y` and `getAnchor().y` as two separate terms of a longer sum, so they
// stay two functions. Adding them first changes the rounding in the last bit.
pub(crate) fn port_position_y(graph: &LGraph, port_ref: PortRef) -> f64 {
    graph.layerless_nodes[port_ref.node].ports[port_ref.port]
        .position
        .y
}

pub(crate) fn port_anchor_y(graph: &LGraph, port_ref: PortRef) -> f64 {
    graph.layerless_nodes[port_ref.node].ports[port_ref.port]
        .anchor
        .y
}
