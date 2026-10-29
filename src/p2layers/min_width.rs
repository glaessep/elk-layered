//! Minimum-width layering translated from Eclipse ELK, EPL-2.0:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/MinWidthLayerer.java

use super::{LayeringError, LayeringInput, LayeringResult};
use crate::graph::{IdVec, LNodeKind, NodeId};
use std::cmp::Reverse;

pub(super) fn layer(input: &LayeringInput<'_>) -> LayeringResult<Vec<Vec<NodeId>>> {
    if input.nodes.is_empty() {
        return Ok(Vec::new());
    }
    let minimum = input
        .nodes
        .iter()
        .filter_map(|&node| {
            let data = &input.graph.layerless_nodes[node];
            (data.kind() == LNodeKind::Normal).then_some(data.size.height)
        })
        .fold(f64::INFINITY, f64::min)
        .max(1.0);
    let mut incoming = IdVec::filled(input.len(), 0);
    let mut outgoing = IdVec::filled(input.len(), 0);
    let mut sizes = IdVec::filled(input.len(), 0.0);
    let mut average = 0.0;
    for &node in &input.nodes {
        incoming[node] = input.incoming[node]
            .iter()
            .filter(|&&edge| input.source(edge) != node)
            .count();
        outgoing[node] = input.outgoing[node]
            .iter()
            .filter(|&&edge| input.target(edge) != node)
            .count();
        sizes[node] = input.graph.layerless_nodes[node].size.height / minimum;
        average += sizes[node];
    }
    average /= input.nodes.len() as f64;
    let dummy_size = input.graph.options.spacing.edge_edge / minimum;
    let mut nodes = input.nodes.clone();
    nodes.sort_by_key(|&node| Reverse(outgoing[node]));
    let width = input.graph.options.layering_min_width_upper_bound;
    let compensator = input
        .graph
        .options
        .layering_min_width_upper_layer_estimation_scaling_factor;
    let widths = if width < 0 { 1..=4 } else { width..=width };
    let compensators = if compensator < 0 {
        1..=2
    } else {
        compensator..=compensator
    };
    let mut best_width = f64::INFINITY;
    let mut best_layers = Vec::new();
    let mut best_count = usize::MAX;
    for width in widths {
        for compensator in compensators.clone() {
            let (candidate_width, candidate) = attempt(
                input,
                &nodes,
                &incoming,
                &outgoing,
                &sizes,
                dummy_size,
                average * f64::from(width),
                f64::from(compensator),
            )?;
            if candidate_width < best_width
                || (candidate_width == best_width && candidate.len() < best_count)
            {
                best_width = candidate_width;
                best_count = candidate.len();
                best_layers = candidate;
            }
        }
    }
    best_layers.reverse();
    Ok(best_layers)
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    input: &LayeringInput<'_>,
    nodes: &[NodeId],
    incoming: &IdVec<NodeId, usize>,
    outgoing: &IdVec<NodeId, usize>,
    sizes: &IdVec<NodeId, f64>,
    dummy_size: f64,
    width_bound: f64,
    compensator: f64,
) -> LayeringResult<(f64, Vec<Vec<NodeId>>)> {
    let mut remaining = nodes.to_vec();
    let mut placed = IdVec::filled(input.len(), false);
    let mut layers = Vec::new();
    let mut current = Vec::new();
    let mut width_current = 0.0;
    let mut width_up = 0.0;
    let mut max_width: f64 = 0.0;
    let mut real_width = 0.0;
    let mut spanning_edges = 0.0;
    let mut going_out = 0.0;
    while !remaining.is_empty() {
        let mut selected = None;
        for (index, &node) in remaining.iter().enumerate() {
            if input.outgoing[node]
                .iter()
                .all(|&edge| input.target(edge) == node || placed[input.target(edge)])
            {
                selected = Some(index);
                break;
            }
        }
        let node = if let Some(index) = selected {
            let node = remaining.remove(index);
            current.push(node);
            width_current += sizes[node] - outgoing[node] as f64 * dummy_size;
            width_up += incoming[node] as f64 * dummy_size;
            going_out += outgoing[node] as f64 * dummy_size;
            real_width += sizes[node];
            Some(node)
        } else {
            None
        };
        if node.is_none() && current.is_empty() {
            return Err(LayeringError::CyclicGraph);
        }
        if node.is_none()
            || remaining.is_empty()
            || node.is_some_and(|node| {
                width_current >= width_bound && sizes[node] > outgoing[node] as f64 * dummy_size
            })
            || width_up >= compensator * width_bound
        {
            for &node in &current {
                placed[node] = true;
            }
            layers.push(std::mem::take(&mut current));
            spanning_edges -= going_out;
            // Preserve the source's second dummy-size multiplication in its candidate score.
            max_width = max_width.max(spanning_edges * dummy_size + real_width);
            spanning_edges += width_up;
            width_current = width_up;
            width_up = 0.0;
            going_out = 0.0;
            real_width = 0.0;
        }
    }
    Ok((max_width, layers))
}
