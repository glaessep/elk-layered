//! Port of ELK's `PartitionMidprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PartitionMidprocessor.java

use std::collections::BTreeMap;

use crate::graph::{LGraph, LPoint, LayeredEdge, NodeId, PortRef, PortSide, PortType};

/// Connects every node of a partition to every node of the next higher partition with an edge
/// that only exists while the layerer runs. The layerer then puts the higher partition in later
/// layers.
///
/// Java: `PartitionMidprocessor.process`.
pub fn midprocess_partitions(graph: &mut LGraph) {
    // Java walks `getLayerlessNodes()`, which holds no node that the layer constraints hid.
    let hidden = graph.hidden_mask();
    // Java collects the nodes in a Guava `HashMultimap`, whose sets iterate in the order the
    // nodes were added in elkjs: see `crate::gwt::LinkedSet`. The nodes of one partition
    // therefore stay in the order of the graph's node list. Java sorts the partition ids, which
    // a `BTreeMap` does by itself.
    let mut partitions: BTreeMap<i32, Vec<NodeId>> = BTreeMap::new();
    for node in graph.node_ids() {
        if hidden[node] {
            continue;
        }
        if let Some(partition) = graph.layerless_nodes[node].partition {
            partitions.entry(partition).or_default().push(node);
        }
    }

    for (first, second) in partitions.values().zip(partitions.values().skip(1)) {
        connect_nodes(graph, first, second);
    }
}

/// Java: `PartitionMidprocessor.connectNodes`.
fn connect_nodes(graph: &mut LGraph, first_partition: &[NodeId], second_partition: &[NodeId]) {
    for &node in first_partition {
        let Some(source) = add_dummy_port(graph, node, PortType::Output, PortSide::East) else {
            continue;
        };
        for &other in second_partition {
            let Some(target) = add_dummy_port(graph, other, PortType::Input, PortSide::West) else {
                continue;
            };
            let mut edge = LayeredEdge::new(
                format!("partition:{}", graph.next_edge_id().index()),
                source,
                target,
            );
            edge.partition_dummy = true;
            graph.add_edge(edge);
        }
    }
}

fn add_dummy_port(
    graph: &mut LGraph,
    node: NodeId,
    port_type: PortType,
    side: PortSide,
) -> Option<PortRef> {
    let port = graph.add_port(node, port_type, side, LPoint::default())?;
    graph.layerless_nodes[port.node].ports[port.port].partition_dummy = true;
    Some(port)
}
