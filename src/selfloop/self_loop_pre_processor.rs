//! Port of ELK's `SelfLoopPreProcessor`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/intermediate/SelfLoopPreProcessor.java

use crate::graph::{LGraph, LNodeKind, OtherGraphs, SelfLoopHolder};
use crate::selfloop::self_loop_holder::{install_self_loop_holder, node_has_self_loop};

pub fn preprocess_self_loops(graph: &mut LGraph, others: OtherGraphs<'_>) {
    graph.self_loop_holders.clear();

    for node in graph.node_ids() {
        if graph.layerless_nodes[node].kind() != LNodeKind::Normal
            || !node_has_self_loop(graph, node)
        {
            continue;
        }

        let mut holder = install_self_loop_holder(graph, node);
        hide_self_loop_edges(graph, &holder);
        hide_ports(graph, others, &mut holder);
        graph.self_loop_holders.push(holder);
    }
}

fn hide_self_loop_edges(graph: &mut LGraph, holder: &SelfLoopHolder) {
    for hyper_loop in &holder.hyper_loops {
        for edge in &hyper_loop.edges {
            graph.detach_edge(edge.edge);
        }
    }
}

fn hide_ports(graph: &mut LGraph, others: OtherGraphs<'_>, holder: &mut SelfLoopHolder) {
    let order_fixed = holder.original_port_constraints.is_order_fixed();
    let hierarchy_mode = graph.layerless_nodes[holder.node]
        .nested_graph
        .and_then(|nested| others.get(nested))
        .is_some_and(|nested| nested.graph_properties.external_ports);

    if order_fixed || hierarchy_mode {
        return;
    }

    for hyper_loop in &mut holder.hyper_loops {
        for port in &mut hyper_loop.ports {
            if port.had_only_self_loops {
                port.hidden = true;
                holder.ports_hidden = true;
            }
        }
    }

    if !holder.ports_hidden {
        return;
    }

    // SelfLoopPreProcessor.hidePorts takes the port off its node (`lPort.setNode(null)`), so no
    // later phase sees it. Here the hidden ports leave the node's list but keep their slots, so
    // the holder's ports and edges still name them and `unhide_ports` can list them again.
    let hidden = holder
        .hyper_loops
        .iter()
        .flat_map(|hyper_loop| hyper_loop.ports.iter())
        .filter(|port| port.hidden)
        .map(|port| port.port)
        .collect::<Vec<_>>();
    let node = &mut graph.layerless_nodes[holder.node];
    // In list order, so that coming back they keep the order they had.
    holder.hidden_ports = node.ports.ids().filter(|id| hidden.contains(id)).collect();
    node.ports.unlist(&holder.hidden_ports);
}

/// Lists the ports that `hide_ports` took off the node again, at the end of its port list.
///
/// Java has no such step. `PortRestorer.restorePorts` (called by `SelfLoopPortRestorer`) clears
/// the node's port list and puts every port back with `lPort.setNode(lNode)`. It visits the
/// hidden ports in the order of its target areas (north start, north middle, and so on), never
/// by the order they had. Here the hidden ports come back first, in the order they left, so that
/// the holder's ports and edges, and so the target areas, can name them in the list again.
/// `restore_hidden_ports` then reorders the whole list in Java's order. Its
/// `restored_port_order` leaves the hidden ports out of the old ports it walks in bursts. So the
/// order they come back in does not reach the result.
pub(crate) fn unhide_ports(graph: &mut LGraph, holder: &mut SelfLoopHolder) {
    let node = &mut graph.layerless_nodes[holder.node];
    node.ports.relist(&holder.hidden_ports);
    holder.hidden_ports.clear();
}
