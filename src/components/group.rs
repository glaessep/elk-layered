//! `ComponentGroup` and `ComponentGroupGraphPlacer` from Eclipse ELK (EPL-2.0).
//!
//! ELK uses this placer when the graph has external ports, so that a component joined to the
//! west port is not placed east of a component joined to the east port.
//!
//! Source references (62d5909f96fad541bc101ad52dabaece6b7eab7e):
//! - `plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/components/ComponentGroup.java`
//! - `plugins/org.eclipse.elk.alg.layered/src/org/eclipse/elk/alg/layered/components/ComponentGroupGraphPlacer.java`
//!
//! Not ported, as the options that switch them on are refused: the compaction of components
//! (`compaction.connectedComponents`) and the model-order placers (`considerModelOrder.components`).

use super::{Component, offset_graph};
use crate::graph::{LPoint, LSize, PortSide};

// A set of port sides, as ELK's `PortSide.SIDES_*` sets. Bits: north 1, east 2, south 4, west 8.
// An undefined side gets its own bit. ELK's `EnumSet` holds it too, and the set then equals none
// of the `SIDES_*` sets.
const NORTH: u8 = 1;
const EAST: u8 = 2;
const SOUTH: u8 = 4;
const WEST: u8 = 8;
const UNDEFINED: u8 = 16;

pub(super) const SIDES_NONE: u8 = 0;
const SIDES_NORTH: u8 = NORTH;
const SIDES_EAST: u8 = EAST;
const SIDES_SOUTH: u8 = SOUTH;
const SIDES_WEST: u8 = WEST;
const SIDES_NORTH_SOUTH: u8 = NORTH | SOUTH;
const SIDES_EAST_WEST: u8 = EAST | WEST;
const SIDES_NORTH_WEST: u8 = NORTH | WEST;
const SIDES_NORTH_EAST: u8 = NORTH | EAST;
const SIDES_SOUTH_WEST: u8 = SOUTH | WEST;
const SIDES_EAST_SOUTH: u8 = EAST | SOUTH;
const SIDES_NORTH_EAST_WEST: u8 = NORTH | EAST | WEST;
const SIDES_EAST_SOUTH_WEST: u8 = EAST | SOUTH | WEST;
const SIDES_NORTH_SOUTH_WEST: u8 = NORTH | SOUTH | WEST;
const SIDES_NORTH_EAST_SOUTH: u8 = NORTH | EAST | SOUTH;
const SIDES_NORTH_EAST_SOUTH_WEST: u8 = NORTH | EAST | SOUTH | WEST;

/// The bit of a port side in a set of sides.
pub(super) fn side_bit(side: PortSide) -> u8 {
    match side {
        PortSide::North => NORTH,
        PortSide::East => EAST,
        PortSide::South => SOUTH,
        PortSide::West => WEST,
        PortSide::Undefined => UNDEFINED,
    }
}

