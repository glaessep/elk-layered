//! Port of ELK's `PartitionPostprocessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/PartitionPostprocessor.java

use crate::graph::{LGraph, PortId};

use super::{IntermediateError, IntermediateResult};

/// Removes the ports and edges that [`super::partition_midprocessor`] added, once the layerer
/// is done with them.
///
/// Java: `PartitionPostprocessor.process`. Java walks the nodes of the layers and drops each
/// port with the `PARTITION_DUMMY` property from its node. The edges stay on the dropped ports,
/// where nothing reaches them. Here an edge also sits in the graph's edge list, so this step cuts
/// the marked edges off the end of that list too. Later steps, which join the parts of a split
/// graph, then never see them.
///
/// Fails when an unmarked edge follows a marked one in the edge list: some step between the
/// midprocessor and this one added an edge, and cutting the list would drop it.
pub fn postprocess_partitions(graph: &mut LGraph) -> IntermediateResult<()> {
    // The midprocessor appended its edges last. Check that nothing came after them before any
    // change, so a failure leaves the graph as it was.
    let first = graph.edges.iter().position(|edge| edge.partition_dummy);
    if let Some(first) = first
        && let Some(edge) = graph
            .edges
            .iter()
            .skip(first)
            .find(|edge| !edge.partition_dummy)
    {
        return Err(IntermediateError::EdgeAfterPartitionEdges {
            edge_id: edge.id.clone(),
        });
    }

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
        // A marked edge runs between two marked ports, so no port that stays names one. The
        // dropped ports forget theirs, which takes one step per port, not one per edge.
        for &id in &ports {
            graph.layerless_nodes[node].ports[id].forget_edges();
        }
        graph.layerless_nodes[node].ports.unlist(&ports);
    }

    if let Some(first) = first {
        graph.edges.truncate(first);
    }
    Ok(())
}
