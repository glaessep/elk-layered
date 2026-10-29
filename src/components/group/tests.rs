use super::*;
use crate::graph::{IdVec, LNode, NodeId};
use crate::options::LayeredOptions;

/// A component of one node at (0, 0) whose graph has this size and joins these sides.
fn component(sides: u8, width: f64, height: f64) -> Component {
    let mut graph = crate::graph::LGraph::new("c", LayeredOptions::default());
    graph.size = LSize { width, height };
    graph
        .layerless_nodes
        .push(LNode::new("n", width, height, None));
    Component {
        graph,
        original_nodes: IdVec::from(vec![NodeId::new(0)]),
        original_edges: IdVec::new(),
        priority: 0,
        ext_port_sides: sides,
    }
}

fn position(component: &Component) -> (f64, f64) {
    let at = component.graph.layerless_nodes[NodeId::new(0)].position;
    (at.x, at.y)
}

/// Places the components and returns where each one ended up and the size of the target.
fn placed(
    mut components: Vec<Component>,
    spacing: f64,
    aspect_ratio: f64,
) -> (Vec<(f64, f64)>, (f64, f64)) {
    let size = place(&mut components, spacing, aspect_ratio);
    (
        components.iter().map(position).collect(),
        (size.width, size.height),
    )
}

// Mutation: give `PortSide::Undefined` the bit of `North`.
#[test]
fn every_port_side_has_its_own_bit() {
    let bits = [
        PortSide::North,
        PortSide::East,
        PortSide::South,
        PortSide::West,
        PortSide::Undefined,
    ]
    .map(side_bit);
    assert_eq!(bits, [1, 2, 4, 8, 16]);
}

// Mutation: drop one row of the table, so there are 72.
#[test]
fn the_constraint_table_has_the_73_rows_of_elk() {
    assert_eq!(CONSTRAINTS.len(), 73);
}

// Mutation: let `can_add` check the sets with `any` over the constraints instead of `all`, or
// compare the held set with the candidate instead of the constraint.
#[test]
fn a_group_refuses_the_sets_that_elk_rules_out() {
    let refused = [
        (SIDES_NONE, SIDES_NORTH_EAST_SOUTH_WEST),
        (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NONE),
        (SIDES_WEST, SIDES_NORTH_SOUTH_WEST),
        (SIDES_NORTH_SOUTH, SIDES_EAST_WEST),
        (SIDES_EAST_WEST, SIDES_NORTH_SOUTH),
        // A set rules out a second component with the same set.
        (SIDES_NORTH_WEST, SIDES_NORTH_WEST),
        (SIDES_EAST_SOUTH, SIDES_EAST_SOUTH),
    ];
    for (held, candidate) in refused {
        let mut group = ComponentGroup::default();
        assert!(group.add(held, 0));
        assert!(
            !group.add(candidate, 1),
            "{held:#06b} rules out {candidate:#06b}"
        );
        assert_eq!(group.members.len(), 1);
    }
}

// Mutation: refuse a component when any member is present, or never refuse.
#[test]
fn a_group_takes_the_sets_that_elk_allows() {
    let allowed = [
        (SIDES_NONE, SIDES_NONE),
        (SIDES_NONE, SIDES_WEST),
        (SIDES_WEST, SIDES_EAST),
        (SIDES_WEST, SIDES_NORTH),
        (SIDES_NORTH_SOUTH, SIDES_NORTH),
        (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_WEST),
        // An undefined side is in none of the sets: it conflicts with nothing.
        (SIDES_NORTH_EAST_SOUTH_WEST, UNDEFINED),
        (UNDEFINED, SIDES_NORTH_EAST_SOUTH_WEST),
    ];
    for (held, candidate) in allowed {
        let mut group = ComponentGroup::default();
        assert!(group.add(held, 0));
        assert!(
            group.add(candidate, 1),
            "{held:#06b} allows {candidate:#06b}"
        );
    }
}

