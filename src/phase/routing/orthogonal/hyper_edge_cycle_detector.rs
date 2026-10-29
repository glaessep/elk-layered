//! Port of ELK's `HyperEdgeCycleDetector`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/HyperEdgeCycleDetector.java

use std::collections::{BTreeSet, VecDeque};

use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::{
    DependencyType, HyperEdgeGraph,
};
use crate::random::JavaRandom;

pub fn detect_cycles(
    graph: &mut HyperEdgeGraph,
    critical_only: bool,
    random: &mut JavaRandom,
) -> Vec<usize> {
    let mut result = Vec::new();
    let mut sources = VecDeque::new();
    let mut sinks = VecDeque::new();

    initialize_cycle_detector(graph, &mut sources, &mut sinks, critical_only);
    compute_linear_ordering_marks(graph, &mut sources, &mut sinks, critical_only, random);

    for source in 0..graph.segments.len() {
        for dependency in graph.segments[source].outgoing_segment_dependencies.clone() {
            let dep = &graph.dependencies[dependency];
            if dep.source.is_none() || dep.target.is_none() {
                continue;
            }
            if (!critical_only || dep.dependency_type == DependencyType::Critical)
                && graph.segments[source].mark > graph.segments[dep.target.unwrap()].mark
            {
                result.push(dependency);
            }
        }
    }

    result
}

fn initialize_cycle_detector(
    graph: &mut HyperEdgeGraph,
    sources: &mut VecDeque<usize>,
    sinks: &mut VecDeque<usize>,
    critical_only: bool,
) {
    let mut next_mark = -1;
    for segment in 0..graph.segments.len() {
        graph.segments[segment].mark = next_mark;
        next_mark -= 1;

        let critical_in_weight = dependency_weight(
            graph,
            &graph.segments[segment].incoming_segment_dependencies,
            Some(DependencyType::Critical),
        );
        let critical_out_weight = dependency_weight(
            graph,
            &graph.segments[segment].outgoing_segment_dependencies,
            Some(DependencyType::Critical),
        );

        let mut in_weight = critical_in_weight;
        let mut out_weight = critical_out_weight;

        if !critical_only {
            in_weight = dependency_weight(
                graph,
                &graph.segments[segment].incoming_segment_dependencies,
                None,
            );
            out_weight = dependency_weight(
                graph,
                &graph.segments[segment].outgoing_segment_dependencies,
                None,
            );
        }

        graph.segments[segment].in_dep_weight = in_weight;
        graph.segments[segment].critical_in_dep_weight = critical_in_weight;
        graph.segments[segment].out_dep_weight = out_weight;
        graph.segments[segment].critical_out_dep_weight = critical_out_weight;

        if out_weight == 0 {
            sinks.push_back(segment);
        } else if in_weight == 0 {
            sources.push_back(segment);
        }
    }
}

fn compute_linear_ordering_marks(
    graph: &mut HyperEdgeGraph,
    sources: &mut VecDeque<usize>,
    sinks: &mut VecDeque<usize>,
    critical_only: bool,
    random: &mut JavaRandom,
) {
    let mut unprocessed = (0..graph.segments.len()).collect::<BTreeSet<_>>();
    let mut max_segments = Vec::new();
    let mark_base = graph.segments.len() as i32;
    let mut next_sink_mark = mark_base - 1;
    let mut next_source_mark = mark_base + 1;

    while !unprocessed.is_empty() {
        while let Some(sink) = sinks.pop_front() {
            unprocessed.remove(&sink);
            graph.segments[sink].mark = next_sink_mark;
            next_sink_mark -= 1;
            update_neighbors(graph, sink, sources, sinks, critical_only);
        }

        while let Some(source) = sources.pop_front() {
            unprocessed.remove(&source);
            graph.segments[source].mark = next_source_mark;
            next_source_mark += 1;
            update_neighbors(graph, source, sources, sinks, critical_only);
        }

        let mut max_outflow = i32::MIN;
        // ELK's TreeSet compares the initial marks (-1, -2, ...), so tied
        // candidates reach the seeded random choice in reverse segment order.
        for segment in unprocessed.iter().rev() {
            if !critical_only
                && graph.segments[*segment].critical_out_dep_weight > 0
                && graph.segments[*segment].critical_in_dep_weight <= 0
            {
                max_segments.clear();
                max_segments.push(*segment);
                break;
            }

            let outflow =
                graph.segments[*segment].out_dep_weight - graph.segments[*segment].in_dep_weight;
            if outflow >= max_outflow {
                if outflow > max_outflow {
                    max_segments.clear();
                    max_outflow = outflow;
                }
                max_segments.push(*segment);
            }
        }

        if !max_segments.is_empty() {
            let random_index = random.next_int(max_segments.len()).unwrap_or(0);
            let max_node = max_segments[random_index];
            unprocessed.remove(&max_node);
            graph.segments[max_node].mark = next_source_mark;
            next_source_mark += 1;
            update_neighbors(graph, max_node, sources, sinks, critical_only);
            max_segments.clear();
        }
    }

    let shift_base = graph.segments.len() as i32 + 1;
    for node in &mut graph.segments {
        if node.mark < mark_base {
            node.mark += shift_base;
        }
    }
}

fn update_neighbors(
    graph: &mut HyperEdgeGraph,
    node: usize,
    sources: &mut VecDeque<usize>,
    sinks: &mut VecDeque<usize>,
    critical_only: bool,
) {
    for dependency in graph.segments[node].outgoing_segment_dependencies.clone() {
        let dep = graph.dependencies[dependency].clone();
        if dep.source.is_none() || dep.target.is_none() {
            continue;
        }
        if !critical_only || dep.dependency_type == DependencyType::Critical {
            let target = dep.target.unwrap();
            if graph.segments[target].mark < 0 && dep.weight > 0 {
                graph.segments[target].in_dep_weight -= dep.weight;
                if dep.dependency_type == DependencyType::Critical {
                    graph.segments[target].critical_in_dep_weight -= dep.weight;
                }

                if graph.segments[target].in_dep_weight <= 0
                    && graph.segments[target].out_dep_weight > 0
                {
                    sources.push_back(target);
                }
            }
        }
    }

    for dependency in graph.segments[node].incoming_segment_dependencies.clone() {
        let dep = graph.dependencies[dependency].clone();
        if dep.source.is_none() || dep.target.is_none() {
            continue;
        }
        if !critical_only || dep.dependency_type == DependencyType::Critical {
            let source = dep.source.unwrap();
            if graph.segments[source].mark < 0 && dep.weight > 0 {
                graph.segments[source].out_dep_weight -= dep.weight;
                if dep.dependency_type == DependencyType::Critical {
                    graph.segments[source].critical_out_dep_weight -= dep.weight;
                }

                if graph.segments[source].out_dep_weight <= 0
                    && graph.segments[source].in_dep_weight > 0
                {
                    sinks.push_back(source);
                }
            }
        }
    }
}

fn dependency_weight(
    graph: &HyperEdgeGraph,
    dependencies: &[usize],
    dependency_type: Option<DependencyType>,
) -> i32 {
    dependencies
        .iter()
        .filter_map(|dependency| graph.dependencies.get(*dependency))
        .filter(|dependency| dependency.source.is_some() && dependency.target.is_some())
        .filter(|dependency| {
            dependency_type
                .map(|kind| dependency.dependency_type == kind)
                .unwrap_or(true)
        })
        .map(|dependency| dependency.weight)
        .sum()
}