/// Which sets of external port connections cannot share a group, ComponentGroup.CONSTRAINTS: a
/// component with the first set rules out any group that holds a component with the second.
const CONSTRAINTS: &[(u8, u8)] = &[
    (SIDES_NONE, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_EAST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_EAST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH, SIDES_NORTH_EAST_WEST),
    (SIDES_SOUTH, SIDES_EAST_SOUTH_WEST),
    (SIDES_SOUTH, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_SOUTH, SIDES_EAST_WEST),
    (SIDES_NORTH_SOUTH, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_SOUTH, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_SOUTH, SIDES_EAST_SOUTH_WEST),
    (SIDES_EAST_WEST, SIDES_NORTH_SOUTH),
    (SIDES_EAST_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_EAST_WEST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_EAST_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_WEST, SIDES_NORTH_WEST),
    (SIDES_NORTH_WEST, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_NORTH_EAST, SIDES_NORTH_EAST),
    (SIDES_NORTH_EAST, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_EAST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_SOUTH_WEST, SIDES_SOUTH_WEST),
    (SIDES_SOUTH_WEST, SIDES_EAST_SOUTH_WEST),
    (SIDES_SOUTH_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_EAST_SOUTH, SIDES_EAST_SOUTH),
    (SIDES_EAST_SOUTH, SIDES_EAST_SOUTH_WEST),
    (SIDES_EAST_SOUTH, SIDES_NORTH_EAST_SOUTH),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_SOUTH),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_WEST),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_EAST),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_NORTH_EAST_WEST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_EAST_SOUTH_WEST, SIDES_SOUTH),
    (SIDES_EAST_SOUTH_WEST, SIDES_NORTH_SOUTH),
    (SIDES_EAST_SOUTH_WEST, SIDES_SOUTH_WEST),
    (SIDES_EAST_SOUTH_WEST, SIDES_EAST_SOUTH),
    (SIDES_EAST_SOUTH_WEST, SIDES_EAST_SOUTH_WEST),
    (SIDES_EAST_SOUTH_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_EAST_SOUTH_WEST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_EAST_SOUTH_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_EAST_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_NORTH_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_SOUTH_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_EAST_SOUTH_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_NORTH_SOUTH_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_EAST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_EAST_WEST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_NORTH_EAST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_EAST_SOUTH),
    (SIDES_NORTH_EAST_SOUTH, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_EAST_SOUTH_WEST),
    (SIDES_NORTH_EAST_SOUTH, SIDES_NORTH_EAST_SOUTH),
    (SIDES_NORTH_EAST_SOUTH, SIDES_NORTH_EAST_SOUTH_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NONE),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_EAST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_SOUTH),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_SOUTH),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_EAST_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_EAST_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_EAST_SOUTH_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_SOUTH_WEST),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_EAST_SOUTH),
    (SIDES_NORTH_EAST_SOUTH_WEST, SIDES_NORTH_EAST_SOUTH_WEST),
];

/// Components that can be placed together, ComponentGroup. A component joins the first group that
/// holds none that rules it out.
#[derive(Default)]
struct ComponentGroup {
    /// The members in the order they joined, each with its set of connected sides: ELK's
    /// `ArrayListMultimap`, whose lists keep the order of `put`.
    members: Vec<(u8, usize)>,
}

impl ComponentGroup {
    /// ComponentGroup.add: adds the component unless a member rules it out.
    fn add(&mut self, sides: u8, component: usize) -> bool {
        if !self.can_add(sides) {
            return false;
        }
        self.members.push((sides, component));
        true
    }

    /// ComponentGroup.canAdd.
    fn can_add(&self, sides: u8) -> bool {
        CONSTRAINTS
            .iter()
            .filter(|(candidate, _)| *candidate == sides)
            .all(|(_, constraint)| self.members.iter().all(|(held, _)| held != constraint))
    }

    /// ComponentGroup.getComponents(Set): the members with exactly these sides.
    fn with_sides(&self, sides: u8) -> Vec<usize> {
        self.members
            .iter()
            .filter(|(held, _)| *held == sides)
            .map(|&(_, component)| component)
            .collect()
    }
}

/// ComponentGroupGraphPlacer.combine, up to the move of the nodes into the target graph:
/// places the components and returns the size of the target.
///
/// Every component gets its place by `offset_graph`, which leaves the graph's own offset alone.
/// The caller applies that offset afterwards, as `AbstractGraphPlacer.moveGraph` does.
pub(super) fn place(components: &mut [Component], spacing: f64, aspect_ratio: f64) -> LSize {
    if components.is_empty() {
        return LSize::default();
    }
    // Construct component groups, each component in the first group that can hold it.
    let mut groups: Vec<ComponentGroup> = Vec::new();
    for (index, component) in components.iter().enumerate() {
        let sides = component.ext_port_sides;
        if !groups.iter_mut().any(|group| group.add(sides, index)) {
            let mut group = ComponentGroup::default();
            group.add(sides, index);
            groups.push(group);
        }
    }
    // Place the components of each group; the groups follow each other along a diagonal.
    let (mut offset_x, mut offset_y) = (0.0f64, 0.0f64);
    for group in &groups {
        let (width, height) = place_group(components, group, spacing, aspect_ratio);
        for &(_, component) in &group.members {
            offset_graph(
                &mut components[component].graph,
                LPoint {
                    x: offset_x,
                    y: offset_y,
                },
            );
        }
        offset_x += width;
        offset_y += height;
    }
    // The group sizes include spacing on the right and at the bottom.
    LSize {
        width: offset_x - spacing,
        height: offset_y - spacing,
    }
}

