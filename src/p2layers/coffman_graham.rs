//! Coffman-Graham layering translated from Eclipse ELK, EPL-2.0:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/CoffmanGrahamLayerer.java
//!
//! Priority-queue ties follow the GWT `java.util.PriorityQueue` emitted in elkjs 0.9.3.
//! Those ties are observable in node order, so a Rust heap's unspecified tie order is unsuitable.

use crate::graph::NodeId;
use super::{LayeringError, LayeringInput, LayeringResult};
use std::cmp::Ordering;

pub(super) fn layer(input: &LayeringInput<'_>) -> LayeringResult<Vec<Vec<NodeId>>> {
    if input.nodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut marked_edges = vec![false; input.graph.edges.len()];
    let mut visited = vec![false; input.len()];
    let mut active = vec![false; input.len()];
    let mut stack = Vec::new();
    // Source transitive reduction marks edges; it does not remove them from the graph.
    for &start in &input.nodes {
        visited.fill(false);
        for &edge in &input.outgoing[start.index()] {
            let root = input.target(edge);
            if visited[root.index()] {
                continue;
            }
            active[root.index()] = true;
            stack.push((root, 0usize));
            while let Some((node, cursor)) = stack.last_mut() {
                if let Some(&edge) = input.outgoing[(*node).index()].get(*cursor) {
                    *cursor += 1;
                    let next = input.target(edge);
                    for &transitive in &input.incoming[next.index()] {
                        if input.source(transitive) == start {
                            marked_edges[transitive] = true;
                        }
                    }
                    if active[next.index()] {
                        return Err(LayeringError::CyclicGraph);
                    }
                    if !visited[next.index()] {
                        active[next.index()] = true;
                        stack.push((next, 0));
                    }
                } else {
                    let (node, _) = stack.pop().expect("the DFS frame exists");
                    visited[node.index()] = true;
                    active[node.index()] = false;
                }
            }
        }
    }

    let mut incoming = vec![0usize; input.len()];
    let mut predecessors = vec![Vec::new(); input.len()];
    let mut topological = vec![0usize; input.len()];
    let mut sources = Vec::new();
    for &node in &input.nodes {
        incoming[node.index()] = input.incoming[node.index()]
            .iter()
            .filter(|&&edge| !marked_edges[edge])
            .count();
        if incoming[node.index()] == 0 {
            push(&mut sources, node, |a, b| {
                compare_topology(&predecessors[a.index()], &predecessors[b.index()])
            });
        }
    }
    let mut count = 0;
    while let Some(node) = pop(&mut sources, |a, b| {
        compare_topology(&predecessors[a.index()], &predecessors[b.index()])
    }) {
        topological[node.index()] = count;
        count += 1;
        for &edge in &input.outgoing[node.index()] {
            if marked_edges[edge] {
                continue;
            }
            let next = input.target(edge);
            incoming[next.index()] -= 1;
            predecessors[next.index()].push(topological[node.index()]);
            if incoming[next.index()] == 0 {
                push(&mut sources, next, |a, b| {
                    compare_topology(&predecessors[a.index()], &predecessors[b.index()])
                });
            }
        }
    }
    if count != input.nodes.len() {
        return Err(LayeringError::CyclicGraph);
    }

    let mut outgoing = vec![0usize; input.len()];
    let mut sinks = Vec::new();
    for &node in &input.nodes {
        outgoing[node.index()] = input.outgoing[node.index()]
            .iter()
            .filter(|&&edge| !marked_edges[edge])
            .count();
        if outgoing[node.index()] == 0 {
            push(&mut sinks, node, |a, b| topological[b.index()].cmp(&topological[a.index()]));
        }
    }
    let mut layers: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut assigned = vec![None; input.len()];
    let bound = input.graph.options.layering_coffman_graham_layer_bound;
    while let Some(node) = pop(&mut sinks, |a, b| topological[b.index()].cmp(&topological[a.index()])) {
        let mut layer = layers.len() - 1;
        if layers[layer].len() as i64 >= i64::from(bound)
            || input.outgoing[node.index()]
                .iter()
                .any(|&edge| assigned[input.target(edge).index()] == Some(layer))
        {
            layers.push(Vec::new());
            layer += 1;
        }
        assigned[node.index()] = Some(layer);
        layers[layer].push(node);
        for &edge in &input.incoming[node.index()] {
            if marked_edges[edge] {
                continue;
            }
            let source = input.source(edge);
            outgoing[source.index()] -= 1;
            if outgoing[source.index()] == 0 {
                push(&mut sinks, source, |a, b| {
                    topological[b.index()].cmp(&topological[a.index()])
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
        // Upstream compares boxed Integers by identity before comparing their values. GWT caches
        // only -128..127; equal larger ranks therefore return Equal immediately.
        if left[a] != right[b] || left[a] >= 128 {
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

fn push(heap: &mut Vec<NodeId>, value: NodeId, mut compare: impl FnMut(NodeId, NodeId) -> Ordering) {
    let mut cursor = heap.len();
    heap.push(value);
    while cursor > 0 {
        let parent = (cursor - 1) / 2;
        if compare(heap[parent], value) != Ordering::Greater {
            break;
        }
        heap[cursor] = heap[parent];
        cursor = parent;
    }
    heap[cursor] = value;
}

fn pop(heap: &mut Vec<NodeId>, mut compare: impl FnMut(NodeId, NodeId) -> Ordering) -> Option<NodeId> {
    let result = *heap.first()?;
    let value = heap.pop().expect("the heap is nonempty");
    if !heap.is_empty() {
        let mut cursor = 0;
        while cursor * 2 + 1 < heap.len() {
            let mut child = cursor * 2 + 1;
            if child + 1 < heap.len() && compare(heap[child + 1], heap[child]) == Ordering::Less {
                child += 1;
            }
            if compare(value, heap[child]) == Ordering::Less {
                break;
            }
            heap[cursor] = heap[child];
            cursor = child;
        }
        heap[cursor] = value;
    }
    Some(result)
}
