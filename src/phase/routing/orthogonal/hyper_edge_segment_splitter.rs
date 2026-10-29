//! Port of ELK's `HyperEdgeSegmentSplitter`.
//!
//! Source: https://github.com/eclipse-elk/elk/blob/62d5909f96fad541bc101ad52dabaece6b7eab7e/plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/p5edges/orthogonal/HyperEdgeSegmentSplitter.java

use crate::phase::routing::orthogonal::hyper_edge_segment::HyperEdgeSegment;
use crate::phase::routing::orthogonal::hyper_edge_segment_dependency::HyperEdgeGraph;
use crate::phase::routing::orthogonal::orthogonal_routing_generator::{
    OrthogonalRoutingThresholds, count_crossings,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FreeArea {
    start_position: f64,
    end_position: f64,
    size: f64,
}

impl FreeArea {
    pub(crate) fn new(start_position: f64, end_position: f64) -> Self {
        debug_assert!(end_position >= start_position);
        Self {
            start_position,
            end_position,
            size: end_position - start_position,
        }
    }

    fn center(&self) -> f64 {
        center(self.start_position, self.end_position)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AreaRating {
    pub(crate) dependencies: i32,
    pub(crate) crossings: i32,
}

pub fn split_segments(
    graph: &mut HyperEdgeGraph,
    dependencies_to_resolve: &[usize],
    thresholds: OrthogonalRoutingThresholds,
) {
    if dependencies_to_resolve.is_empty() {
        return;
    }

    let mut free_areas = find_free_areas(&graph.segments, thresholds.critical_conflict_threshold);
    let segments_to_split = decide_which_segments_to_split(graph, dependencies_to_resolve);

    let mut ordered_segments = segments_to_split;
    ordered_segments.sort_by(|left, right| {
        graph.segments[*left]
            .length()
            .total_cmp(&graph.segments[*right].length())
    });

    for segment in ordered_segments {
        split_segment(graph, segment, &mut free_areas, thresholds);
    }
}

fn find_free_areas(
    segments: &[HyperEdgeSegment],
    critical_conflict_threshold: f64,
) -> Vec<FreeArea> {
    let mut sorted_coordinates = segments
        .iter()
        .flat_map(|segment| {
            segment
                .incoming_connection_coordinates
                .iter()
                .chain(segment.outgoing_connection_coordinates.iter())
                .copied()
        })
        .collect::<Vec<_>>();
    sorted_coordinates.sort_by(|left, right| left.total_cmp(right));

    let mut free_areas = Vec::new();
    for window in sorted_coordinates.windows(2) {
        let start = window[0];
        let end = window[1];
        if end - start >= 2.0 * critical_conflict_threshold {
            free_areas.push(FreeArea::new(
                start + critical_conflict_threshold,
                end - critical_conflict_threshold,
            ));
        }
    }

    free_areas
}

fn decide_which_segments_to_split(
    graph: &mut HyperEdgeGraph,
    dependencies: &[usize],
) -> Vec<usize> {
    let mut segments_to_split = Vec::new();

    for dependency in dependencies {
        let Some(source_segment) = graph.dependencies[*dependency].source else {
            continue;
        };
        let Some(target_segment) = graph.dependencies[*dependency].target else {
            continue;
        };

        if segments_to_split.contains(&source_segment)
            || segments_to_split.contains(&target_segment)
        {
            continue;
        }

        let mut segment_to_split = source_segment;
        let mut segment_causing_split = target_segment;

        if graph.segments[source_segment].represents_hyperedge()
            && !graph.segments[target_segment].represents_hyperedge()
        {
            segment_to_split = target_segment;
            segment_causing_split = source_segment;
        }

        segments_to_split.push(segment_to_split);
        graph.segments[segment_to_split].split_by = Some(segment_causing_split);
    }

    segments_to_split
}

pub(crate) fn split_segment(
    graph: &mut HyperEdgeGraph,
    segment: usize,
    free_areas: &mut Vec<FreeArea>,
    thresholds: OrthogonalRoutingThresholds,
) {
    let split_position = compute_position_to_split_and_update_free_areas(
        graph,
        &graph.segments[segment],
        free_areas,
        thresholds.critical_conflict_threshold,
    );
    let split_partner = graph.split_segment_at(segment, split_position);
    update_dependencies_after_split(graph, segment, split_partner, thresholds);
}

fn update_dependencies_after_split(
    graph: &mut HyperEdgeGraph,
    segment: usize,
    split_partner: usize,
    thresholds: OrthogonalRoutingThresholds,
) {
    let Some(split_causing_segment) = graph.segments[segment].split_by else {
        return;
    };

    graph.add_critical_dependency(segment, split_causing_segment);
    graph.add_critical_dependency(split_causing_segment, split_partner);

    let current_segment_count = graph.segments.len();
    for other_segment in 0..current_segment_count {
        if other_segment == split_causing_segment
            || other_segment == segment
            || other_segment == split_partner
        {
            continue;
        }

        graph.create_dependency_if_necessary(other_segment, segment, thresholds);
        graph.create_dependency_if_necessary(other_segment, split_partner, thresholds);
    }
}

fn compute_position_to_split_and_update_free_areas(
    graph: &HyperEdgeGraph,
    segment: &HyperEdgeSegment,
    free_areas: &mut Vec<FreeArea>,
    critical_conflict_threshold: f64,
) -> f64 {
    let mut first_possible_area_index = None;
    let mut last_possible_area_index = None;

    for (index, area) in free_areas.iter().enumerate() {
        if area.start_position > segment.end_coordinate() {
            break;
        } else if area.end_position >= segment.start_coordinate() {
            first_possible_area_index.get_or_insert(index);
            last_possible_area_index = Some(index);
        }
    }

    let Some(first_index) = first_possible_area_index else {
        return center(segment.start_coordinate(), segment.end_coordinate());
    };
    let last_index = last_possible_area_index.unwrap_or(first_index);
    let best_area_index =
        choose_best_area_index(graph, segment, free_areas, first_index, last_index);
    let split_position = free_areas[best_area_index].center();
    use_area(free_areas, best_area_index, critical_conflict_threshold);
    split_position
}

fn choose_best_area_index(
    graph: &HyperEdgeGraph,
    segment: &HyperEdgeSegment,
    free_areas: &[FreeArea],
    from_index: usize,
    to_index: usize,
) -> usize {
    let mut best_area_index = from_index;

    if from_index < to_index {
        let (mut split_segment, mut split_partner) = segment.simulate_split();
        let mut best_area = free_areas[best_area_index];
        let mut best_rating = rate_area(
            graph,
            segment,
            &mut split_segment,
            &mut split_partner,
            best_area,
        );

        for (index, area) in free_areas
            .iter()
            .enumerate()
            .take(to_index + 1)
            .skip(from_index + 1)
        {
            let current_rating = rate_area(
                graph,
                segment,
                &mut split_segment,
                &mut split_partner,
                *area,
            );
            if is_better_area(*area, current_rating, best_area, best_rating) {
                best_area = *area;
                best_rating = current_rating;
                best_area_index = index;
            }
        }
    }

    best_area_index
}

pub(crate) fn rate_area(
    graph: &HyperEdgeGraph,
    segment: &HyperEdgeSegment,
    split_segment: &mut HyperEdgeSegment,
    split_partner: &mut HyperEdgeSegment,
    area: FreeArea,
) -> AreaRating {
    let area_center = area.center();

    // HyperEdgeSegmentSplitter.rateArea changes the connection lists only. It does not call
    // recomputeExtent, so both segments keep the extent that simulateSplit gave them: the split
    // segment spans its incoming coordinates, the partner its outgoing ones, without the centre.
    split_segment.outgoing_connection_coordinates.clear();
    split_segment
        .outgoing_connection_coordinates
        .push(area_center);

    split_partner.incoming_connection_coordinates.clear();
    split_partner
        .incoming_connection_coordinates
        .push(area_center);

    let mut rating = AreaRating::default();

    for dependency in &segment.incoming_segment_dependencies {
        let Some(other_segment) = graph.dependencies[*dependency].source else {
            continue;
        };
        let other_segment = &graph.segments[other_segment];
        update_considering_both_orderings(&mut rating, split_segment, other_segment);
        update_considering_both_orderings(&mut rating, split_partner, other_segment);
    }

    for dependency in &segment.outgoing_segment_dependencies {
        let Some(other_segment) = graph.dependencies[*dependency].target else {
            continue;
        };
        let other_segment = &graph.segments[other_segment];
        update_considering_both_orderings(&mut rating, split_segment, other_segment);
        update_considering_both_orderings(&mut rating, split_partner, other_segment);
    }

    rating.dependencies += 2;
    if let Some(split_by) = segment.split_by {
        let split_by_segment = &graph.segments[split_by];
        rating.crossings += count_crossings_for_single_ordering(split_segment, split_by_segment);
        rating.crossings += count_crossings_for_single_ordering(split_by_segment, split_partner);
    }

    rating
}

fn update_considering_both_orderings(
    rating: &mut AreaRating,
    left_candidate: &HyperEdgeSegment,
    right_candidate: &HyperEdgeSegment,
) {
    let crossings_left_right = count_crossings_for_single_ordering(left_candidate, right_candidate);
    let crossings_right_left = count_crossings_for_single_ordering(right_candidate, left_candidate);

    if crossings_left_right == crossings_right_left {
        if crossings_left_right > 0 {
            rating.dependencies += 2;
            rating.crossings += crossings_left_right;
        }
    } else {
        rating.dependencies += 1;
        rating.crossings += crossings_left_right.min(crossings_right_left);
    }
}

fn count_crossings_for_single_ordering(left: &HyperEdgeSegment, right: &HyperEdgeSegment) -> i32 {
    count_crossings(
        &left.outgoing_connection_coordinates,
        right.start_coordinate(),
        right.end_coordinate(),
    ) + count_crossings(
        &right.incoming_connection_coordinates,
        left.start_coordinate(),
        left.end_coordinate(),
    )
}

fn is_better_area(
    current_area: FreeArea,
    current_rating: AreaRating,
    best_area: FreeArea,
    best_rating: AreaRating,
) -> bool {
    if current_rating.crossings < best_rating.crossings {
        return true;
    }

    current_rating.crossings == best_rating.crossings
        && (current_rating.dependencies < best_rating.dependencies
            || (current_rating.dependencies == best_rating.dependencies
                && current_area.size > best_area.size))
}

fn use_area(
    free_areas: &mut Vec<FreeArea>,
    used_area_index: usize,
    critical_conflict_threshold: f64,
) {
    let old_area = free_areas.remove(used_area_index);

    if old_area.size / 2.0 >= critical_conflict_threshold {
        let mut insert_index = used_area_index;
        let old_area_center = old_area.center();

        let new_end_1 = old_area_center - critical_conflict_threshold;
        if old_area.start_position <= new_end_1 {
            free_areas.insert(
                insert_index,
                FreeArea::new(old_area.start_position, new_end_1),
            );
            insert_index += 1;
        }

        let new_start_2 = old_area_center + critical_conflict_threshold;
        if new_start_2 <= old_area.end_position {
            free_areas.insert(
                insert_index,
                FreeArea::new(new_start_2, old_area.end_position),
            );
        }
    }
}

fn center(first: f64, second: f64) -> f64 {
    (first + second) / 2.0
}
