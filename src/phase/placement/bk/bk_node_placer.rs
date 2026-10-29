//! Port of ELK's `BKNodePlacer`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p4nodes/bk/BKNodePlacer.java

use std::collections::{HashMap, HashSet};

use crate::graph::{EdgeId, LGraph, LNodeKind, LayeredEdge, NodeId};
use crate::options::FixedAlignment;
use crate::phase::placement::bk::bk_aligned_layout::{BKAlignedLayout, HDirection, VDirection};
use crate::phase::placement::bk::bk_aligner::{inside_block_shift, vertical_alignment};
use crate::phase::placement::bk::bk_compactor::horizontal_compaction;
use crate::phase::placement::bk::neighborhood_information::NeighborhoodInformation;

const MIN_LAYERS_FOR_CONFLICTS: usize = 3;

pub fn place_nodes_brandes_koepf(graph: &mut LGraph) {
    if graph.layers().is_empty() {
        return;
    }

    let ni = NeighborhoodInformation::build_for(graph);
    if ni.node_count == 0 {
        return;
    }

    let marked_edges = mark_conflicts(graph, &ni);
    let mut layouts = layouts_for_fixed_alignment(graph, &ni);

    for layout in &mut layouts {
        vertical_alignment(graph, &ni, layout, &marked_edges);
        inside_block_shift(graph, &ni, layout);
    }

    for layout in &mut layouts {
        horizontal_compaction(graph, &ni, layout);
    }

    let chosen = choose_layout(graph, &ni, layouts);
    apply_layout(graph, &ni, &chosen);
}

fn layouts_for_fixed_alignment(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
) -> Vec<BKAlignedLayout> {
    match graph.options.node_placement_bk_fixed_alignment {
        FixedAlignment::LeftDown => vec![BKAlignedLayout::new(
            ni.node_count,
            VDirection::Down,
            HDirection::Left,
        )],
        FixedAlignment::LeftUp => vec![BKAlignedLayout::new(
            ni.node_count,
            VDirection::Up,
            HDirection::Left,
        )],
        FixedAlignment::RightDown => vec![BKAlignedLayout::new(
            ni.node_count,
            VDirection::Down,
            HDirection::Right,
        )],
        FixedAlignment::RightUp => vec![BKAlignedLayout::new(
            ni.node_count,
            VDirection::Up,
            HDirection::Right,
        )],
        FixedAlignment::None | FixedAlignment::Balanced => vec![
            BKAlignedLayout::new(ni.node_count, VDirection::Down, HDirection::Right),
            BKAlignedLayout::new(ni.node_count, VDirection::Up, HDirection::Right),
            BKAlignedLayout::new(ni.node_count, VDirection::Down, HDirection::Left),
            BKAlignedLayout::new(ni.node_count, VDirection::Up, HDirection::Left),
        ],
    }
}

fn mark_conflicts(graph: &LGraph, ni: &NeighborhoodInformation) -> HashSet<EdgeId> {
    let number_of_layers = graph.layers().len();
    let mut marked_edges = HashSet::new();
    if number_of_layers < MIN_LAYERS_FOR_CONFLICTS {
        return marked_edges;
    }

    let layer_sizes = graph
        .layers()
        .iter()
        .map(|layer| layer.nodes().len())
        .collect::<Vec<_>>();

    for i in 1..number_of_layers - 1 {
        let current_layer = &graph.layers()[i + 1];
        let mut k_0 = 0usize;
        let mut l = 0usize;

        for l_1 in 0..layer_sizes[i + 1] {
            let v_l_i = current_layer.nodes()[l_1];
            if l_1 == layer_sizes[i + 1] - 1
                || incident_to_inner_segment(graph, ni, v_l_i, i + 1, i)
            {
                let mut k_1 = layer_sizes[i].saturating_sub(1);
                if incident_to_inner_segment(graph, ni, v_l_i, i + 1, i)
                    && let Some(left) = ni.left_neighbors[ni.node_id[v_l_i]].first()
                {
                    k_1 = ni.node_index[ni.node_id[left.node]];
                }

                while l <= l_1 {
                    let v_l = current_layer.nodes()[l];
                    if !incident_to_inner_segment(graph, ni, v_l, i + 1, i) {
                        for upper_neighbor in &ni.left_neighbors[ni.node_id[v_l]] {
                            let k = ni.node_index[ni.node_id[upper_neighbor.node]];
                            if k < k_0 || k > k_1 {
                                marked_edges.insert(upper_neighbor.edge);
                            }
                        }
                    }
                    l += 1;
                }

                k_0 = k_1;
            }
        }
    }

    marked_edges
}

fn incident_to_inner_segment(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    node: NodeId,
    layer1: usize,
    layer2: usize,
) -> bool {
    if graph.layerless_nodes[node].kind() != LNodeKind::LongEdge {
        return false;
    }

    for edge in graph.node_incoming_edges(node) {
        let source = graph.edges[edge].source().node;
        if graph.layerless_nodes[source].kind() == LNodeKind::LongEdge
            && graph.layerless_nodes[source].layer_index() == Some(layer2)
            && graph.layerless_nodes[node].layer_index() == Some(layer1)
            && ni.layer_index.get(layer2).copied() == Some(layer2)
        {
            return true;
        }
    }

    false
}

