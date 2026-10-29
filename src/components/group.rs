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
    // ELK casts the square root to `float`, which elkjs leaves a double: see
    // `crate::gwt::float_value`.
    max_row_width = max_row_width.max(crate::gwt::float_value(total_area.sqrt()) * aspect_ratio);
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
mod tests;
