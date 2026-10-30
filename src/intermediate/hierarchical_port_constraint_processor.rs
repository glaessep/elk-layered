//! Port of ELK's `HierarchicalPortConstraintProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/HierarchicalPortConstraintProcessor.java

use crate::graph::{
    DummyData, ExternalPortData, GraphId, LGraph, LNode, NodeId, PortId, PortRef, PortSide,
    PortType,
};
use crate::options::{Alignment, PortConstraints};

pub fn process_hierarchical_port_constraints(graph: &mut LGraph) {
    process_east_west_hierarchical_port_constraints(graph);
    process_north_south_hierarchical_port_constraints(graph);
}

fn process_east_west_hierarchical_port_constraints(graph: &mut LGraph) {
    if !graph.options.port_constraints.is_order_fixed() || graph.layers().is_empty() {
        return;
    }

    process_east_west_hierarchical_port_constraints_for_layer(graph, 0);
    if graph.layers().len() > 1 {
        process_east_west_hierarchical_port_constraints_for_layer(graph, graph.layers().len() - 1);
    }
}

fn process_east_west_hierarchical_port_constraints_for_layer(
    graph: &mut LGraph,
    layer_index: usize,
) {
    let mut nodes = graph.layers()[layer_index].nodes().to_vec();
    // ELK's comparator answers "less" for any node against one without an external port, both
    // ways round, so it is no total order: see `crate::gwt::sort`. Only two regular nodes compare
    // inconsistently, so Rust's sort gives the dummies the same order, but it may panic on it.
    crate::gwt::sort(&mut nodes, |left, right| {
        let left_node = &graph.layerless_nodes[left];
        let right_node = &graph.layerless_nodes[right];

        match (left_node.external_port(), right_node.external_port()) {
            (_, None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(left), Some(right)) => left.ratio_or_position.total_cmp(&right.ratio_or_position),
        }
    });

    let mut last_hierarchical_dummy: Option<NodeId> = None;
    for node in nodes {
        let Some(external) = graph.layerless_nodes[node].external_port() else {
            break;
        };
        if !matches!(external.side, PortSide::West | PortSide::East) {
            continue;
        }

        if let Some(previous) = last_hierarchical_dummy {
            graph
                .tables
                .in_layer
                .row_mut(previous)
                .successor_constraints
                .push(node);
        }
        last_hierarchical_dummy = Some(node);
    }
}

fn process_north_south_hierarchical_port_constraints(graph: &mut LGraph) {
    if !graph.options.port_constraints.is_side_fixed() {
        return;
    }

    let original_layer_count = graph.layers().len();
    let mut new_dummy_nodes = vec![Vec::<NodeId>::new(); original_layer_count + 2];
    let mut replacement_maps =
        vec![Vec::<(NorthSouthReplacementKey, ReplacementDummy)>::new(); original_layer_count + 2];
    let mut original_external_port_dummies = Vec::new();

    for layer_index in 0..original_layer_count {
        let current_nodes = graph.layers()[layer_index].nodes().to_vec();

        for current_node in current_nodes {
            if is_north_south_external_port_dummy(graph, current_node) {
                original_external_port_dummies.push(current_node);
                continue;
            }

            let incoming_edges = graph.node_incoming_edges(current_node);
            for edge in incoming_edges {
                let source_node = graph.edges[edge].source().node;
                if !is_north_south_external_port_dummy(graph, source_node) {
                    continue;
                }

                let replacement = replacement_dummy_for_original(
                    graph,
                    source_node,
                    layer_index,
                    &mut replacement_maps,
                    &mut new_dummy_nodes,
                );
                graph.set_edge_source(
                    edge,
                    PortRef {
                        node: replacement.node,
                        port: replacement.output,
                    },
                );
            }

            let outgoing_edges = graph.node_outgoing_edges(current_node);
            for edge in outgoing_edges {
                let target_node = graph.edges[edge].target().node;
                if !is_north_south_external_port_dummy(graph, target_node) {
                    continue;
                }

                let replacement = replacement_dummy_for_original(
                    graph,
                    target_node,
                    layer_index + 2,
                    &mut replacement_maps,
                    &mut new_dummy_nodes,
                );
                graph.set_edge_target(
                    edge,
                    PortRef {
                        node: replacement.node,
                        port: replacement.input,
                    },
                );
            }
        }
    }

    for (virtual_layer, nodes) in new_dummy_nodes.into_iter().enumerate() {
        if nodes.is_empty() {
            continue;
        }

        let layer_index = if virtual_layer == 0 {
            graph.insert_layer(0);
            0
        } else if virtual_layer == original_layer_count + 1 {
            graph.push_layer();
            graph.layers().len() - 1
        } else {
            virtual_layer - 1
        };

        for node in nodes {
            graph.set_node_layer(node, layer_index);
        }
    }

    for original_dummy in original_external_port_dummies {
        graph.remove_node_from_layer(original_dummy);
        if !graph
            .replaced_external_port_dummies
            .contains(&original_dummy)
        {
            graph.replaced_external_port_dummies.push(original_dummy);
        }
    }
}

