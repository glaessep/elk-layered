//! Common node dimension calculation utilities.
//!
//! Source references:
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/nodespacing/NodeDimensionCalculation.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/nodespacing/NodeLabelAndSizeCalculator.java
//! - https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/nodespacing/NodeMarginCalculator.java

use super::cells::{self, CellLabel, CellNode, CellPort, CellPortLabel, CellPorts};
use crate::graph::{LGraph, LMargin, LPadding, NodeId, PortId};
use crate::options::ElkPadding;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn union(&mut self, other: Self) {
        let min_x = self.x.min(other.x);
        let min_y = self.y.min(other.y);
        let max_x = (self.x + self.width).max(other.x + other.width);
        let max_y = (self.y + self.height).max(other.y + other.height);
        self.x = min_x;
        self.y = min_y;
        self.width = max_x - min_x;
        self.height = max_y - min_y;
    }
}

/// Sizes each node and places its ports and labels.
///
/// Source: `NodeLabelAndSizeCalculator.process(graph, node, true, false)`, through
/// `LGraphAdapters.adapt(graph, true, true, ...)`: edges at north and south port dummies count
/// as edges of their ports.
pub fn calculate_label_and_node_sizes(graph: &mut LGraph, nodes: impl IntoIterator<Item = NodeId>) {
    for node in nodes {
        size_node_and_place_ports_and_labels(graph, node);
    }
}

pub fn calculate_node_margins(graph: &mut LGraph, nodes: impl IntoIterator<Item = NodeId>) {
    for node in nodes {
        process_node_margin(graph, node);
    }
}

/// Source: `LPortAdapter.getIncomingEdges` and `getOutgoingEdges`, with transparent north and
/// south edges, asked whether they are empty: the port's own edges, those of its north or south
/// port dummy, and its self loops, which the self loop holder keeps apart.
fn port_has_edges(graph: &LGraph, node: NodeId, port: PortId) -> bool {
    let data = &graph.layerless_nodes[node].ports[port];
    if !data.incoming_edges().is_empty() || !data.outgoing_edges().is_empty() {
        return true;
    }
    // A port of a node with children has its dummy in the graph inside the node. Its labels
    // are refused, and only labels read the edges, so that dummy is left out.
    if let Some(dummy) = data.port_dummy
        && dummy.graph_id == graph.id
    {
        let dummy = &graph.layerless_nodes[dummy.node];
        if dummy
            .ports
            .iter()
            .any(|p| !p.incoming_edges().is_empty() || !p.outgoing_edges().is_empty())
        {
            return true;
        }
    }
    graph
        .self_loop_holders
        .iter()
        .filter(|holder| holder.node == node)
        .flat_map(|holder| &holder.hyper_loops)
        .flat_map(|hyper_loop| &hyper_loop.edges)
        .any(|edge| edge.source_port == port || edge.target_port == port)
}

/// Runs ELK's cell system on one node: its size, when its size constraints are not fixed, the
/// positions of its ports, and the positions of the labels it places.
fn size_node_and_place_ports_and_labels(graph: &mut LGraph, node: NodeId) {
    let horizontal_layout_mode = !graph.options.direction.is_vertical();
    let data = &graph.layerless_nodes[node];
    let surrounding = graph.options.spacing.ports_surrounding;
    let ports = CellPorts {
        ports: data
            .ports
            .iter_with_ids()
            .map(|(id, port)| CellPort {
                side: port.side,
                size: port.size,
                position: port.position,
                border_offset: port.border_offset,
                ratio_or_position: port.ratio_or_position,
                labels: port
                    .labels
                    .iter()
                    .map(|label| CellPortLabel {
                        size: label.size,
                        position: label.position,
                    })
                    .collect(),
                has_edges: port_has_edges(graph, node, id),
                has_compound_connections: port.inside_connections,
            })
            .collect(),
        constraints: data.port_constraints,
        label_placement: data.port_label_placement,
        treat_as_group: data.port_labels_treat_as_group,
        compound: data.nested_graph.is_some(),
        port_port_spacing: graph.options.spacing.port_port,
        label_port_horizontal: graph.options.spacing.label_port_horizontal,
        label_port_vertical: graph.options.spacing.label_port_vertical,
        surrounding: ElkPadding {
            top: surrounding.top,
            right: surrounding.right,
            bottom: surrounding.bottom,
            left: surrounding.left,
        },
        alignment: graph
            .port_alignment_of(node)
            .unwrap_or(graph.options.port_alignment_default),
    };
    let cell_node = CellNode {
        size: data.size,
        sizing: data.node_size,
        placement: data.node_label_placement,
        labels: data
            .labels
            .iter()
            .map(|label| CellLabel {
                size: label.size,
                placement: label.node_label_placement,
            })
            .collect(),
        node_labels_padding: graph.options.node_labels_padding,
        label_node_spacing: graph.options.spacing.label_node,
        label_label_spacing: graph.options.spacing.label_label,
        fixed_graph_size: false,
        ports,
    };
    let result = cells::process(&cell_node, horizontal_layout_mode, true, false);
    let data = &mut graph.layerless_nodes[node];
    data.size = result.size;
    for (label, position) in data.labels.iter_mut().zip(result.label_positions) {
        if let Some(position) = position {
            label.position = position;
        }
    }
    let port_ids: Vec<PortId> = data.ports.ids().collect();
    for (id, (position, label_positions)) in port_ids.into_iter().zip(
        result
            .port_positions
            .into_iter()
            .zip(result.port_label_positions),
    ) {
        let port = &mut data.ports[id];
        port.position = position;
        for (label, position) in port.labels.iter_mut().zip(label_positions) {
            if let Some(position) = position {
                label.position = position;
            }
        }
    }
    if let Some(padding) = result.padding {
        data.padding = LPadding {
            top: padding.top,
            right: padding.right,
            bottom: padding.bottom,
            left: padding.left,
        };
    }
}

fn process_node_margin(graph: &mut LGraph, node: NodeId) {
    let position = graph.layerless_nodes[node].position;
    let size = graph.layerless_nodes[node].size;
    let mut bounds = Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };

    for label in &graph.layerless_nodes[node].labels {
        bounds.union(Rect {
            x: position.x + label.position.x,
            y: position.y + label.position.y,
            width: label.size.width,
            height: label.size.height,
        });
    }

    for port in &graph.layerless_nodes[node].ports {
        let port_x = position.x + port.position.x;
        let port_y = position.y + port.position.y;
        bounds.union(Rect {
            x: port_x,
            y: port_y,
            width: port.size.width,
            height: port.size.height,
        });

        for label in &port.labels {
            bounds.union(Rect {
                x: port_x + label.position.x,
                y: port_y + label.position.y,
                width: label.size.width,
                height: label.size.height,
            });
        }
    }

    graph.layerless_nodes[node].margin = LMargin {
        top: (position.y - bounds.y).max(0.0),
        bottom: (bounds.y + bounds.height - (position.y + size.height)).max(0.0),
        left: (position.x - bounds.x).max(0.0),
        right: (bounds.x + bounds.width - (position.x + size.width)).max(0.0),
    };
}

#[cfg(test)]
mod tests;
