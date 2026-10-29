//! Port of ELK's `BKAligner`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/BKAligner.java

use std::collections::HashSet;

use crate::graph::{EdgeId, LGraph, LNodeKind, NodeId};
use crate::phase::placement::bk::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use crate::phase::placement::bk::bk_node_placer::{blocks, get_edge};
use crate::phase::placement::bk::neighborhood_information::NeighborhoodInformation;

pub(crate) fn vertical_alignment(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &mut BKAlignedLayout,
    marked_edges: &HashSet<EdgeId>,
) {
    for node in ni.iter_nodes() {
        let id = ni.node_id[node];
        layout.root[id] = id;
        layout.align[id] = id;
        layout.inner_shift[id] = 0.0;
        layout.only_dummies[id] = true;
    }

    let layer_iter: Box<dyn Iterator<Item = usize>> = match layout.hdir {
        HDirection::Left => Box::new((0..graph.layers().len()).rev()),
        HDirection::Right => Box::new(0..graph.layers().len()),
    };

    for layer_index in layer_iter {
        let layer = &graph.layers()[layer_index];
        let mut r = match layout.vdir {
            VDirection::Up => isize::MAX,
            VDirection::Down => -1,
        };

        let node_iter: Box<dyn Iterator<Item = NodeId>> = match layout.vdir {
            VDirection::Up => Box::new(layer.nodes().iter().copied().rev()),
            VDirection::Down => Box::new(layer.nodes().iter().copied()),
        };

        for v_i_k in node_iter {
            let v_id = ni.node_id[v_i_k];
            let neighbors = match layout.hdir {
                HDirection::Left => &ni.right_neighbors[v_id],
                HDirection::Right => &ni.left_neighbors[v_id],
            };
            if neighbors.is_empty() {
                continue;
            }

            let d = neighbors.len();
            let low = ((d as f64 + 1.0) / 2.0).floor() as usize - 1;
            let high = ((d as f64 + 1.0) / 2.0).ceil() as usize - 1;

            if layout.vdir == VDirection::Up {
                for m in (low..=high).rev() {
                    if layout.align[v_id] == v_id {
                        let u_m = &neighbors[m];
                        let u_id = ni.node_id[u_m.node];
                        if !marked_edges.contains(&u_m.edge) && r > ni.node_index[u_id] as isize {
                            layout.align[u_id] = v_id;
                            layout.root[v_id] = layout.root[u_id];
                            layout.align[v_id] = layout.root[v_id];
                            let root = layout.root[v_id];
                            layout.only_dummies[root] &=
                                graph.layerless_nodes[v_i_k].kind() == LNodeKind::LongEdge;
                            r = ni.node_index[u_id] as isize;
                        }
                    }
                }
            } else {
                for u_m in neighbors.iter().take(high + 1).skip(low) {
                    if layout.align[v_id] == v_id {
                        let u_id = ni.node_id[u_m.node];
                        if !marked_edges.contains(&u_m.edge) && r < ni.node_index[u_id] as isize {
                            layout.align[u_id] = v_id;
                            layout.root[v_id] = layout.root[u_id];
                            layout.align[v_id] = layout.root[v_id];
                            let root = layout.root[v_id];
                            layout.only_dummies[root] &=
                                graph.layerless_nodes[v_i_k].kind() == LNodeKind::LongEdge;
                            r = ni.node_index[u_id] as isize;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn inside_block_shift(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &mut BKAlignedLayout,
) {
    let blocks = blocks(layout);
    for root in blocks.keys().copied() {
        let root_node = ni.id_to_node[root];
        let mut space_above = graph.layerless_nodes[root_node].margin.top;
        let mut space_below = graph.layerless_nodes[root_node].size.height
            + graph.layerless_nodes[root_node].margin.bottom;
        layout.inner_shift[root] = 0.0;

        let mut current = root;
        while layout.align[current] != root {
            let next = layout.align[current];
            let current_node = ni.id_to_node[current];
            let next_node = ni.id_to_node[next];
            let port_pos_diff = get_edge(graph, current_node, next_node)
                .map(|edge| edge_port_position_difference(graph, edge, layout.hdir))
                .unwrap_or(0.0);

            let next_inner_shift = layout.inner_shift[current] + port_pos_diff;
            layout.inner_shift[next] = next_inner_shift;

            let lnode = &graph.layerless_nodes[next_node];
            space_above = space_above.max(lnode.margin.top - next_inner_shift);
            space_below =
                space_below.max(next_inner_shift + lnode.size.height + lnode.margin.bottom);
            current = next;
        }

        let mut current = root;
        loop {
            layout.inner_shift[current] += space_above;
            current = layout.align[current];
            if current == root {
                break;
            }
        }

        layout.block_size[root] = space_above + space_below;
    }
}

// Java: BKAligner.insideBlockShift. Java adds and subtracts left to right, as
// `t.pos + t.anchor - s.pos - s.anchor`. Subtracting the sum `s.pos + s.anchor` instead can
// round differently in the last bit, so the four terms stay separate here.
fn edge_port_position_difference(graph: &LGraph, edge: EdgeId, hdir: HDirection) -> f64 {
    let source = &graph.layerless_nodes[graph.edges[edge].source().node].ports
        [graph.edges[edge].source().port];
    let target = &graph.layerless_nodes[graph.edges[edge].target().node].ports
        [graph.edges[edge].target().port];
    match hdir {
        HDirection::Left => {
            target.position.y + target.anchor.y - source.position.y - source.anchor.y
        }
        HDirection::Right => {
            source.position.y + source.anchor.y - target.position.y - target.anchor.y
        }
    }
}