fn is_north_south_external_port_dummy(graph: &LGraph, node: NodeId) -> bool {
    graph.layerless_nodes[node]
        .external_port()
        .is_some_and(|external| matches!(external.side, PortSide::North | PortSide::South))
}

fn replacement_dummy_for_original(
    graph: &mut LGraph,
    original_dummy: NodeId,
    virtual_layer: usize,
    maps: &mut [Vec<(NorthSouthReplacementKey, ReplacementDummy)>],
    new_dummy_nodes: &mut [Vec<NodeId>],
) -> ReplacementDummy {
    let key = north_south_replacement_key(graph, original_dummy);
    if let Some((_, replacement)) = maps[virtual_layer]
        .iter()
        .find(|(candidate, _)| *candidate == key)
    {
        return *replacement;
    }

    let replacement = create_hierarchical_port_replacement_dummy(graph, original_dummy);
    maps[virtual_layer].push((key, replacement));
    new_dummy_nodes[virtual_layer].push(replacement.node);
    replacement
}

/// A north/south replacement dummy and the ids of the two ports it was made with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReplacementDummy {
    node: NodeId,
    /// The west input port.
    input: PortId,
    /// The east output port.
    output: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NorthSouthReplacementKey {
    Origin { graph_id: GraphId, port: PortRef },
    Dummy(NodeId),
}

fn north_south_replacement_key(graph: &LGraph, original_dummy: NodeId) -> NorthSouthReplacementKey {
    graph
        .layerless_nodes
        .get(original_dummy)
        .and_then(|node| node.external_port())
        .and_then(|external| external.origin_port.as_ref())
        .map(|origin| NorthSouthReplacementKey::Origin {
            graph_id: origin.graph_id,
            port: origin.port,
        })
        .unwrap_or(NorthSouthReplacementKey::Dummy(original_dummy))
}

pub(crate) fn create_hierarchical_port_replacement_dummy(
    graph: &mut LGraph,
    original_dummy: NodeId,
) -> ReplacementDummy {
    let original = graph.layerless_nodes[original_dummy].clone();
    let original_external = original
        .external_port()
        .expect("only an external port dummy gets a replacement")
        .clone();
    let node_index = graph.push_node_with(|id| {
        let mut replacement = LNode::new(
            format!("{}:replacement:{}", original.id, id.index()),
            original.size.width,
            original.size.height,
            None,
        );
        replacement.set_dummy(DummyData::ExternalPort(ExternalPortData {
            side: original_external.side,
            size: original_external.size,
            ratio_or_position: original_external.ratio_or_position,
            replaced_dummy: Some(original_dummy),
            origin_port: original_external.origin_port,
            in_layer_constraint: original_external.in_layer_constraint,
            anchor: original_external.anchor,
            ..ExternalPortData::default()
        }));
        replacement.margin = original.margin;
        replacement.padding = original.padding;
        replacement.node_label_placement = original.node_label_placement;
        replacement.model_order = original.model_order;
        replacement.copy_layer_constraint_from(&original);
        replacement.port_constraints = PortConstraints::FixedPos;
        replacement.node_alignment = Alignment::Center;
        replacement
    });
    // The replacement takes over the successors of the dummy it stands in for.
    let successor_constraints = graph
        .in_layer_successor_constraints(original_dummy)
        .to_vec();
    if !successor_constraints.is_empty() {
        graph
            .tables
            .in_layer
            .row_mut(node_index)
            .successor_constraints = successor_constraints;
    }

    let input = graph
        .add_port(
            node_index,
            PortType::Input,
            PortSide::West,
            Default::default(),
        )
        .expect("replacement node was just inserted");
    let output = graph
        .add_port(
            node_index,
            PortType::Output,
            PortSide::East,
            Default::default(),
        )
        .expect("replacement node was just inserted");
    ReplacementDummy {
        node: node_index,
        input: input.port,
        output: output.port,
    }
}
