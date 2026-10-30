//! Port of ELK's `GraphTransformer`, which turns the graph for the layout direction.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/GraphTransformer.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/options/DirectionCongruency.java

use crate::graph::{
    ExternalPortData, InLayerConstraint, LGraph, LLabel, LNode, LPadding, LPoint, LPort, LSize,
    PortSide,
};
use crate::options::{Alignment, DirectionCongruency, ElkDirection, ElkPadding, LayerConstraint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphTransformMode {
    ToInternalLeftToRight,
    ToInputDirection,
}

pub fn transform_graph_direction(graph: &mut LGraph, mode: GraphTransformMode) {
    match graph.options.direction {
        ElkDirection::Left | ElkDirection::Down | ElkDirection::Up => {}
        ElkDirection::Right | ElkDirection::Undefined => return,
    }

    match graph.options.direction_congruency {
        DirectionCongruency::ReadingDirection => match graph.options.direction {
            ElkDirection::Left => mirror_all_x(graph),
            ElkDirection::Down => transpose_all(graph),
            ElkDirection::Up => match mode {
                GraphTransformMode::ToInternalLeftToRight => {
                    transpose_all(graph);
                    mirror_all_y(graph);
                }
                GraphTransformMode::ToInputDirection => {
                    mirror_all_y(graph);
                    transpose_all(graph);
                }
            },
            ElkDirection::Right | ElkDirection::Undefined => {}
        },
        DirectionCongruency::Rotation => match mode {
            GraphTransformMode::ToInternalLeftToRight => match graph.options.direction {
                ElkDirection::Left => {
                    mirror_all_x(graph);
                    mirror_all_y(graph);
                }
                ElkDirection::Down => rotate90_clockwise(graph),
                ElkDirection::Up => rotate90_counter_clockwise(graph),
                ElkDirection::Right | ElkDirection::Undefined => {}
            },
            GraphTransformMode::ToInputDirection => match graph.options.direction {
                ElkDirection::Left => {
                    mirror_all_x(graph);
                    mirror_all_y(graph);
                }
                ElkDirection::Down => rotate90_counter_clockwise(graph),
                ElkDirection::Up => rotate90_clockwise(graph),
                ElkDirection::Right | ElkDirection::Undefined => {}
            },
        },
    }
}

fn rotate90_clockwise(graph: &mut LGraph) {
    transpose_all(graph);
    mirror_all_x(graph);
}

fn rotate90_counter_clockwise(graph: &mut LGraph) {
    mirror_all_x(graph);
    transpose_all(graph);
}

fn mirror_all_x(graph: &mut LGraph) {
    let offset = graph_x_offset(graph);
    graph.padding.swap_left_right();
    graph.options.node_labels_padding.swap_left_right();

    for node in &mut graph.layerless_nodes {
        mirror_node_x(node, offset);
    }
    for edge in &mut graph.edges {
        for point in &mut edge.bend_points {
            point.mirror_x(offset);
        }
        // The junction points move with the bend points.
        for point in &mut edge.junction_points {
            point.mirror_x(offset);
        }
        for label in &mut edge.labels {
            mirror_label_x(label, offset);
        }
    }
}

fn mirror_node_x(node: &mut LNode, offset: f64) {
    node.position.mirror_x(offset - node.size.width);
    node.padding.swap_left_right();
    node.node_label_placement = node.node_label_placement.mirror_x();
    node.node_alignment = mirror_alignment_x(node.node_alignment);
    for port in node.ports.slots_mut() {
        mirror_port_x(port, node.size.width);
    }
    if let Some((external, layer_constraint)) = node.external_port_and_layer_constraint_mut() {
        *layer_constraint = mirror_layer_constraint_x(*layer_constraint);
        external.side = mirror_port_side_x(external.side);
    }
    for label in &mut node.labels {
        label.node_label_placement = label.node_label_placement.map(|p| p.mirror_x());
        mirror_label_x_within_node(label, node.size.width);
    }
}

fn mirror_port_x(port: &mut LPort, node_width: f64) {
    port.position.mirror_x(node_width - port.size.width);
    port.anchor.mirror_x(port.size.width);
    port.side = mirror_port_side_x(port.side);
    reverse_port_index(port);
    for label in &mut port.labels {
        mirror_label_x_within_node(label, port.size.width);
    }
}

fn mirror_all_y(graph: &mut LGraph) {
    let offset = graph_y_offset(graph);
    graph.padding.swap_top_bottom();
    graph.options.node_labels_padding.swap_top_bottom();

    for node in &mut graph.layerless_nodes {
        mirror_node_y(node, offset);
    }
    for edge in &mut graph.edges {
        for point in &mut edge.bend_points {
            point.mirror_y(offset);
        }
        // The junction points move with the bend points.
        for point in &mut edge.junction_points {
            point.mirror_y(offset);
        }
        for label in &mut edge.labels {
            mirror_label_y(label, offset);
        }
    }
}

fn mirror_node_y(node: &mut LNode, offset: f64) {
    node.position.mirror_y(offset - node.size.height);
    node.padding.swap_top_bottom();
    node.node_label_placement = node.node_label_placement.mirror_y();
    node.node_alignment = mirror_alignment_y(node.node_alignment);
    if let Some(external) = node.external_port_mut() {
        external.in_layer_constraint = match external.in_layer_constraint {
            InLayerConstraint::Top => InLayerConstraint::Bottom,
            InLayerConstraint::Bottom => InLayerConstraint::Top,
            InLayerConstraint::None => InLayerConstraint::None,
        };
        external.side = mirror_port_side_y(external.side);
    }
    for port in node.ports.slots_mut() {
        mirror_port_y(port, node.size.height);
    }
    for label in &mut node.labels {
        label.node_label_placement = label.node_label_placement.map(|p| p.mirror_y());
        mirror_label_y_within_node(label, node.size.height);
    }
}

fn mirror_port_y(port: &mut LPort, node_height: f64) {
    port.position.mirror_y(node_height - port.size.height);
    port.anchor.mirror_y(port.size.height);
    port.side = mirror_port_side_y(port.side);
    reverse_port_index(port);
    for label in &mut port.labels {
        mirror_label_y_within_node(label, port.size.height);
    }
}

fn transpose_all(graph: &mut LGraph) {
    graph.offset.transpose();
    graph.size.transpose();
    graph.padding.transpose();
    graph.options.node_labels_padding.transpose();
    graph.options.edge_label_side_selection = graph.options.edge_label_side_selection.transpose();

    for node in &mut graph.layerless_nodes {
        transpose_node(node);
    }
    for edge in &mut graph.edges {
        for point in &mut edge.bend_points {
            point.transpose();
        }
        // The junction points move with the bend points.
        for point in &mut edge.junction_points {
            point.transpose();
        }
        for label in &mut edge.labels {
            transpose_label(label);
        }
    }
}

fn transpose_node(node: &mut LNode) {
    node.position.transpose();
    node.size.transpose();
    node.padding.transpose();
    node.node_label_placement = node.node_label_placement.transpose();
    // `GraphTransformer.transposeProperties` swaps the minimum size too.
    node.node_size.minimum.transpose();
    node.node_alignment = transpose_alignment(node.node_alignment);
    for port in node.ports.slots_mut() {
        transpose_port(port);
    }
    if let Some((external, layer_constraint)) = node.external_port_and_layer_constraint_mut() {
        transpose_layer_constraint_for_external_port(external, layer_constraint);
        external.side = transpose_port_side(external.side);
    }
    for label in &mut node.labels {
        label.node_label_placement = label.node_label_placement.map(|p| p.transpose());
        transpose_label(label);
    }
}

fn transpose_port(port: &mut LPort) {
    port.position.transpose();
    port.anchor.transpose();
    port.size.transpose();
    port.side = transpose_port_side(port.side);
    reverse_port_index(port);
    for label in &mut port.labels {
        transpose_label(label);
    }
}

fn graph_x_offset(graph: &LGraph) -> f64 {
    let offset = if graph.size.width == 0.0 {
        graph
            .layerless_nodes
            .iter()
            .map(|node| node.position.x + node.size.width + node.margin.right)
            .fold(0.0, f64::max)
    } else {
        graph.size.width - graph.offset.x
    };
    offset - graph.offset.x
}

fn graph_y_offset(graph: &LGraph) -> f64 {
    let offset = if graph.size.height == 0.0 {
        graph
            .layerless_nodes
            .iter()
            .map(|node| node.position.y + node.size.height + node.margin.bottom)
            .fold(0.0, f64::max)
    } else {
        graph.size.height - graph.offset.y
    };
    offset - graph.offset.y
}

fn mirror_label_x(label: &mut LLabel, offset: f64) {
    label.position.mirror_x(offset - label.size.width);
}

fn mirror_label_y(label: &mut LLabel, offset: f64) {
    label.position.mirror_y(offset - label.size.height);
}

fn mirror_label_x_within_node(label: &mut LLabel, node_width: f64) {
    label.position.mirror_x(node_width - label.size.width);
}

fn mirror_label_y_within_node(label: &mut LLabel, node_height: f64) {
    label.position.mirror_y(node_height - label.size.height);
}

fn transpose_label(label: &mut LLabel) {
    label.position.transpose();
    label.size.transpose();
}

fn mirror_alignment_x(alignment: Alignment) -> Alignment {
    match alignment {
        Alignment::Left => Alignment::Right,
        Alignment::Right => Alignment::Left,
        alignment => alignment,
    }
}

fn mirror_alignment_y(alignment: Alignment) -> Alignment {
    match alignment {
        Alignment::Top => Alignment::Bottom,
        Alignment::Bottom => Alignment::Top,
        alignment => alignment,
    }
}

fn transpose_alignment(alignment: Alignment) -> Alignment {
    match alignment {
        Alignment::Left => Alignment::Top,
        Alignment::Right => Alignment::Bottom,
        Alignment::Top => Alignment::Left,
        Alignment::Bottom => Alignment::Right,
        alignment => alignment,
    }
}

fn mirror_port_side_x(side: PortSide) -> PortSide {
    match side {
        PortSide::East => PortSide::West,
        PortSide::West => PortSide::East,
        side => side,
    }
}

fn mirror_port_side_y(side: PortSide) -> PortSide {
    match side {
        PortSide::North => PortSide::South,
        PortSide::South => PortSide::North,
        side => side,
    }
}

fn transpose_port_side(side: PortSide) -> PortSide {
    match side {
        PortSide::North => PortSide::West,
        PortSide::West => PortSide::North,
        PortSide::South => PortSide::East,
        PortSide::East => PortSide::South,
        PortSide::Undefined => PortSide::Undefined,
    }
}

fn mirror_layer_constraint_x(layer_constraint: LayerConstraint) -> LayerConstraint {
    match layer_constraint {
        LayerConstraint::First => LayerConstraint::Last,
        LayerConstraint::FirstSeparate => LayerConstraint::LastSeparate,
        LayerConstraint::Last => LayerConstraint::First,
        LayerConstraint::LastSeparate => LayerConstraint::FirstSeparate,
        LayerConstraint::None => LayerConstraint::None,
    }
}

fn transpose_layer_constraint_for_external_port(
    external: &mut ExternalPortData,
    layer_constraint: &mut LayerConstraint,
) {
    if *layer_constraint == LayerConstraint::FirstSeparate {
        *layer_constraint = LayerConstraint::None;
        external.in_layer_constraint = InLayerConstraint::Top;
    } else if *layer_constraint == LayerConstraint::LastSeparate {
        *layer_constraint = LayerConstraint::None;
        external.in_layer_constraint = InLayerConstraint::Bottom;
    } else if external.in_layer_constraint == InLayerConstraint::Top {
        *layer_constraint = LayerConstraint::FirstSeparate;
        external.in_layer_constraint = InLayerConstraint::None;
        // ELK sets the constraint with `setProperty`, so it counts as explicit from here on.
        external.layer_constraint_explicit = true;
    } else if external.in_layer_constraint == InLayerConstraint::Bottom {
        *layer_constraint = LayerConstraint::LastSeparate;
        external.in_layer_constraint = InLayerConstraint::None;
        external.layer_constraint_explicit = true;
    }
}

fn reverse_port_index(port: &mut LPort) {
    if let Some(index) = port.port_index.as_mut() {
        *index = -*index;
    }
}

trait PointTransform {
    fn mirror_x(&mut self, offset: f64);
    fn mirror_y(&mut self, offset: f64);
    fn transpose(&mut self);
}

impl PointTransform for LPoint {
    fn mirror_x(&mut self, offset: f64) {
        self.x = offset - self.x;
    }

    fn mirror_y(&mut self, offset: f64) {
        self.y = offset - self.y;
    }

    fn transpose(&mut self) {
        std::mem::swap(&mut self.x, &mut self.y);
    }
}

trait SizeTransform {
    fn transpose(&mut self);
}

impl SizeTransform for LSize {
    fn transpose(&mut self) {
        std::mem::swap(&mut self.width, &mut self.height);
    }
}

trait PaddingTransform {
    fn swap_left_right(&mut self);
    fn swap_top_bottom(&mut self);
    fn transpose(&mut self);
}

impl PaddingTransform for LPadding {
    fn swap_left_right(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }

    fn swap_top_bottom(&mut self) {
        std::mem::swap(&mut self.top, &mut self.bottom);
    }

    fn transpose(&mut self) {
        let old_top = self.top;
        let old_bottom = self.bottom;
        let old_left = self.left;
        let old_right = self.right;

        self.top = old_left;
        self.bottom = old_right;
        self.left = old_top;
        self.right = old_bottom;
    }
}

impl PaddingTransform for ElkPadding {
    fn swap_left_right(&mut self) {
        std::mem::swap(&mut self.left, &mut self.right);
    }

    fn swap_top_bottom(&mut self) {
        std::mem::swap(&mut self.top, &mut self.bottom);
    }

    fn transpose(&mut self) {
        let old_top = self.top;
        let old_bottom = self.bottom;
        let old_left = self.left;
        let old_right = self.right;

        self.top = old_left;
        self.bottom = old_right;
        self.left = old_top;
        self.right = old_bottom;
    }
}

#[cfg(test)]
mod tests;
