//! Stretch-width layering translated from Eclipse ELK, EPL-2.0:
//! https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p2layers/StretchWidthLayerer.java

use crate::graph::NodeId;
use super::{LayeringError, LayeringInput, LayeringResult};
use crate::graph::LNodeKind;
use std::cmp::Reverse;

/// Most times the adaptive loop may widen `max_width` and start over.
///
/// ELK has no such bound. Java's `StretchWidthLayerer` divides every size by the unclamped
/// minimum but starts `maxWidth` from the clamped one. A normal node much shorter than one unit
/// then leaves `maxWidth` far below the tallest node's size, and ELK adds 1 per retry until it
/// catches up. Heights `1e-15` and `1` need about 1e15 retries. Each retry walks every node, so a
/// layout that needs even a million of them does not finish in reasonable time in ELK either.
/// The bound turns that case into an error and changes no layering ELK completes.
const MAX_WIDTH_RETRIES: usize = 1_000_000;

pub(super) fn layer(input: &LayeringInput<'_>) -> LayeringResult<Vec<Vec<NodeId>>> {
    if input.nodes.is_empty() {
        return Ok(Vec::new());
    }
    let mut ranks = vec![0; input.len()];
    let mut incoming = vec![0; input.len()];
    let mut outgoing = vec![0; input.len()];
    let mut minimum = f64::INFINITY;
    let mut maximum = f64::NEG_INFINITY;
    let mut total_outgoing = 0.0;
    for &node in &input.nodes {
        outgoing[node.index()] = input.outgoing[node.index()].len();
        incoming[node.index()] = input.incoming[node.index()].len();
        ranks[node.index()] = input.incoming[node.index()]
            .iter()
            .map(|&edge| input.outgoing[input.source(edge).index()].len())
            .fold(outgoing[node.index()], usize::max);
        total_outgoing += outgoing[node.index()] as f64;
        let data = &input.graph.layerless_nodes[node];
        if data.kind == LNodeKind::Normal {
            minimum = minimum.min(data.size.height);
            maximum = maximum.max(data.size.height);
        }
    }
    let mut nodes = input.nodes.clone();
    nodes.sort_by_key(|&node| Reverse(ranks[node.index()]));
    // ELK normalizes nodes before clamping the minimum to one. A zero minimum together with a
    // positive height makes the adaptive retry loop infinite. Keep the all-zero case's NaN
    // comparisons (both Java and JavaScript treat them as false), but reject infinite sizes.
    let mut sizes = vec![0.0; input.len()];
    for &node in &nodes {
        sizes[node.index()] = input.graph.layerless_nodes[node].size.height / minimum;
        if sizes[node.index()].is_infinite() {
            return Err(LayeringError::InvalidStretchWidthDimensions);
        }
    }
    minimum = minimum.max(1.0);
    maximum = maximum.max(1.0);
    let dummy_size = input.graph.options.spacing.edge_edge / minimum;
    let mut max_width = maximum / minimum;
    let influence = total_outgoing / nodes.len() as f64;
    let mut remaining = nodes.clone();
    let mut remaining_outgoing = outgoing.clone();
    let mut layers: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut width_current = 0.0;
    let mut width_up = 0.0;
    let mut retries = 0;
    while !remaining.is_empty() {
        let selected = remaining
            .iter()
            .position(|&node| remaining_outgoing[node.index()] == 0);
        let go_up = selected.is_some_and(|index| {
            let node = remaining[index];
            width_current - outgoing[node.index()] as f64 * dummy_size + sizes[node.index()] > max_width
                || width_up + incoming[node.index()] as f64 * dummy_size
                    > max_width * influence * dummy_size
        });
        let current = layers.last_mut().expect("the initial layer exists");
        if selected.is_none() || (go_up && !current.is_empty()) {
            if current.is_empty() {
                return Err(LayeringError::CyclicGraph);
            }
            for &node in current.iter() {
                for &edge in &input.incoming[node.index()] {
                    remaining_outgoing[input.source(edge).index()] -= 1;
                }
            }
            layers.push(Vec::new());
            width_current = width_up;
            width_up = 0.0;
        } else if go_up {
            layers.clear();
            layers.push(Vec::new());
            width_current = 0.0;
            width_up = 0.0;
            let next = max_width + 1.0;
            if !next.is_finite() || next == max_width {
                return Err(LayeringError::InvalidStretchWidthDimensions);
            }
            max_width = next;
            retries += 1;
            if retries > MAX_WIDTH_RETRIES {
                return Err(LayeringError::StretchWidthRetryLimit {
                    limit: MAX_WIDTH_RETRIES,
                });
            }
            remaining.clone_from(&nodes);
            remaining_outgoing.clone_from(&outgoing);
        } else {
            let Some(selected) = selected else {
                return Err(LayeringError::CyclicGraph);
            };
            let node = remaining.remove(selected);
            current.push(node);
            width_current = width_current - outgoing[node.index()] as f64 * dummy_size + sizes[node.index()];
            width_up += incoming[node.index()] as f64 * dummy_size;
        }
    }
    layers.reverse();
    Ok(layers)
}
