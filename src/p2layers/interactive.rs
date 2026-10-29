//! Interactive layering translated from Eclipse ELK, EPL-2.0:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/InteractiveLayerer.java

use crate::graph::NodeId;
use super::{LayeringInput, LayeringResult};
use std::collections::VecDeque;

struct Span {
    start: f64,
    end: f64,
    nodes: Vec<NodeId>,
}

pub(super) fn layer(input: &LayeringInput<'_>) -> LayeringResult<Vec<Vec<NodeId>>> {
    // Check the same DAG precondition as the other layerers before the shift-until-stable loop.
    super::longest_path::layer(input, true)?;
    let mut spans: Vec<Span> = Vec::new();
    for &node in &input.nodes {
        let data = &input.graph.layerless_nodes[node];
        let start = data.position.x;
        let end = (start + data.size.width).max(start + 1.0);
        let mut found = None;
        let mut cursor = 0;
        while cursor < spans.len() {
            if spans[cursor].start >= end {
                break;
            }
            if spans[cursor].end > start {
                if let Some(previous) = found {
                    let removed = spans.remove(cursor);
                    let span: &mut Span = &mut spans[previous];
                    span.nodes.extend(removed.nodes);
                    span.end = span.end.max(removed.end);
                    continue;
                }
                let span = &mut spans[cursor];
                span.nodes.push(node);
                span.start = span.start.min(start);
                span.end = span.end.max(end);
                found = Some(cursor);
            }
            cursor += 1;
        }
        if found.is_none() {
            spans.insert(
                cursor,
                Span {
                    start,
                    end,
                    nodes: vec![node],
                },
            );
        }
    }
    let mut layers = spans.into_iter().map(|span| span.nodes).collect::<Vec<_>>();
    let mut assigned = vec![0; input.len()];
    for (index, nodes) in layers.iter().enumerate() {
        for &node in nodes {
            assigned[node.index()] = index;
        }
    }
    let mut checked = vec![false; input.len()];
    let mut queued = vec![false; input.len()];
    let mut queue = VecDeque::new();
    for &root in &input.nodes {
        if checked[root.index()] {
            continue;
        }
        queue.push_back(root);
        queued[root.index()] = true;
        while let Some(node) = queue.pop_front() {
            queued[node.index()] = false;
            checked[node.index()] = true;
            let layer = assigned[node.index()];
            for &edge in &input.outgoing[node.index()] {
                let next = input.target(edge);
                if next == node || assigned[next.index()] > layer {
                    continue;
                }
                layers[assigned[next.index()]].retain(|&entry| entry != next);
                if layers.len() == layer + 1 {
                    layers.push(Vec::new());
                }
                layers[layer + 1].push(next);
                assigned[next.index()] = layer + 1;
                if !queued[next.index()] {
                    queue.push_back(next);
                    queued[next.index()] = true;
                }
            }
        }
    }
    layers.retain(|layer| !layer.is_empty());
    Ok(layers)
}