fn offset_all(components: &mut [Component], indices: &[usize], x: f64, y: f64) {
    for &index in indices {
        offset_graph(&mut components[index].graph, LPoint { x, y });
    }
}

/// ComponentGroupGraphPlacer.placeComponents: the size of the group.
#[allow(clippy::too_many_lines)]
fn place_group(
    components: &mut [Component],
    group: &ComponentGroup,
    spacing: f64,
    aspect_ratio: f64,
) -> (f64, f64) {
    // Place the components of each set of sides on their own, as if no other component were in
    // the group. Each size has spacing on the right and at the bottom.
    let none = group.with_sides(SIDES_NONE);
    let n = group.with_sides(SIDES_NORTH);
    let s = group.with_sides(SIDES_SOUTH);
    let w = group.with_sides(SIDES_WEST);
    let e = group.with_sides(SIDES_EAST);
    let nw = group.with_sides(SIDES_NORTH_WEST);
    let ne = group.with_sides(SIDES_NORTH_EAST);
    let sw = group.with_sides(SIDES_SOUTH_WEST);
    let se = group.with_sides(SIDES_EAST_SOUTH);
    let we = group.with_sides(SIDES_EAST_WEST);
    let ns = group.with_sides(SIDES_NORTH_SOUTH);
    let nwe = group.with_sides(SIDES_NORTH_EAST_WEST);
    let swe = group.with_sides(SIDES_EAST_SOUTH_WEST);
    let wns = group.with_sides(SIDES_NORTH_SOUTH_WEST);
    let ens = group.with_sides(SIDES_NORTH_EAST_SOUTH);
    let nesw = group.with_sides(SIDES_NORTH_EAST_SOUTH_WEST);

    let size_c = place_in_rows(components, &none, spacing, aspect_ratio);
    let size_n = place_horizontally(components, &n, spacing);
    let size_s = place_horizontally(components, &s, spacing);
    let size_w = place_vertically(components, &w, spacing);
    let size_e = place_vertically(components, &e, spacing);
    let size_nw = place_horizontally(components, &nw, spacing);
    let size_ne = place_horizontally(components, &ne, spacing);
    let size_sw = place_horizontally(components, &sw, spacing);
    let size_se = place_horizontally(components, &se, spacing);
    let size_we = place_vertically(components, &we, spacing);
    let size_ns = place_horizontally(components, &ns, spacing);
    let size_nwe = place_horizontally(components, &nwe, spacing);
    let size_swe = place_horizontally(components, &swe, spacing);
    let size_wns = place_vertically(components, &wns, spacing);
    let size_ens = place_vertically(components, &ens, spacing);
    let size_nesw = place_horizontally(components, &nesw, spacing);

    // The widest column and the highest row of the grid. ELK adds a fourth row for the east-west
    // components and a fourth column for the north-south ones.
    let col_left = max4(size_nw.0, size_w.0, size_sw.0, size_wns.0);
    let col_mid = max4(size_n.0, size_c.0, size_s.0, size_nesw.0);
    let col_ns = size_ns.0;
    let col_right = max4(size_ne.0, size_e.0, size_se.0, size_ens.0);
    let row_top = max4(size_nw.1, size_n.1, size_ne.1, size_nwe.1);
    let row_mid = max4(size_w.1, size_c.1, size_e.1, size_nesw.1);
    let row_we = size_we.1;
    let row_bottom = max4(size_sw.1, size_s.1, size_se.1, size_swe.1);

    // Move each set to its cell. The north-west, north-east-west and north-south-west sets stay
    // at (0, 0).
    offset_all(components, &none, col_left + col_ns, row_top + row_we);
    offset_all(components, &nesw, col_left + col_ns, row_top + row_we);
    offset_all(components, &n, col_left + col_ns, 0.0);
    offset_all(
        components,
        &s,
        col_left + col_ns,
        row_top + row_we + row_mid,
    );
    offset_all(components, &w, 0.0, row_top + row_we);
    offset_all(
        components,
        &e,
        col_left + col_ns + col_mid,
        row_top + row_we,
    );
    offset_all(components, &ne, col_left + col_ns + col_mid, 0.0);
    offset_all(components, &sw, 0.0, row_top + row_we + row_mid);
    offset_all(
        components,
        &se,
        col_left + col_ns + col_mid,
        row_top + row_we + row_mid,
    );
    offset_all(components, &we, 0.0, row_top);
    offset_all(components, &ns, col_left, 0.0);
    offset_all(components, &swe, 0.0, row_top + row_we + row_mid);
    offset_all(components, &ens, col_left + col_ns + col_mid, 0.0);

    let width = (col_left + col_mid + col_ns + col_right)
        .max(size_we.0)
        .max(size_nwe.0)
        .max(size_swe.0);
    let height = (row_top + row_mid + row_we + row_bottom)
        .max(size_ns.1)
        .max(size_wns.1)
        .max(size_ens.1);
    (width, height)
}

