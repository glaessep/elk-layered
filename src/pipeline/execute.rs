//! Running one processor on one graph, and the resizing of the graph around its content.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalNodeResizingProcessor.java

use super::assembler::{
    CrossingMinimizer, CycleBreaker, EdgeRouter, IntermediateProcessor, Layerer, NodePlacer,
    Processor,
};
use super::{PipelineResult, actual_graph_size};
use crate::graph::{LGraph, LNode, LSize, OtherGraphs, PortSide};
use crate::intermediate::{
    calculate_layer_sizes_and_graph_height, insert_label_dummies, join_long_edges,
    merge_hyperedge_dummies, position_interactive_external_ports, postprocess_end_labels,
    postprocess_layer_constraints, preprocess_end_labels, preprocess_layer_constraints,
    process_hierarchical_port_constraints, process_hierarchical_port_dummy_sizes,
    process_hierarchical_port_orthogonal_edges, process_hierarchical_port_positions,
    process_inverted_ports, remove_label_dummies, restore_reversed_edges,
    reverse_edges_for_edge_and_layer_constraints, select_label_sides, sort_end_labels,
    split_long_edges, switch_label_dummies,
};
use crate::north_south::{postprocess_north_south_ports, preprocess_north_south_ports};
use crate::options::{ContentAlignment, LayeringStrategy, PortConstraints};
use crate::p1cycles::{
    break_cycles_depth_first, break_cycles_greedy, break_cycles_greedy_model_order,
    break_cycles_interactive, break_cycles_model_order,
};
use crate::p2layers::{layer_network_simplex, layer_with_strategy};
use crate::p3order::{
    process_port_sides, sort_by_input_model, sort_port_lists,
    sweep::{CrossMinType, minimize_crossings_layer_sweep_with_type},
};
use crate::p4nodes::{
    calculate_innermost_node_margins, calculate_label_and_node_sizes, place_nodes_brandes_koepf,
    place_nodes_network_simplex, place_nodes_simple, process_in_layer_constraints,
};
use crate::p5edges::route_edges_orthogonal;
use crate::selfloops::{
    postprocess_self_loops, preprocess_self_loops, restore_self_loop_ports, route_self_loops,
};
use crate::transform::{GraphTransformMode, transform_graph_direction};

/// Runs `processor` on `graph`. `others` are the graphs inside its nodes.
pub(super) fn execute_processor(
    graph: &mut LGraph,
    others: OtherGraphs<'_>,
    processor: Processor,
) -> PipelineResult<()> {
    match processor {
        Processor::CycleBreaker(breaker) => match breaker {
            CycleBreaker::Greedy => break_cycles_greedy(graph),
            CycleBreaker::DepthFirst => break_cycles_depth_first(graph),
            CycleBreaker::Interactive => break_cycles_interactive(graph),
            CycleBreaker::ModelOrder => break_cycles_model_order(graph),
            CycleBreaker::GreedyModelOrder => break_cycles_greedy_model_order(graph),
        },
        Processor::Layerer(layerer) => match layerer {
            Layerer::NetworkSimplex => layer_network_simplex(graph)?,
            Layerer::LongestPath => layer_with_strategy(graph, LayeringStrategy::LongestPath)?,
            Layerer::LongestPathSource => {
                layer_with_strategy(graph, LayeringStrategy::LongestPathSource)?
            }
            Layerer::CoffmanGraham => layer_with_strategy(graph, LayeringStrategy::CoffmanGraham)?,
            Layerer::Interactive => layer_with_strategy(graph, LayeringStrategy::Interactive)?,
            Layerer::StretchWidth => layer_with_strategy(graph, LayeringStrategy::StretchWidth)?,
            Layerer::MinWidth => layer_with_strategy(graph, LayeringStrategy::MinWidth)?,
        },
        Processor::CrossingMinimizer(CrossingMinimizer::NoMinimizer) => {}
        Processor::CrossingMinimizer(CrossingMinimizer::LayerSweep) => {
            minimize_crossings_layer_sweep_with_type(graph, others, CrossMinType::Barycenter);
        }
        Processor::NodePlacer(placer) => match placer {
            NodePlacer::Simple => place_nodes_simple(graph),
            NodePlacer::BrandesKoepf => place_nodes_brandes_koepf(graph),
            NodePlacer::NetworkSimplex => place_nodes_network_simplex(graph)?,
        },
        Processor::EdgeRouter(EdgeRouter::Orthogonal) => route_edges_orthogonal(graph),
        Processor::Intermediate(intermediate) => execute_intermediate(graph, others, intermediate)?,
    }
    Ok(())
}