fn choose_layout(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layouts: Vec<BKAlignedLayout>,
) -> BKAlignedLayout {
    let fallback_layout = layouts.first().cloned();
    let favor_straight_edges = graph
        .options
        .node_placement_favor_straight_edges
        .unwrap_or(graph.options.edge_routing == crate::options::EdgeRouting::Orthogonal);
    let produce_balanced_layout = matches!(
        graph.options.node_placement_bk_fixed_alignment,
        FixedAlignment::Balanced
    ) || (graph.options.node_placement_bk_fixed_alignment
        == FixedAlignment::None
        && !favor_straight_edges);

    if produce_balanced_layout
        && layouts.len() >= 4
        && let Some(balanced) = create_balanced_layout(graph, ni, &layouts)
        && check_order_constraint(graph, ni, &balanced)
    {
        return balanced;
    }

    let mut chosen = None;
    for layout in layouts {
        if !check_order_constraint(graph, ni, &layout) {
            continue;
        }
        if chosen
            .as_ref()
            .map(|current: &BKAlignedLayout| {
                current.layout_size(graph, ni) > layout.layout_size(graph, ni)
            })
            .unwrap_or(true)
        {
            chosen = Some(layout);
        }
    }

    chosen.unwrap_or_else(|| {
        fallback_layout.unwrap_or_else(|| {
            BKAlignedLayout::new(ni.node_count, VDirection::Down, HDirection::Right)
        })
    })
}

fn create_balanced_layout(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layouts: &[BKAlignedLayout],
) -> Option<BKAlignedLayout> {
    if layouts.len() < 4 {
        return None;
    }

    let mut balanced = BKAlignedLayout::new(ni.node_count, VDirection::Down, HDirection::Right);
    let no_of_layouts = layouts.len();
    let mut min = vec![f64::INFINITY; no_of_layouts];
    let mut max = vec![f64::NEG_INFINITY; no_of_layouts];
    let mut width = vec![0.0; no_of_layouts];
    let mut min_width_layout = 0usize;

    for (index, layout) in layouts.iter().enumerate() {
        width[index] = layout.layout_size(graph, ni);
        if width[min_width_layout] > width[index] {
            min_width_layout = index;
        }
        for node in ni.iter_nodes() {
            let id = ni.node_id[node];
            let node_pos_y = layout.y[id]? + layout.inner_shift[id];
            min[index] = min[index].min(node_pos_y);
            max[index] = max[index].max(node_pos_y + graph.layerless_nodes[node].size.height);
        }
    }

    let mut shift = vec![0.0; no_of_layouts];
    for (index, layout) in layouts.iter().enumerate() {
        shift[index] = match layout.vdir {
            VDirection::Down => min[min_width_layout] - min[index],
            VDirection::Up => max[min_width_layout] - max[index],
        };
    }

    for node in ni.iter_nodes() {
        let id = ni.node_id[node];
        let mut calculated_y = Vec::with_capacity(no_of_layouts);
        for (index, layout) in layouts.iter().enumerate() {
            calculated_y.push(layout.y[id]? + layout.inner_shift[id] + shift[index]);
        }
        calculated_y.sort_by(f64::total_cmp);
        balanced.y[id] = Some((calculated_y[1] + calculated_y[2]) / 2.0);
        balanced.inner_shift[id] = 0.0;
        balanced.root[id] = id;
        balanced.block_size[id] = graph.layerless_nodes[node].size.height
            + graph.layerless_nodes[node].margin.top
            + graph.layerless_nodes[node].margin.bottom;
    }

    Some(balanced)
}

fn check_order_constraint(
    graph: &LGraph,
    ni: &NeighborhoodInformation,
    layout: &BKAlignedLayout,
) -> bool {
    for layer in graph.layers() {
        let mut pos = f64::NEG_INFINITY;
        for node in layer.nodes() {
            let id = ni.node_id[*node];
            let Some(y) = layout.y[id] else {
                return false;
            };
            let top = y + layout.inner_shift[id] - graph.layerless_nodes[*node].margin.top;
            let bottom = y
                + layout.inner_shift[id]
                + graph.layerless_nodes[*node].size.height
                + graph.layerless_nodes[*node].margin.bottom;
            if top > pos && bottom > pos {
                pos = bottom;
            } else {
                return false;
            }
        }
    }
    true
}

fn apply_layout(graph: &mut LGraph, ni: &NeighborhoodInformation, layout: &BKAlignedLayout) {
    for node in ni.iter_nodes() {
        let id = ni.node_id[node];
        if let Some(y) = layout.y[id] {
            graph.layerless_nodes[node].position.y = y + layout.inner_shift[id];
        }
    }
}

pub(crate) fn blocks(layout: &BKAlignedLayout) -> HashMap<usize, Vec<usize>> {
    let mut blocks: HashMap<usize, Vec<usize>> = HashMap::new();
    for node in 0..layout.root.len() {
        let root = layout.root[node];
        if root != usize::MAX {
            blocks.entry(root).or_default().push(node);
        }
    }
    blocks
}

pub(crate) fn get_edge(graph: &LGraph, source: NodeId, target: NodeId) -> Option<EdgeId> {
    graph
        .node_connected_edges(source)
        .into_iter()
        .find(|edge| edge_opposite_node(&graph.edges[*edge], source) == Some(target))
}

pub(crate) fn edge_opposite_node(edge: &LayeredEdge, node: NodeId) -> Option<NodeId> {
    if edge.source().node == node {
        Some(edge.target().node)
    } else if edge.target().node == node {
        Some(edge.source().node)
    } else {
        None
    }
}
