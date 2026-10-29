//! Coffman-Graham layering translated from Eclipse ELK, EPL-2.0:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/CoffmanGrahamLayerer.java
//!
//! Priority-queue ties follow GWT's `java.util.PriorityQueue`, as elkjs 0.9.3 runs it: see
//! [`crate::gwt::priority_queue_poll`]. Those ties decide the node order.

use super::{LayeringError, LayeringInput, LayeringResult};
use crate::graph::{IdVec, NodeId};
use std::cmp::Ordering;

use crate::gwt::{priority_queue_offer, priority_queue_poll};

pub(super) fn layer(input: &LayeringInput<'_>) -> LayeringResult<Vec<Vec<NodeId>>> {
    if input.nodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut marked_edges = IdVec::filled(input.graph.edges.len(), false);
    let mut visited = IdVec::filled(input.len(), false);
    let mut active = IdVec::filled(input.len(), false);
    let mut stack = Vec::new();
    // Source transitive reduction marks edges; it does not remove them from the graph.
    for &start in &input.nodes {
        visited.fill(false);
        for &edge in &input.outgoing[start] {
            let root = input.target(edge);
            if visited[root] {
                continue;
            }
            active[root] = true;
            stack.push((root, 0usize));
            while let Some((node, cursor)) = stack.last_mut() {
                if let Some(&edge) = input.outgoing[*node].get(*cursor) {
                    *cursor += 1;
                    let next = input.target(edge);
                    for &transitive in &input.incoming[next] {
                        if input.source(transitive) == start {
                            marked_edges[transitive] = true;
                        }
                    }
                    if active[next] {
                        return Err(LayeringError::CyclicGraph);
                    }
                    if !visited[next] {
                        active[next] = true;
                        stack.push((next, 0));
                    }
                } else {
                    let (node, _) = stack.pop().expect("the DFS frame exists");
                    visited[node] = true;
                    active[node] = false;
                }
            }
        }
    }

    let mut incoming = IdVec::filled(input.len(), 0usize);
    let mut predecessors = IdVec::filled(input.len(), Vec::new());
    let mut topological = IdVec::filled(input.len(), 0usize);
    let mut sources = Vec::new();
    for &node in &input.nodes {
        incoming[node] = input.incoming[node]
            .iter()
            .filter(|&&edge| !marked_edges[edge])
            .count();
        if incoming[node] == 0 {
            priority_queue_offer(&mut sources, node, |a, b| {
                compare_topology(&predecessors[a], &predecessors[b])
            });
        }
    }
    let mut count = 0;
    while let Some(node) = priority_queue_poll(&mut sources, |a, b| {
        compare_topology(&predecessors[a], &predecessors[b])
    }) {
        topological[node] = count;
        count += 1;
        for &edge in &input.outgoing[node] {
            if marked_edges[edge] {
                continue;
            }
            let next = input.target(edge);
            incoming[next] -= 1;
            predecessors[next].push(topological[node]);
            if incoming[next] == 0 {
                priority_queue_offer(&mut sources, next, |a, b| {
                    compare_topology(&predecessors[a], &predecessors[b])
                });
            }
        }
    }
    if count != input.nodes.len() {
        return Err(LayeringError::CyclicGraph);
    }

    let mut outgoing = IdVec::filled(input.len(), 0usize);
    let mut sinks = Vec::new();
    for &node in &input.nodes {
        outgoing[node] = input.outgoing[node]
            .iter()
            .filter(|&&edge| !marked_edges[edge])
            .count();
        if outgoing[node] == 0 {
            priority_queue_offer(&mut sinks, node, |a, b| topological[b].cmp(&topological[a]));
        }
    }
    let mut layers: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut assigned = IdVec::filled(input.len(), None);
    let bound = input.graph.options.layering_coffman_graham_layer_bound;
    while let Some(node) =
        priority_queue_poll(&mut sinks, |a, b| topological[b].cmp(&topological[a]))
    {
        let mut layer = layers.len() - 1;
        if layers[layer].len() as i64 >= i64::from(bound)
            || input.outgoing[node]
                .iter()
                .any(|&edge| assigned[input.target(edge)] == Some(layer))
        {
            layers.push(Vec::new());
            layer += 1;
        }
        assigned[node] = Some(layer);
        layers[layer].push(node);
        for &edge in &input.incoming[node] {
            if marked_edges[edge] {
                continue;
            }
            let source = input.source(edge);
            outgoing[source] -= 1;
            if outgoing[source] == 0 {
                priority_queue_offer(&mut sinks, source, |a, b| {
                    topological[b].cmp(&topological[a])
                });
            }
        }
    }
    layers.reverse();
    Ok(layers)
}

fn compare_topology(left: &[usize], right: &[usize]) -> Ordering {
    let mut a = left.len();
    let mut b = right.len();
    while a > 0 && b > 0 {
        a -= 1;
        b -= 1;
        // Upstream compares boxed Integers by identity before comparing their values, so equal
        // ranks above 127 return Equal at once: see `crate::gwt::boxed_integers_differ`.
        if crate::gwt::boxed_integers_differ(left[a] as i64, right[b] as i64) {
            return left[a].cmp(&right[b]);
        }
    }
    // Preserve upstream's hasNext(), rather than silently correcting it to hasPrevious().
    if a == left.len() && b == right.len() {
        Ordering::Equal
    } else if a == left.len() {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}