// Mutation: add a component to the last group that fits instead of the first.
#[test]
fn a_component_joins_the_first_group_that_holds_it() {
    // The second component rules out the first group, so it starts a second one. The third
    // fits both; it joins the first.
    let components = vec![
        component(SIDES_NONE, 10.0, 10.0),
        component(SIDES_NORTH_EAST_SOUTH_WEST, 10.0, 10.0),
        component(SIDES_NORTH_WEST, 6.0, 4.0),
    ];
    let (at, size) = placed(components, 5.0, 1.6);
    // Group one: the north-west component is 11 wide and 9 high with spacing, so the
    // none-component sits at (11, 9). The group is 26 wide and 24 high.
    assert_eq!(at, vec![(11.0, 9.0), (26.0, 24.0), (0.0, 0.0)]);
    assert_eq!(size, (36.0, 34.0));
}

// Mutation: round the square root of the area to `f32`, as Java on a JVM does.
#[test]
fn the_row_width_keeps_the_square_root_a_double() {
    // The area is 700. sqrt(700) * 3.4016802 is just below 90 as a double, which is where the
    // second component ends; as a `float`, it is just above. elkjs wraps the second component.
    let components = vec![
        component(SIDES_NONE, 30.0, 10.0),
        component(SIDES_NONE, 40.0, 10.0),
    ];
    let (at, _) = placed(components, 20.0, 3.401_680_2);
    assert_eq!(at, vec![(0.0, 0.0), (0.0, 30.0)]);
}

// Mutation: return a size of 0 for no component, or read the first component of none.
#[test]
fn no_component_makes_an_empty_graph() {
    let (at, size) = placed(Vec::new(), 20.0, 1.6);
    assert!(at.is_empty());
    assert_eq!(size, (0.0, 0.0));
}

// Mutation: size the graph by the component alone, `width` and `height`. The placer keeps
// at least the spacing of one row in a row of components with no port.
#[test]
fn a_lone_component_without_a_port_is_at_least_as_wide_as_the_spacing() {
    let (at, size) = placed(vec![component(SIDES_NONE, 5.0, 7.0)], 20.0, 1.6);
    assert_eq!(at, vec![(0.0, 0.0)]);
    assert_eq!(size, (20.0, 7.0));
    // A wider component sets the width itself.
    let (_, size) = placed(vec![component(SIDES_NONE, 50.0, 7.0)], 20.0, 1.6);
    assert_eq!(size, (50.0, 7.0));
}

// Mutation: in `place_group`, move the north-east cell by the width of the left column only,
// or swap two of the offsets of the cells.
#[test]
fn the_eight_outer_cells_of_the_grid() {
    let components = vec![
        component(SIDES_NORTH, 10.0, 4.0),
        component(SIDES_SOUTH, 12.0, 5.0),
        component(SIDES_WEST, 6.0, 20.0),
        component(SIDES_EAST, 7.0, 30.0),
        component(SIDES_NORTH_WEST, 3.0, 2.0),
        component(SIDES_NORTH_EAST, 5.0, 9.0),
        component(SIDES_SOUTH_WEST, 2.0, 8.0),
        component(SIDES_EAST_SOUTH, 4.0, 3.0),
    ];
    let (at, size) = placed(components, 1.0, 1.6);
    assert_eq!(
        at,
        vec![
            (7.0, 0.0),   // north: over the middle column
            (7.0, 41.0),  // south: below the top row and the middle row
            (0.0, 10.0),  // west: left column, below the top row
            (20.0, 10.0), // east: right of the middle column
            (0.0, 0.0),   // north-west
            (20.0, 0.0),  // north-east
            (0.0, 41.0),  // south-west
            (20.0, 41.0), // south-east
        ]
    );
    assert_eq!(size, (27.0, 49.0));
}

