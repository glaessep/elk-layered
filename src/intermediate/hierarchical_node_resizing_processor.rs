//! Port of ELK's `HierarchicalNodeResizingProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalNodeResizingProcessor.java

use crate::graph::{LGraph, LSize, PortSide};
use crate::options::ContentAlignment;
use crate::pipeline::{PipelineResult, actual_graph_size};

/// Ports `HierarchicalNodeResizingProcessor.process`.
pub fn resize_hierarchical_node_graph(graph: &mut LGraph) -> PipelineResult<()> {
    graph.clear_layers();
    resize_graph_to_minimum(graph)
}

/// Ports `ElkLayered.resizeGraph`. `HierarchicalNodeResizingProcessor.resizeGraph` is the same.
pub(crate) fn resize_graph_to_minimum(graph: &mut LGraph) -> PipelineResult<()> {
    graph.options.validate_node_size_minimum()?;
    let old_size = actual_graph_size(graph);
    let mut new_size = old_size;
    if let Some(minimum) = graph.options.effective_node_size_minimum() {
        new_size.width = new_size.width.max(minimum.width);
        new_size.height = new_size.height.max(minimum.height);
    }
    resize_graph_no_really_i_mean_it(graph, old_size, new_size);
    Ok(())
}

/// Ports `ElkLayered.resizeGraphNoReallyIMeanIt`. `HierarchicalNodeResizingProcessor` has the
/// same method.
pub(crate) fn resize_graph_no_really_i_mean_it(
    graph: &mut LGraph,
    old_size: LSize,
    new_size: LSize,
) {
    fn alignment_offset(alignment: ContentAlignment, old: f64, new: f64) -> f64 {
        if new <= old {
            return 0.0;
        }
        match alignment {
            ContentAlignment::Start => 0.0,
            ContentAlignment::Center => (new - old) / 2.0,
            ContentAlignment::End => new - old,
        }
    }
    graph.offset.x += alignment_offset(
        graph.options.horizontal_content_alignment,
        old_size.width,
        new_size.width,
    );
    graph.offset.y += alignment_offset(
        graph.options.vertical_content_alignment,
        old_size.height,
        new_size.height,
    );
    if graph.graph_properties.external_ports
        && (new_size.width > old_size.width || new_size.height > old_size.height)
    {
        for node in &mut graph.layerless_nodes {
            let Some(side) = node.external_port().map(|external| external.side) else {
                continue;
            };
            match side {
                PortSide::East => node.position.x += new_size.width - old_size.width,
                PortSide::South => node.position.y += new_size.height - old_size.height,
                PortSide::North | PortSide::West | PortSide::Undefined => {}
            }
        }
    }

    graph.size.width = new_size.width - graph.padding.left - graph.padding.right;
    graph.size.height = new_size.height - graph.padding.top - graph.padding.bottom;
}
