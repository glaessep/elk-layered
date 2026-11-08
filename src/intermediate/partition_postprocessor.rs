//! Port of ELK's `PartitionPostprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PartitionPostprocessor.java

use crate::graph::{LGraph, PortId};

/// Removes the ports and edges that [`super::partition_midprocessor`] added, once the layerer
/// is done with them.
///
/// Java: `PartitionPostprocessor.process`. Java walks the nodes of the layers and drops each
/// port with the `PARTITION_DUMMY` property from its node. The edges are left on the dropped
/// ports, where nothing reaches them. Here an edge sits in the graph's edge list too, so this
/// detaches each one and removes it from the end of that list. Later steps, which join the
/// parts of a split graph, then never see it.
pub fn postprocess_partitions(graph: &mut LGraph) {
    // The marked ports exist only on nodes that the midprocessor saw, and every one of those sits
    // in a layer now, so walking all nodes finds the same ports as walking the layers.
    for node in graph.node_ids() {
        let ports: Vec<PortId> = graph.layerless_nodes[node]
            .ports
            .iter_with_ids()
            .filter(|(_, port)| port.partition_dummy)
            .map(|(id, _)| id)
            .collect();
        if ports.is_empty() {
            continue;
        }
        for &id in &ports {
            let port = &graph.layerless_nodes[node].ports[id];
            let edges: Vec<_> = port
                .incoming_edges()
                .iter()
                .chain(port.outgoing_edges())
                .copied()
                .collect();
            for edge in edges {
                graph.detach_edge(edge);
            }
        }
        graph.layerless_nodes[node].ports.unlist(&ports);
    }

    // Nothing adds an edge between the midprocessor and this step, so the marked edges are the
    // last ones. A marked edge anywhere else stays detached.
    while graph.edges.last().is_some_and(|edge| edge.partition_dummy) {
        graph.edges.pop();
    }
}
