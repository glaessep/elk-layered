//! Port of ELK's `LayerSizeAndGraphHeightCalculator`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/LayerSizeAndGraphHeightCalculator.java

use crate::graph::{LGraph, LNodeKind, LSize};

pub fn calculate_layer_sizes_and_graph_height(graph: &mut LGraph) {
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut found_nodes = false;
    let ports_surrounding = graph.options.spacing.ports_surrounding;

    let mut sizes = Vec::with_capacity(graph.layers().len());
    for layer in graph.layers() {
        let mut size = LSize::default();

        if layer.nodes().is_empty() {
            sizes.push(size);
            continue;
        }

        found_nodes = true;

        for node_index in layer.nodes() {
            let node = &graph.layerless_nodes[*node_index];
            size.width = size
                .width
                .max(node.size.width + node.margin.left + node.margin.right);
        }

        let first_node = &graph.layerless_nodes[layer.nodes()[0]];
        let mut top = first_node.position.y - first_node.margin.top;
        if first_node.kind() == LNodeKind::ExternalPort {
            top -= ports_surrounding.top;
        }

        let last_node = &graph.layerless_nodes[layer.nodes()[layer.nodes().len() - 1]];
        let mut bottom = last_node.position.y + last_node.size.height + last_node.margin.bottom;
        if last_node.kind() == LNodeKind::ExternalPort {
            bottom += ports_surrounding.bottom;
        }

        size.height = bottom - top;
        sizes.push(size);
        min_y = min_y.min(top);
        max_y = max_y.max(bottom);
    }
    for (layer_index, size) in sizes.into_iter().enumerate() {
        *graph.layer_size_mut(layer_index) = size;
    }

    if !found_nodes {
        min_y = 0.0;
        max_y = 0.0;
    }

    graph.size.height = max_y - min_y;
    graph.offset.y -= min_y;
}