fn execute_intermediate(
    graph: &mut LGraph,
    others: OtherGraphs<'_>,
    processor: IntermediateProcessor,
) -> PipelineResult<()> {
    use IntermediateProcessor as P;
    match processor {
        P::DirectionPreprocessor => {
            transform_graph_direction(graph, GraphTransformMode::ToInputDirection);
        }
        P::DirectionPostprocessor => {
            transform_graph_direction(graph, GraphTransformMode::ToInternalLeftToRight);
        }
        P::EdgeAndLayerConstraintEdgeReverser => {
            reverse_edges_for_edge_and_layer_constraints(graph);
        }
        P::InteractiveExternalPortPositioner => position_interactive_external_ports(graph),
        P::SelfLoopPreProcessor => preprocess_self_loops(graph, others),
        P::LayerConstraintPreprocessor => preprocess_layer_constraints(graph)?,
        P::LabelDummyInserter => insert_label_dummies(graph),
        P::LayerConstraintPostprocessor => postprocess_layer_constraints(graph)?,
        P::HierarchicalPortConstraintProcessor => process_hierarchical_port_constraints(graph),
        P::LongEdgeSplitter => split_long_edges(graph),
        P::PortSideProcessor => process_port_sides(graph, others),
        P::InvertedPortProcessor => process_inverted_ports(graph),
        P::PortListSorter => sort_port_lists(graph),
        P::SortByInputModelProcessor => sort_by_input_model(graph),
        P::NorthSouthPortPreprocessor => preprocess_north_south_ports(graph),
        P::NorthSouthPortPostprocessor => postprocess_north_south_ports(graph),
        P::OneSidedGreedySwitch => {
            minimize_crossings_layer_sweep_with_type(
                graph,
                others,
                CrossMinType::OneSidedGreedySwitch,
            );
        }
        P::TwoSidedGreedySwitch => {
            minimize_crossings_layer_sweep_with_type(
                graph,
                others,
                CrossMinType::TwoSidedGreedySwitch,
            );
        }
        P::SelfLoopPortRestorer => restore_self_loop_ports(graph, others),
        P::InLayerConstraintProcessor => process_in_layer_constraints(graph),
        P::LabelAndNodeSizeProcessor => calculate_label_and_node_sizes(graph),
        P::InnermostNodeMarginCalculator => calculate_innermost_node_margins(graph),
        P::SelfLoopRouter => route_self_loops(graph),
        P::LabelDummySwitcher => switch_label_dummies(graph),
        P::LabelSideSelector => select_label_sides(graph),
        P::HyperedgeDummyMerger => merge_hyperedge_dummies(graph),
        P::EndLabelPreprocessor => preprocess_end_labels(graph),
        P::LayerSizeAndGraphHeightCalculator => calculate_layer_sizes_and_graph_height(graph),
        P::HierarchicalPortDummySizeProcessor => process_hierarchical_port_dummy_sizes(graph),
        P::HierarchicalPortPositionProcessor => process_hierarchical_port_positions(graph),
        P::HierarchicalPortOrthogonalEdgeRouter => {
            process_hierarchical_port_orthogonal_edges(graph);
        }
        P::LongEdgeJoiner => join_long_edges(graph),
        P::SelfLoopPostProcessor => postprocess_self_loops(graph),
        P::LabelDummyRemover => remove_label_dummies(graph),
        P::EndLabelSorter => sort_end_labels(graph),
        P::ReversedEdgeRestorer => restore_reversed_edges(graph),
        P::EndLabelPostprocessor => postprocess_end_labels(graph),
        P::HierarchicalNodeResizer => resize_hierarchical_node_graph(graph)?,
    }
    Ok(())
}

fn resize_hierarchical_node_graph(graph: &mut LGraph) -> PipelineResult<()> {
    graph.clear_layers();
    resize_graph_to_minimum(graph)
}

/// Ports `ElkLayered.resizeGraph`.
pub(super) fn resize_graph_to_minimum(graph: &mut LGraph) -> PipelineResult<()> {
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

/// Ports `ElkLayered.resizeGraphNoReallyIMeanIt`.
pub(super) fn resize_graph_no_really_i_mean_it(
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

pub(super) fn resize_layered_node(
    node: &mut LNode,
    new_size: LSize,
    move_ports: bool,
    move_labels: bool,
) {
    let old_size = node.size;
    let width_ratio = ratio_or_one(new_size.width, old_size.width);
    let height_ratio = ratio_or_one(new_size.height, old_size.height);
    let width_diff = new_size.width - old_size.width;
    let height_diff = new_size.height - old_size.height;

    if move_ports {
        let fixed_ports = node.port_constraints == PortConstraints::FixedPos;
        for port in node.ports.slots_mut() {
            match port.side {
                PortSide::North => {
                    if !fixed_ports {
                        port.position.x *= width_ratio;
                    }
                }
                PortSide::East => {
                    port.position.x += width_diff;
                    if !fixed_ports {
                        port.position.y *= height_ratio;
                    }
                }
                PortSide::South => {
                    if !fixed_ports {
                        port.position.x *= width_ratio;
                    }
                    port.position.y += height_diff;
                }
                PortSide::West => {
                    if !fixed_ports {
                        port.position.y *= height_ratio;
                    }
                }
                PortSide::Undefined => {}
            }
        }
    }

    if move_labels {
        for label in &mut node.labels {
            let mid_x = label.position.x + label.size.width / 2.0;
            let mid_y = label.position.y + label.size.height / 2.0;
            let width_percent = ratio_or_zero(mid_x, old_size.width);
            let height_percent = ratio_or_zero(mid_y, old_size.height);

            if width_percent + height_percent >= 1.0 {
                if width_percent - height_percent > 0.0 && mid_y >= 0.0 {
                    label.position.x += width_diff;
                    label.position.y += height_diff * height_percent;
                } else if width_percent - height_percent < 0.0 && mid_x >= 0.0 {
                    label.position.x += width_diff * width_percent;
                    label.position.y += height_diff;
                }
            }
        }
    }

    node.size = new_size;
    // `LGraphUtil.resizeNode` fixes the size: later processors keep it.
    node.node_size.constraints = crate::options::SizeConstraints::FIXED;
}

fn ratio_or_one(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        1.0
    } else {
        numerator / denominator
    }
}

fn ratio_or_zero(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}
