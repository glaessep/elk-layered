//! Port of ELK's `HyperEdgeSegmentDependency`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/HyperEdgeSegmentDependency.java

use crate::phase::routing::orthogonal::hyper_edge_segment::HyperEdgeSegment;
use crate::phase::routing::orthogonal::orthogonal_routing_generator::CRITICAL_DEPENDENCY_WEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyType {
    Regular,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperEdgeSegmentDependency {
    pub dependency_type: DependencyType,
    pub source: Option<usize>,
    pub target: Option<usize>,
    pub weight: i32,
}

/// The segments and dependencies of one routing run. Java links them by reference. Rust keeps
/// them in two lists and links them by index, so the code that changes a link is a method here.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HyperEdgeGraph {
    pub segments: Vec<HyperEdgeSegment>,
    pub dependencies: Vec<HyperEdgeSegmentDependency>,
}

impl HyperEdgeGraph {
    pub fn add_segment(&mut self, segment: HyperEdgeSegment) -> usize {
        let index = self.segments.len();
        self.segments.push(segment);
        index
    }

    pub fn add_regular_dependency(&mut self, source: usize, target: usize, weight: i32) -> usize {
        self.add_dependency(DependencyType::Regular, source, target, weight)
    }

    pub fn add_critical_dependency(&mut self, source: usize, target: usize) -> usize {
        self.add_dependency(
            DependencyType::Critical,
            source,
            target,
            CRITICAL_DEPENDENCY_WEIGHT,
        )
    }

    fn add_dependency(
        &mut self,
        dependency_type: DependencyType,
        source: usize,
        target: usize,
        weight: i32,
    ) -> usize {
        let index = self.dependencies.len();
        self.dependencies.push(HyperEdgeSegmentDependency {
            dependency_type,
            source: None,
            target: None,
            weight,
        });
        self.set_dependency_source(index, Some(source));
        self.set_dependency_target(index, Some(target));
        index
    }

    pub fn remove_dependency(&mut self, dependency: usize) {
        self.set_dependency_source(dependency, None);
        self.set_dependency_target(dependency, None);
    }

    pub fn reverse_dependency(&mut self, dependency: usize) {
        let source = self.dependencies[dependency].source;
        let target = self.dependencies[dependency].target;
        self.set_dependency_source(dependency, target);
        self.set_dependency_target(dependency, source);
    }

    fn set_dependency_source(&mut self, dependency: usize, new_source: Option<usize>) {
        if let Some(source) = self.dependencies[dependency].source {
            remove_value(
                &mut self.segments[source].outgoing_segment_dependencies,
                dependency,
            );
        }

        self.dependencies[dependency].source = new_source;

        if let Some(source) = new_source {
            self.segments[source]
                .outgoing_segment_dependencies
                .push(dependency);
        }
    }

    fn set_dependency_target(&mut self, dependency: usize, new_target: Option<usize>) {
        if let Some(target) = self.dependencies[dependency].target {
            remove_value(
                &mut self.segments[target].incoming_segment_dependencies,
                dependency,
            );
        }

        self.dependencies[dependency].target = new_target;

        if let Some(target) = new_target {
            self.segments[target]
                .incoming_segment_dependencies
                .push(dependency);
        }
    }
}

fn remove_value(values: &mut Vec<usize>, value: usize) {
    if let Some(index) = values.iter().position(|candidate| *candidate == value) {
        values.remove(index);
    }
}