// Mutation: drop `row_we` from the offset of the north-south column, or the east-west row.
#[test]
fn the_east_west_row_and_a_north_south_column() {
    let (at, size) = placed(
        vec![
            component(SIDES_EAST_WEST, 8.0, 6.0),
            component(SIDES_NORTH, 10.0, 4.0),
            component(SIDES_SOUTH, 12.0, 5.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 5.0), (0.0, 0.0), (0.0, 12.0)]);
    assert_eq!(size, (12.0, 17.0));

    let (at, size) = placed(
        vec![
            component(SIDES_NORTH_SOUTH, 4.0, 9.0),
            component(SIDES_WEST, 6.0, 20.0),
            component(SIDES_EAST, 7.0, 30.0),
            component(SIDES_SOUTH, 12.0, 5.0),
            component(SIDES_NORTH_EAST, 5.0, 2.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(
        at,
        vec![
            (7.0, 0.0),
            (0.0, 3.0),
            (25.0, 3.0),
            (12.0, 34.0),
            (25.0, 0.0)
        ]
    );
    assert_eq!(size, (32.0, 39.0));
}

// Mutation: offset the north-east-south-west components like the middle ones, or leave out
// the offset of the east-south-west or north-east-south ones.
#[test]
fn the_components_that_join_three_or_four_sides() {
    let (at, size) = placed(
        vec![
            component(SIDES_NORTH_EAST_SOUTH_WEST, 10.0, 4.0),
            component(SIDES_NORTH_WEST, 3.0, 2.0),
            component(SIDES_EAST_SOUTH, 4.0, 3.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(4.0, 3.0), (0.0, 0.0), (15.0, 8.0)]);
    assert_eq!(size, (19.0, 11.0));

    let (at, size) = placed(
        vec![
            component(SIDES_NORTH_EAST_WEST, 10.0, 4.0),
            component(SIDES_EAST_SOUTH_WEST, 8.0, 3.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 0.0), (0.0, 5.0)]);
    assert_eq!(size, (10.0, 8.0));

    let (at, size) = placed(
        vec![
            component(SIDES_NORTH_SOUTH_WEST, 6.0, 20.0),
            component(SIDES_NORTH_EAST_SOUTH, 7.0, 30.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 0.0), (7.0, 0.0)]);
    assert_eq!(size, (14.0, 30.0));

    // Right of a north-south column: the column is 5 wide.
    let (at, size) = placed(
        vec![
            component(SIDES_NORTH_SOUTH, 4.0, 9.0),
            component(SIDES_NORTH_EAST_SOUTH, 7.0, 30.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 0.0), (5.0, 0.0)]);
    assert_eq!(size, (12.0, 30.0));

    // Below the top, the east-west row and the middle row.
    let (at, size) = placed(
        vec![
            component(SIDES_EAST_SOUTH_WEST, 8.0, 3.0),
            component(SIDES_EAST_WEST, 8.0, 6.0),
            component(SIDES_NORTH, 10.0, 4.0),
            component(SIDES_WEST, 6.0, 20.0),
        ],
        1.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 33.0), (0.0, 5.0), (7.0, 0.0), (0.0, 12.0)]);
    assert_eq!(size, (17.0, 36.0));
}

// Mutation: use `x + width >= max_row_width` to start a row, or drop the aspect ratio from
// the row width.
#[test]
fn components_without_a_port_fill_rows() {
    let three = vec![
        component(SIDES_NONE, 10.0, 10.0),
        component(SIDES_NONE, 10.0, 10.0),
        component(SIDES_NONE, 10.0, 10.0),
    ];
    let (at, size) = placed(three, 5.0, 1.6);
    // The row is as wide as sqrt(300) * 1.6 = 27.7: two fit, the third starts a row.
    assert_eq!(at, vec![(0.0, 0.0), (15.0, 0.0), (0.0, 15.0)]);
    assert_eq!(size, (25.0, 25.0));
    // A row of exactly the width of two components takes the second one: sqrt(400) = 20.
    let two = vec![
        component(SIDES_NONE, 10.0, 20.0),
        component(SIDES_NONE, 10.0, 20.0),
    ];
    let (at, _) = placed(two, 0.0, 1.0);
    assert_eq!(at, vec![(0.0, 0.0), (10.0, 0.0)]);
}

// Mutation: put the second group at the origin instead of after the first.
#[test]
fn groups_follow_each_other_along_a_diagonal() {
    let (at, size) = placed(
        vec![
            component(SIDES_NONE, 10.0, 10.0),
            component(SIDES_NORTH_EAST_SOUTH_WEST, 20.0, 8.0),
        ],
        5.0,
        1.6,
    );
    assert_eq!(at, vec![(0.0, 0.0), (15.0, 15.0)]);
    assert_eq!(size, (35.0, 23.0));
}
