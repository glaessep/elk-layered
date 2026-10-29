//! The expected sizes are what elkjs 0.9.3 gives a 10 x 10 leaf node with these options.

use super::*;
use crate::options::NodeLabelLocation as L;

fn label(width: f64, height: f64, location: Option<NodeLabelLocation>) -> CellLabel {
    CellLabel {
        size: LSize { width, height },
        placement: location.map(NodeLabelPlacement::from_location),
    }
}

fn node(
    constraints: SizeConstraints,
    options: SizeOptions,
    placement: NodeLabelLocation,
    labels: Vec<CellLabel>,
) -> CellNode {
    CellNode {
        size: LSize {
            width: 10.0,
            height: 10.0,
        },
        sizing: NodeSizing {
            constraints,
            options,
            minimum: LSize::default(),
        },
        placement: NodeLabelPlacement::from_location(placement),
        labels,
        node_labels_padding: ElkPadding::uniform(5.0),
        label_node_spacing: 5.0,
        label_label_spacing: 0.0,
        fixed_graph_size: false,
    }
}

fn sized(node: &CellNode) -> (f64, f64) {
    let size = process(node, true, false).size;
    (size.width, size.height)
}

fn positions(node: &CellNode, horizontal: bool) -> Vec<(f64, f64)> {
    process(node, horizontal, true)
        .label_positions
        .into_iter()
        .map(|p| p.map_or((f64::NAN, f64::NAN), |p| (p.x, p.y)))
        .collect()
}

const LABELS: SizeConstraints = SizeConstraints::NODE_LABELS;

// Mutation: drop the `symmetrical` branch of `grid_column_widths`.
#[test]
fn the_label_grid_is_symmetric_unless_asked_otherwise() {
    let title = || vec![label(160.0, 16.0, None)];
    let symmetric = node(LABELS, SizeOptions::default(), L::InsideTopLeft, title());
    assert_eq!(sized(&symmetric), (330.0, 42.0));
    let asymmetric = node(LABELS, SizeOptions::ASYMMETRICAL, L::InsideTopLeft, title());
    assert_eq!(sized(&asymmetric), (170.0, 26.0));
    assert_eq!(positions(&asymmetric, true), vec![(5.0, 5.0)]);
}

// Mutation: ignore `tabular` in `Cells::minimum_width`.
#[test]
fn tabular_labels_line_up_their_columns() {
    let labels = || {
        vec![
            label(100.0, 10.0, Some(L::InsideTopLeft)),
            label(50.0, 10.0, Some(L::InsideBottomCenter)),
        ]
    };
    let rows = node(LABELS, SizeOptions::default(), L::Fixed, labels());
    assert_eq!(sized(&rows), (210.0, 30.0));
    assert_eq!(positions(&rows, true), vec![(5.0, 5.0), (80.0, 15.0)]);
    let table = node(
        LABELS,
        SizeOptions::FORCE_TABULAR_NODE_LABELS,
        L::Fixed,
        labels(),
    );
    assert_eq!(sized(&table), (260.0, 30.0));
    assert_eq!(positions(&table, true), vec![(5.0, 5.0), (105.0, 15.0)]);
}

// Mutation: ignore `OUTSIDE_NODE_LABELS_OVERHANG` throughout this file.
#[test]
fn outside_labels_widen_the_node_unless_they_may_overhang() {
    let labels = || vec![label(200.0, 10.0, None)];
    let wide = node(
        LABELS,
        SizeOptions::default(),
        L::OutsideTopCenter,
        labels(),
    );
    assert_eq!(sized(&wide), (200.0, 0.0));
    assert_eq!(positions(&wide, true), vec![(0.0, -15.0)]);
    let overhang = node(
        LABELS,
        SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG,
        L::OutsideTopCenter,
        labels(),
    );
    assert_eq!(sized(&overhang), (0.0, 0.0));
    assert_eq!(positions(&overhang, true), vec![(-100.0, -15.0)]);
}

// Mutation: set the middle row's width flag too for `MINIMUM_SIZE_ACCOUNTS_FOR_PADDING`.
#[test]
fn a_minimum_for_the_client_area_counts_the_height_only() {
    let mut client = node(
        SizeConstraints::MINIMUM_SIZE,
        SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING,
        L::Fixed,
        vec![],
    );
    client.sizing.minimum = LSize {
        width: 100.0,
        height: 50.0,
    };
    // ELK sets the middle row's height flag twice and its width flag never.
    assert_eq!(sized(&client), (0.0, 60.0));
}

// Mutation: swap the two stacking modes in `add_label`.
#[test]
fn labels_stack_across_the_layout_direction() {
    let labels = || vec![label(30.0, 10.0, None), label(20.0, 12.0, None)];
    let horizontal = node(LABELS, SizeOptions::default(), L::InsideTopLeft, labels());
    assert_eq!(sized(&horizontal), (70.0, 54.0));
    assert_eq!(positions(&horizontal, true), vec![(5.0, 5.0), (5.0, 15.0)]);
    // A DOWN layout runs the cells on the transposed node, so the labels are 10 x 30 and
    // 12 x 20 here; elkjs then reports the node as 76 x 66.
    let mut vertical = node(
        LABELS,
        SizeOptions::default(),
        L::InsideTopLeft,
        vec![label(10.0, 30.0, None), label(12.0, 20.0, None)],
    );
    vertical.label_label_spacing = 3.0;
    let size = process(&vertical, false, false).size;
    assert_eq!((size.width, size.height), (66.0, 76.0));
}

// Mutation: skip `place_labels` in `process` and place no label.
#[test]
fn fixed_constraints_keep_the_size_and_still_place_labels() {
    let fixed = node(
        SizeConstraints::FIXED,
        SizeOptions::default(),
        L::InsideTopRight,
        vec![label(20.0, 10.0, None), label(20.0, 10.0, Some(L::Fixed))],
    );
    assert_eq!(sized(&fixed), (10.0, 10.0));
    let placed = positions(&fixed, true);
    assert_eq!(placed[0], (-15.0, 5.0));
    assert!(placed[1].0.is_nan(), "a fixed label keeps its own position");
}

// Mutation: leave the gap out of the top padding in `inside_node_label_padding`.
#[test]
fn inside_labels_reserve_padding_on_their_sides() {
    let mut title = node(
        LABELS,
        SizeOptions::default(),
        L::InsideTopLeft,
        vec![label(160.0, 16.0, None)],
    );
    let padding = inside_node_label_padding(&title);
    assert_eq!(
        (padding.top, padding.right, padding.bottom, padding.left),
        (21.0, 0.0, 0.0, 165.0)
    );
    title.label_label_spacing = 2.0;
    let padding = inside_node_label_padding(&title);
    assert_eq!((padding.top, padding.left), (25.0, 169.0));
    let outside = node(
        LABELS,
        SizeOptions::default(),
        L::OutsideBottomRight,
        vec![label(160.0, 16.0, None)],
    );
    let padding = inside_node_label_padding(&outside);
    assert_eq!(
        (padding.top, padding.right, padding.bottom, padding.left),
        (0.0, 0.0, 0.0, 0.0)
    );
    let bottom_right = node(
        LABELS,
        SizeOptions::default(),
        L::InsideBottomRight,
        vec![label(30.0, 8.0, None)],
    );
    let padding = inside_node_label_padding(&bottom_right);
    assert_eq!((padding.bottom, padding.right), (13.0, 35.0));
}