/// `ElkMath.maxd` of four numbers.
fn max4(a: f64, b: f64, c: f64, d: f64) -> f64 {
    a.max(b).max(c).max(d)
}

/// ComponentGroupGraphPlacer.placeComponentsHorizontally: a line from left to right. The size
/// holds spacing on the right and, when there is a component, at the bottom.
fn place_horizontally(components: &mut [Component], indices: &[usize], spacing: f64) -> (f64, f64) {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for &index in indices {
        let graph = &mut components[index].graph;
        offset_graph(graph, LPoint { x, y: 0.0 });
        x += graph.size.width + spacing;
        y = y.max(graph.size.height);
    }
    if y > 0.0 {
        y += spacing;
    }
    (x, y)
}

/// ComponentGroupGraphPlacer.placeComponentsVertically: a line from top to bottom.
fn place_vertically(components: &mut [Component], indices: &[usize], spacing: f64) -> (f64, f64) {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for &index in indices {
        let graph = &mut components[index].graph;
        offset_graph(graph, LPoint { x: 0.0, y });
        y += graph.size.height + spacing;
        x = x.max(graph.size.width);
    }
    if x > 0.0 {
        x += spacing;
    }
    (x, y)
}

/// ComponentGroupGraphPlacer.placeComponentsInRows: rows as `SimpleRowGraphPlacer` makes them.
fn place_in_rows(
    components: &mut [Component],
    indices: &[usize],
    spacing: f64,
    aspect_ratio: f64,
) -> (f64, f64) {
    if indices.is_empty() {
        return (0.0, 0.0);
    }
    let mut max_row_width = 0.0f64;
    let mut total_area = 0.0f64;
    for &index in indices {
        let size = components[index].graph.size;
        max_row_width = max_row_width.max(size.width);
        total_area += size.width * size.height;
    }
    // ELK takes the square root as a `float`.
    max_row_width = max_row_width.max(f64::from(total_area.sqrt() as f32) * aspect_ratio);
    let (mut x, mut y, mut highest, mut broadest) = (0.0f64, 0.0f64, 0.0f64, spacing);
    for &index in indices {
        let graph = &mut components[index].graph;
        let size = graph.size;
        if x + size.width > max_row_width {
            x = 0.0;
            y += highest + spacing;
            highest = 0.0;
        }
        offset_graph(graph, LPoint { x, y });
        broadest = broadest.max(x + size.width);
        highest = highest.max(size.height);
        x += size.width + spacing;
    }
    (broadest + spacing, y + highest + spacing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{LNode, NodeId};
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
            original_nodes: vec![NodeId::new(0)],
            original_edges: Vec::new(),
            priority: 0,
            ext_port_sides: sides,
        }
    }

    fn position(component: &Component) -> (f64, f64) {
        let at = component.graph.layerless_nodes[0].position;
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
}
