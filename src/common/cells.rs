//! ELK's cell system for node labels and node sizes.
//!
//! This ports the part of `NodeLabelAndSizeCalculator` that a node without ports needs: the
//! cells that hold node labels, the node size they ask for, and where they put the labels.
//! The cells for ports and port labels exist, as in ELK, but stay empty. A node whose size
//! constraints include `PORTS` or `PORT_LABELS` is refused before it gets here.
//!
//! Source references, all under
//! `plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/nodespacing/`:
//! - `NodeLabelAndSizeCalculator.java`
//! - `cellsystem/{Cell,AtomicCell,LabelCell,ContainerCell,GridContainerCell,StripContainerCell}.java`
//! - `internal/NodeContext.java`, `internal/NodeLabelLocation.java`
//! - `internal/algorithm/{NodeLabelCellCreator,InsidePortLabelCellCreator,CellSystemConfigurator,
//!   NodeSizeCalculator,NodeLabelAndSizeUtilities,LabelPlacer}.java`

use crate::graph::{LPoint, LSize};
use crate::options::{
    ElkPadding, NodeLabelLocation, NodeLabelPlacement, NodeSizing, SizeConstraints, SizeOptions,
};

/// One node label as the cell system sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellLabel {
    pub(crate) size: LSize,
    /// The label's own `nodeLabels.placement`, which wins over the node's.
    pub(crate) placement: Option<NodeLabelPlacement>,
}

/// What `NodeContext` reads from a node and the graph around it.
#[derive(Debug, Clone)]
pub(crate) struct CellNode {
    pub(crate) size: LSize,
    pub(crate) sizing: NodeSizing,
    pub(crate) placement: NodeLabelPlacement,
    pub(crate) labels: Vec<CellLabel>,
    /// `nodeLabels.padding`, from the node's individual spacings or else its graph.
    pub(crate) node_labels_padding: ElkPadding,
    /// `spacing.labelNode`, likewise.
    pub(crate) label_node_spacing: f64,
    /// `spacing.labelLabel`, likewise.
    pub(crate) label_label_spacing: f64,
    /// `nodeSize.fixedGraphSize` of the graph the node is in.
    pub(crate) fixed_graph_size: bool,
}

/// The result of `NodeLabelAndSizeCalculator.process`.
#[derive(Debug, Clone)]
pub(crate) struct CellResult {
    /// The node size that was or would be applied.
    pub(crate) size: LSize,
    /// A new position for each label the cells placed, by label index. Labels whose location
    /// is fixed keep their position, so they get `None`.
    pub(crate) label_positions: Vec<Option<LPoint>>,
    /// The node padding, when the size options ask to compute it.
    pub(crate) padding: Option<ElkPadding>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strip {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VAlign {
    Top,
    Center,
    Bottom,
}

type CellId = usize;

#[derive(Debug, Clone)]
enum Kind {
    Atomic {
        minimum_content: LSize,
    },
    Label {
        horizontal_layout_mode: bool,
        horizontal: HAlign,
        vertical: VAlign,
        gap: f64,
        labels: Vec<usize>,
        minimum_content: LSize,
    },
    Strip {
        mode: Strip,
        symmetrical: bool,
        gap: f64,
        cells: [Option<CellId>; 3],
    },
    Grid {
        gap: f64,
        tabular: bool,
        symmetrical: bool,
        cells: [[Option<CellId>; 3]; 3],
        center_cell_minimum_size: Option<LSize>,
        only_center_cell_contributes: bool,
        center_cell_rect: Rect,
    },
}

#[derive(Debug, Clone)]
struct Cell {
    padding: ElkPadding,
    rect: Rect,
    contributes_to_minimum_width: bool,
    contributes_to_minimum_height: bool,
    kind: Kind,
}

fn zero_padding() -> ElkPadding {
    ElkPadding::uniform(0.0)
}

/// The cells of one node, in an arena.
struct Cells<'a> {
    cells: Vec<Cell>,
    label_sizes: &'a [CellLabel],
}

impl Cells<'_> {
    fn add(&mut self, kind: Kind) -> CellId {
        self.cells.push(Cell {
            padding: zero_padding(),
            rect: Rect::default(),
            contributes_to_minimum_width: false,
            contributes_to_minimum_height: false,
            kind,
        });
        self.cells.len() - 1
    }

    /// Source: `Cell.getMinimumWidth` and its overrides.
    fn minimum_width(&self, id: CellId) -> f64 {
        let cell = &self.cells[id];
        let padding = cell.padding;
        match &cell.kind {
            Kind::Atomic { minimum_content }
            | Kind::Label {
                minimum_content, ..
            } => minimum_content.width + padding.left + padding.right,
            Kind::Strip {
                mode,
                symmetrical,
                gap,
                cells,
            } => {
                let mut width = 0.0;
                if *mode == Strip::Vertical {
                    // `Arrays.stream(...).max().orElse(0)`.
                    let mut max: Option<f64> = None;
                    for child in cells.iter().flatten() {
                        if self.cells[*child].contributes_to_minimum_width {
                            let child_width = self.minimum_width(*child);
                            max = Some(max.map_or(child_width, |m| m.max(child_width)));
                        }
                    }
                    width = max.unwrap_or(0.0);
                } else {
                    let widths = self.strip_widths(cells, *symmetrical, true);
                    let mut active = 0;
                    for cell_width in widths {
                        if cell_width > 0.0 {
                            width += cell_width;
                            active += 1;
                        }
                    }
                    if active > 1 {
                        width += gap * (active - 1) as f64;
                    }
                }
                if width > 0.0 {
                    width + padding.left + padding.right
                } else {
                    0.0
                }
            }
            Kind::Grid {
                gap,
                tabular,
                symmetrical,
                cells,
                center_cell_minimum_size,
                only_center_cell_contributes,
                ..
            } => {
                let mut width = 0.0;
                if *only_center_cell_contributes {
                    if let Some(minimum) = center_cell_minimum_size {
                        width = minimum.width;
                    } else if let Some(center) = cells[1][1] {
                        width = self.minimum_width(center);
                    }
                } else if *tabular {
                    width = sum_with_gaps(
                        self.grid_column_widths(
                            cells,
                            None,
                            true,
                            *symmetrical,
                            *center_cell_minimum_size,
                        ),
                        *gap,
                    );
                } else {
                    for row in 0..3 {
                        width = f64::max(
                            width,
                            sum_with_gaps(
                                self.grid_column_widths(
                                    cells,
                                    Some(row),
                                    true,
                                    *symmetrical,
                                    *center_cell_minimum_size,
                                ),
                                *gap,
                            ),
                        );
                    }
                }
                if width > 0.0 {
                    width + padding.left + padding.right
                } else {
                    0.0
                }
            }
        }
    }

    /// Source: `Cell.getMinimumHeight` and its overrides.
    fn minimum_height(&self, id: CellId) -> f64 {
        let cell = &self.cells[id];
        let padding = cell.padding;
        match &cell.kind {
            Kind::Atomic { minimum_content }
            | Kind::Label {
                minimum_content, ..
            } => minimum_content.height + padding.top + padding.bottom,
            Kind::Strip {
                mode,
                symmetrical,
                gap,
                cells,
            } => {
                let mut height = 0.0;
                if *mode == Strip::Vertical {
                    let heights = self.strip_heights(cells, *symmetrical, true);
                    let mut active = 0;
                    for cell_height in heights {
                        if cell_height > 0.0 {
                            height += cell_height;
                            active += 1;
                        }
                    }
                    if active > 1 {
                        height += gap * (active - 1) as f64;
                    }
                } else {
                    let mut max: Option<f64> = None;
                    for child in cells.iter().flatten() {
                        if self.cells[*child].contributes_to_minimum_height {
                            let child_height = self.minimum_height(*child);
                            max = Some(max.map_or(child_height, |m| m.max(child_height)));
                        }
                    }
                    height = max.unwrap_or(0.0);
                }
                if height > 0.0 {
                    height + padding.top + padding.bottom
                } else {
                    0.0
                }
            }
            Kind::Grid {
                gap,
                symmetrical,
                cells,
                center_cell_minimum_size,
                only_center_cell_contributes,
                ..
            } => {
                let mut height = 0.0;
                if *only_center_cell_contributes {
                    if let Some(minimum) = center_cell_minimum_size {
                        height = minimum.height;
                    } else if let Some(center) = cells[1][1] {
                        height = self.minimum_height(center);
                    }
                } else {
                    height = sum_with_gaps(
                        self.grid_row_heights(cells, true, *symmetrical, *center_cell_minimum_size),
                        *gap,
                    );
                }
                if height > 0.0 {
                    height + padding.top + padding.bottom
                } else {
                    0.0
                }
            }
        }
    }

    /// Source: `ContainerCell.minWidthOfCell`.
    fn min_width_of_cell(&self, cell: Option<CellId>, respect_contribution_flag: bool) -> f64 {
        let Some(id) = cell else {
            return 0.0;
        };
        let data = &self.cells[id];
        if respect_contribution_flag && !data.contributes_to_minimum_width {
            return 0.0;
        }
        if let Kind::Atomic { minimum_content } = data.kind
            && minimum_content.width == 0.0
        {
            return 0.0;
        }
        self.minimum_width(id)
    }

    /// Source: `ContainerCell.minHeightOfCell`.
    fn min_height_of_cell(&self, cell: Option<CellId>, respect_contribution_flag: bool) -> f64 {
        let Some(id) = cell else {
            return 0.0;
        };
        let data = &self.cells[id];
        if respect_contribution_flag && !data.contributes_to_minimum_height {
            return 0.0;
        }
        if let Kind::Atomic { minimum_content } = data.kind
            && minimum_content.height == 0.0
        {
            return 0.0;
        }
        self.minimum_height(id)
    }

    /// Source: `StripContainerCell.minCellWidths`.
    fn strip_widths(
        &self,
        cells: &[Option<CellId>; 3],
        symmetrical: bool,
        respect: bool,
    ) -> [f64; 3] {
        let mut widths = [
            self.min_width_of_cell(cells[0], respect),
            self.min_width_of_cell(cells[1], respect),
            self.min_width_of_cell(cells[2], respect),
        ];
        if symmetrical {
            widths[0] = f64::max(widths[0], widths[2]);
            widths[2] = widths[0];
        }
        widths
    }

    /// Source: `StripContainerCell.minCellHeights`.
    fn strip_heights(
        &self,
        cells: &[Option<CellId>; 3],
        symmetrical: bool,
        respect: bool,
    ) -> [f64; 3] {
        let mut heights = [
            self.min_height_of_cell(cells[0], respect),
            self.min_height_of_cell(cells[1], respect),
            self.min_height_of_cell(cells[2], respect),
        ];
        if symmetrical {
            heights[0] = f64::max(heights[0], heights[2]);
            heights[2] = heights[0];
        }
        heights
    }

    /// Source: `GridContainerCell.minColumnWidths` and `minWidthOfColumn`.
    fn grid_column_widths(
        &self,
        cells: &[[Option<CellId>; 3]; 3],
        row: Option<usize>,
        respect: bool,
        symmetrical: bool,
        center_minimum: Option<LSize>,
    ) -> [f64; 3] {
        let column_width = |column: usize| {
            let mut max_min_width = 0.0;
            match row {
                None => {
                    for row_cells in cells {
                        max_min_width = f64::max(
                            max_min_width,
                            self.min_width_of_cell(row_cells[column], respect),
                        );
                    }
                }
                Some(row) => max_min_width = self.min_width_of_cell(cells[row][column], respect),
            }
            if column == 1
                && let Some(minimum) = center_minimum
            {
                max_min_width = f64::max(max_min_width, minimum.width);
            }
            max_min_width
        };
        let mut widths = [column_width(0), column_width(1), column_width(2)];
        if symmetrical {
            widths[0] = f64::max(widths[0], widths[2]);
            widths[2] = widths[0];
        }
        widths
    }

    /// Source: `GridContainerCell.minRowHeights` and `minHeightOfRow`.
    fn grid_row_heights(
        &self,
        cells: &[[Option<CellId>; 3]; 3],
        respect: bool,
        symmetrical: bool,
        center_minimum: Option<LSize>,
    ) -> [f64; 3] {
        let row_height = |row: usize| {
            let mut max_min_height = 0.0;
            for cell in cells[row] {
                max_min_height = f64::max(max_min_height, self.min_height_of_cell(cell, respect));
            }
            if row == 1
                && let Some(minimum) = center_minimum
            {
                max_min_height = f64::max(max_min_height, minimum.height);
            }
            max_min_height
        };
        let mut heights = [row_height(0), row_height(1), row_height(2)];
        if symmetrical {
            heights[0] = f64::max(heights[0], heights[2]);
            heights[2] = heights[0];
        }
        heights
    }

    fn apply_horizontal(&mut self, cell: Option<CellId>, x: f64, width: f64) {
        if let Some(id) = cell {
            self.cells[id].rect.x = x;
            self.cells[id].rect.width = width;
        }
    }

    fn apply_vertical(&mut self, cell: Option<CellId>, y: f64, height: f64) {
        if let Some(id) = cell {
            self.cells[id].rect.y = y;
            self.cells[id].rect.height = height;
        }
    }

    /// Source: `ContainerCell.layoutChildrenHorizontally` overrides. Atomic and label cells
    /// have no children.
    fn layout_children_horizontally(&mut self, id: CellId) {
        let rect = self.cells[id].rect;
        let padding = self.cells[id].padding;
        match self.cells[id].kind.clone() {
            Kind::Strip {
                mode,
                symmetrical,
                gap,
                cells,
            } => {
                if mode == Strip::Vertical {
                    let x = rect.x + padding.left;
                    let width = rect.width - padding.left - padding.right;
                    for child in cells {
                        self.apply_horizontal(child, x, width);
                    }
                } else {
                    let mut widths = self.strip_widths(&cells, symmetrical, false);
                    self.apply_horizontal(cells[0], rect.x + padding.left, widths[0]);
                    self.apply_horizontal(
                        cells[2],
                        rect.x + rect.width - padding.right - widths[2],
                        widths[2],
                    );
                    let mut free = rect.width - padding.left - padding.right;
                    if widths[0] > 0.0 {
                        free -= widths[0] + gap;
                        widths[0] += gap;
                    }
                    if widths[2] > 0.0 {
                        free -= widths[2] + gap;
                    }
                    widths[1] = f64::max(widths[1], free);
                    self.apply_horizontal(
                        cells[1],
                        rect.x + padding.left + widths[0] - (widths[1] - free) / 2.0,
                        widths[1],
                    );
                }
                for child in cells.into_iter().flatten() {
                    if matches!(
                        self.cells[child].kind,
                        Kind::Strip { .. } | Kind::Grid { .. }
                    ) {
                        self.layout_children_horizontally(child);
                    }
                }
            }
            Kind::Grid {
                gap,
                tabular,
                symmetrical,
                cells,
                center_cell_minimum_size,
                ..
            } => {
                if tabular {
                    let widths = self.grid_column_widths(
                        &cells,
                        None,
                        false,
                        symmetrical,
                        center_cell_minimum_size,
                    );
                    for row in 0..3 {
                        self.apply_widths_to_row(id, &cells, row, widths, gap, rect, padding);
                    }
                } else {
                    for row in 0..3 {
                        let widths = self.grid_column_widths(
                            &cells,
                            Some(row),
                            false,
                            symmetrical,
                            center_cell_minimum_size,
                        );
                        self.apply_widths_to_row(id, &cells, row, widths, gap, rect, padding);
                    }
                }
            }
            Kind::Atomic { .. } | Kind::Label { .. } => {}
        }
    }

    /// Source: `GridContainerCell.applyWidthsToRow`.
    #[allow(clippy::too_many_arguments)]
    fn apply_widths_to_row(
        &mut self,
        grid: CellId,
        cells: &[[Option<CellId>; 3]; 3],
        row: usize,
        mut widths: [f64; 3],
        gap: f64,
        rect: Rect,
        padding: ElkPadding,
    ) {
        // `applyWidthToColumn` sets the column on every row, so the last row wins. Port that.
        let apply_column = |this: &mut Self, column: usize, x: f64, width: f64| {
            for row_cells in cells {
                this.apply_horizontal(row_cells[column], x, width);
            }
        };
        apply_column(self, 0, rect.x + padding.left, widths[0]);
        apply_column(
            self,
            2,
            rect.x + rect.width - padding.right - widths[2],
            widths[2],
        );
        let mut free = rect.width - padding.left - padding.right;
        if widths[0] > 0.0 {
            widths[0] += gap;
            free -= widths[0];
        }
        if widths[2] > 0.0 {
            widths[2] += gap;
            free -= widths[2];
        }
        let center_width = f64::max(0.0, free);
        widths[1] = f64::max(widths[1], free);
        apply_column(
            self,
            1,
            rect.x + padding.left + widths[0] - (widths[1] - free) / 2.0,
            widths[1],
        );
        if row == 1
            && let Kind::Grid {
                center_cell_rect, ..
            } = &mut self.cells[grid].kind
        {
            center_cell_rect.width = center_width;
            center_cell_rect.x = rect.x + padding.left + (center_width - free) / 2.0;
        }
    }

    /// Source: `ContainerCell.layoutChildrenVertically` overrides.
    fn layout_children_vertically(&mut self, id: CellId) {
        let rect = self.cells[id].rect;
        let padding = self.cells[id].padding;
        match self.cells[id].kind.clone() {
            Kind::Strip {
                mode,
                symmetrical,
                gap,
                cells,
            } => {
                if mode == Strip::Vertical {
                    let mut heights = self.strip_heights(&cells, symmetrical, false);
                    self.apply_vertical(cells[0], rect.y + padding.top, heights[0]);
                    self.apply_vertical(
                        cells[2],
                        rect.y + rect.height - padding.bottom - heights[2],
                        heights[2],
                    );
                    let content_height = rect.height - padding.top - padding.bottom;
                    let mut free = content_height;
                    if heights[0] > 0.0 {
                        heights[0] += gap;
                        free -= heights[0];
                    }
                    if heights[2] > 0.0 {
                        free -= heights[2] + gap;
                    }
                    heights[1] = f64::max(heights[1], free);
                    self.apply_vertical(
                        cells[1],
                        rect.y + padding.top + heights[0] - (heights[1] - free) / 2.0,
                        heights[1],
                    );
                } else {
                    let y = rect.y + padding.top;
                    let height = rect.height - padding.top - padding.bottom;
                    for child in cells {
                        self.apply_vertical(child, y, height);
                    }
                }
                for child in cells.into_iter().flatten() {
                    if matches!(
                        self.cells[child].kind,
                        Kind::Strip { .. } | Kind::Grid { .. }
                    ) {
                        self.layout_children_vertically(child);
                    }
                }
            }
            Kind::Grid {
                gap,
                symmetrical,
                cells,
                center_cell_minimum_size,
                ..
            } => {
                let mut heights =
                    self.grid_row_heights(&cells, false, symmetrical, center_cell_minimum_size);
                let apply_row = |this: &mut Self, row: usize, y: f64, height: f64| {
                    for cell in cells[row] {
                        this.apply_vertical(cell, y, height);
                    }
                };
                apply_row(self, 0, rect.y + padding.top, heights[0]);
                apply_row(
                    self,
                    2,
                    rect.y + rect.height - padding.bottom - heights[2],
                    heights[2],
                );
                let mut free = rect.height - padding.top - padding.bottom;
                if heights[0] > 0.0 {
                    heights[0] += gap;
                    free -= heights[0];
                }
                if heights[2] > 0.0 {
                    heights[2] += gap;
                    free -= heights[2];
                }
                if let Kind::Grid {
                    center_cell_rect, ..
                } = &mut self.cells[id].kind
                {
                    center_cell_rect.height = f64::max(0.0, free);
                    center_cell_rect.y =
                        rect.y + padding.top + (center_cell_rect.height - free) / 2.0;
                }
                heights[1] = f64::max(heights[1], free);
                apply_row(
                    self,
                    1,
                    rect.y + padding.top + heights[0] - (heights[1] - free) / 2.0,
                    heights[1],
                );
            }
            Kind::Atomic { .. } | Kind::Label { .. } => {}
        }
    }

    /// Source: `LabelCell.addLabel`.
    fn add_label(&mut self, id: CellId, label: usize) {
        let label_size = self.label_sizes[label].size;
        if let Kind::Label {
            horizontal_layout_mode,
            gap,
            labels,
            minimum_content,
            ..
        } = &mut self.cells[id].kind
        {
            labels.push(label);
            if *horizontal_layout_mode {
                minimum_content.width = f64::max(minimum_content.width, label_size.width);
                minimum_content.height += label_size.height;
                if labels.len() > 1 {
                    minimum_content.height += *gap;
                }
            } else {
                minimum_content.width += label_size.width;
                minimum_content.height = f64::max(minimum_content.height, label_size.height);
                if labels.len() > 1 {
                    minimum_content.width += *gap;
                }
            }
        }
    }

    /// Source: `LabelCell.applyLabelLayout` and its two mode-specific halves.
    fn apply_label_layout(&self, id: CellId, positions: &mut [Option<LPoint>]) {
        let cell = &self.cells[id];
        let Kind::Label {
            horizontal_layout_mode,
            horizontal,
            vertical,
            gap,
            labels,
            minimum_content,
        } = &cell.kind
        else {
            return;
        };
        let rect = cell.rect;
        let padding = cell.padding;
        if *horizontal_layout_mode {
            let mut y = rect.y;
            if *vertical == VAlign::Center {
                y += (rect.height - minimum_content.height) / 2.0;
            } else if *vertical == VAlign::Bottom {
                y += rect.height - minimum_content.height;
            }
            for &label in labels {
                let size = self.label_sizes[label].size;
                let label_y = y;
                y += size.height + gap;
                let x = match horizontal {
                    HAlign::Left => rect.x + padding.left,
                    HAlign::Center => rect.x + padding.left + (rect.width - size.width) / 2.0,
                    HAlign::Right => rect.x + rect.width - padding.right - size.width,
                };
                positions[label] = Some(LPoint { x, y: label_y });
            }
        } else {
            let mut x = rect.x;
            if *horizontal == HAlign::Center {
                x += (rect.width - minimum_content.width) / 2.0;
            } else if *horizontal == HAlign::Right {
                x += rect.width - minimum_content.width;
            }
            for &label in labels {
                let size = self.label_sizes[label].size;
                let label_x = x;
                x += size.width + gap;
                let y = match vertical {
                    VAlign::Top => rect.y + padding.top,
                    VAlign::Center => rect.y + padding.top + (rect.height - size.height) / 2.0,
                    VAlign::Bottom => rect.y + rect.height - padding.bottom - size.height,
                };
                positions[label] = Some(LPoint { x: label_x, y });
            }
        }
    }
}

/// Source: `GridContainerCell.sumWithGaps`.
fn sum_with_gaps(values: [f64; 3], gap: f64) -> f64 {
    let mut sum = 0.0;
    let mut active = 0;
    for value in values {
        if value > 0.0 {
            sum += value;
            active += 1;
        }
    }
    if active > 1 {
        sum += gap * (active - 1) as f64;
    }
    sum
}

/// The alignments and grid cell of a location. Source: `NodeLabelLocation`'s fields.
fn location_cell(location: NodeLabelLocation) -> Option<(HAlign, VAlign, usize, usize)> {
    use NodeLabelLocation as L;
    Some(match location {
        L::OutsideTopLeft => (HAlign::Left, VAlign::Bottom, 0, 0),
        L::OutsideTopCenter => (HAlign::Center, VAlign::Bottom, 0, 1),
        L::OutsideTopRight => (HAlign::Right, VAlign::Bottom, 0, 2),
        L::OutsideBottomLeft => (HAlign::Left, VAlign::Top, 2, 0),
        L::OutsideBottomCenter => (HAlign::Center, VAlign::Top, 2, 1),
        L::OutsideBottomRight => (HAlign::Right, VAlign::Top, 2, 2),
        L::OutsideLeftTop => (HAlign::Right, VAlign::Top, 0, 0),
        L::OutsideLeftCenter => (HAlign::Right, VAlign::Center, 1, 0),
        L::OutsideLeftBottom => (HAlign::Right, VAlign::Bottom, 2, 0),
        L::OutsideRightTop => (HAlign::Left, VAlign::Top, 0, 2),
        L::OutsideRightCenter => (HAlign::Left, VAlign::Center, 1, 2),
        L::OutsideRightBottom => (HAlign::Left, VAlign::Bottom, 2, 2),
        L::InsideTopLeft => (HAlign::Left, VAlign::Top, 0, 0),
        L::InsideTopCenter => (HAlign::Center, VAlign::Top, 0, 1),
        L::InsideTopRight => (HAlign::Right, VAlign::Top, 0, 2),
        L::InsideCenterLeft => (HAlign::Left, VAlign::Center, 1, 0),
        L::InsideCenter => (HAlign::Center, VAlign::Center, 1, 1),
        L::InsideCenterRight => (HAlign::Right, VAlign::Center, 1, 2),
        L::InsideBottomLeft => (HAlign::Left, VAlign::Bottom, 2, 0),
        L::InsideBottomCenter => (HAlign::Center, VAlign::Bottom, 2, 1),
        L::InsideBottomRight => (HAlign::Right, VAlign::Bottom, 2, 2),
        L::Fixed => return None,
    })
}

/// The side an outside location belongs to. Source: `NodeLabelLocation.getOutsideSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    North,
    East,
    South,
    West,
}

fn outside_side(location: NodeLabelLocation) -> Option<Side> {
    use NodeLabelLocation as L;
    match location {
        L::OutsideTopLeft | L::OutsideTopCenter | L::OutsideTopRight => Some(Side::North),
        L::OutsideBottomLeft | L::OutsideBottomCenter | L::OutsideBottomRight => Some(Side::South),
        L::OutsideLeftTop | L::OutsideLeftCenter | L::OutsideLeftBottom => Some(Side::West),
        L::OutsideRightTop | L::OutsideRightCenter | L::OutsideRightBottom => Some(Side::East),
        _ => None,
    }
}

/// `NodeContext` with its cell system.
struct Context<'a> {
    node: &'a CellNode,
    node_size: LSize,
    cells: Cells<'a>,
    node_container: CellId,
    node_container_middle_row: CellId,
    inside_node_label_container: CellId,
    /// North, east, south, west.
    outside_containers: Option<[CellId; 4]>,
    /// Indexed by `NodeLabelLocation as usize`; the order of `values()` is ELK's.
    label_cells: Vec<(NodeLabelLocation, CellId)>,
}

impl<'a> Context<'a> {
    /// Source: `NodeContext`'s constructor and `NodeLabelCellCreator.createNodeLabelCells`.
    fn new(node: &'a CellNode, only_inside: bool, horizontal_layout_mode: bool) -> Self {
        let symmetry = !node.sizing.options.contains(SizeOptions::ASYMMETRICAL);
        let tabular = node
            .sizing
            .options
            .contains(SizeOptions::FORCE_TABULAR_NODE_LABELS);
        let label_cell_spacing = 2.0 * node.label_label_spacing;
        let mut cells = Cells {
            cells: Vec::new(),
            label_sizes: &node.labels,
        };
        let node_container = cells.add(Kind::Strip {
            mode: Strip::Vertical,
            symmetrical: symmetry,
            gap: 0.0,
            cells: [None; 3],
        });
        let node_container_middle_row = cells.add(Kind::Strip {
            mode: Strip::Horizontal,
            symmetrical: symmetry,
            gap: 0.0,
            cells: [None; 3],
        });
        set_strip_cell(&mut cells, node_container, 1, node_container_middle_row);

        let inside_node_label_container = cells.add(Kind::Grid {
            gap: label_cell_spacing,
            tabular,
            symmetrical: symmetry,
            cells: [[None; 3]; 3],
            center_cell_minimum_size: None,
            only_center_cell_contributes: false,
            center_cell_rect: Rect::default(),
        });
        cells.cells[inside_node_label_container].padding = node.node_labels_padding;
        set_strip_cell(
            &mut cells,
            node_container_middle_row,
            1,
            inside_node_label_container,
        );

        let outside_containers = (!only_inside).then(|| {
            let mut container = |mode: Strip| {
                cells.add(Kind::Strip {
                    mode,
                    symmetrical: symmetry,
                    gap: label_cell_spacing,
                    cells: [None; 3],
                })
            };
            let north = container(Strip::Horizontal);
            let south = container(Strip::Horizontal);
            let west = container(Strip::Vertical);
            let east = container(Strip::Vertical);
            cells.cells[north].padding.bottom = node.label_node_spacing;
            cells.cells[south].padding.top = node.label_node_spacing;
            cells.cells[west].padding.right = node.label_node_spacing;
            cells.cells[east].padding.left = node.label_node_spacing;
            [north, east, south, west]
        });

        let mut context = Self {
            node,
            node_size: node.size,
            cells,
            node_container,
            node_container_middle_row,
            inside_node_label_container,
            outside_containers,
            label_cells: Vec::new(),
        };

        for (index, label) in node.labels.iter().enumerate() {
            let placement = label.placement.unwrap_or(node.placement);
            let location = placement.location();
            if location == NodeLabelLocation::Fixed {
                continue;
            }
            if only_inside && !location.is_inside() {
                continue;
            }
            let cell = context.label_cell(location, horizontal_layout_mode);
            context.cells.add_label(cell, index);
        }
        context
    }

    /// Source: `NodeLabelCellCreator.retrieveNodeLabelCell`.
    fn label_cell(&mut self, location: NodeLabelLocation, horizontal_layout_mode: bool) -> CellId {
        if let Some((_, cell)) = self.label_cells.iter().find(|(l, _)| *l == location) {
            return *cell;
        }
        let (horizontal, vertical, row, column) =
            location_cell(location).expect("a fixed location has no cell");
        let cell = self.cells.add(Kind::Label {
            horizontal_layout_mode,
            horizontal,
            vertical,
            gap: self.node.label_label_spacing,
            labels: Vec::new(),
            minimum_content: LSize::default(),
        });
        self.label_cells.push((location, cell));
        if location.is_inside() {
            if let Kind::Grid { cells, .. } =
                &mut self.cells.cells[self.inside_node_label_container].kind
            {
                cells[row][column] = Some(cell);
            }
        } else if let (Some(side), Some(containers)) =
            (outside_side(location), self.outside_containers)
        {
            match side {
                Side::North | Side::South => {
                    self.cells.cells[cell].contributes_to_minimum_height = true;
                    let container = if side == Side::North {
                        containers[0]
                    } else {
                        containers[2]
                    };
                    set_strip_cell(&mut self.cells, container, column, cell);
                }
                Side::West | Side::East => {
                    self.cells.cells[cell].contributes_to_minimum_width = true;
                    let container = if side == Side::West {
                        containers[3]
                    } else {
                        containers[1]
                    };
                    set_strip_cell(&mut self.cells, container, row, cell);
                }
            }
        }
        cell
    }

    /// Source: `InsidePortLabelCellCreator.createInsidePortLabelCells`. A node without ports
    /// gets four empty cells. Their paddings never count, because an atomic cell with an
    /// empty content area has no minimum size.
    fn create_inside_port_label_cells(&mut self) {
        for (container, area) in [
            (self.node_container, 0),
            (self.node_container, 2),
            (self.node_container_middle_row, 0),
            (self.node_container_middle_row, 2),
        ] {
            let cell = self.cells.add(Kind::Atomic {
                minimum_content: LSize::default(),
            });
            set_strip_cell(&mut self.cells, container, area, cell);
        }
    }

    /// Source: `NodeLabelAndSizeUtilities.setupMinimumClientAreaSize` and
    /// `getMinimumClientAreaSize`.
    fn setup_minimum_client_area_size(&mut self) {
        let sizing = self.node.sizing;
        if sizing.constraints.contains(SizeConstraints::MINIMUM_SIZE)
            && sizing
                .options
                .contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
        {
            let minimum = minimum_node_or_client_area_size(&sizing);
            if let Kind::Grid {
                center_cell_minimum_size,
                ..
            } = &mut self.cells.cells[self.inside_node_label_container].kind
            {
                *center_cell_minimum_size = Some(minimum);
            }
        }
    }

    /// Source: `CellSystemConfigurator.configureCellSystemSizeContributions`, without the
    /// `PORTS` branch: ports are refused before this runs.
    fn configure_size_contributions(&mut self) {
        let sizing = self.node.sizing;
        if sizing.constraints.is_empty() {
            return;
        }
        if sizing.constraints.contains(SizeConstraints::NODE_LABELS) {
            let inside = self.inside_node_label_container;
            let middle = self.node_container_middle_row;
            for id in [inside, middle] {
                self.cells.cells[id].contributes_to_minimum_height = true;
                self.cells.cells[id].contributes_to_minimum_width = true;
            }
            let overhang = sizing
                .options
                .contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG);
            for (location, cell) in self.label_cells.clone() {
                let contributes = location.is_inside() || !overhang;
                self.cells.cells[cell].contributes_to_minimum_height = contributes;
                self.cells.cells[cell].contributes_to_minimum_width = contributes;
            }
        }
        if sizing.constraints.contains(SizeConstraints::MINIMUM_SIZE)
            && sizing
                .options
                .contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
        {
            // ELK sets the height flag twice and never the width flag. Port that.
            self.cells.cells[self.node_container_middle_row].contributes_to_minimum_height = true;
            let inside = self.inside_node_label_container;
            if !self.cells.cells[inside].contributes_to_minimum_height {
                self.cells.cells[inside].contributes_to_minimum_height = true;
                self.cells.cells[inside].contributes_to_minimum_width = true;
                if let Kind::Grid {
                    only_center_cell_contributes,
                    ..
                } = &mut self.cells.cells[inside].kind
                {
                    *only_center_cell_contributes = true;
                }
            }
        }
    }

    fn size_constraints_fixed(&self) -> bool {
        // Source: `NodeLabelAndSizeUtilities.areSizeConstraintsFixed`.
        let constraints = self.node.sizing.constraints;
        constraints.is_empty() || constraints == SizeConstraints::PORT_LABELS
    }

    /// Source: `NodeSizeCalculator.setNodeWidth`.
    fn set_node_width(&mut self) {
        let width = if self.size_constraints_fixed() {
            self.node_size.width
        } else {
            let mut width = self.cells.minimum_width(self.node_container);
            let sizing = self.node.sizing;
            if sizing.constraints.contains(SizeConstraints::NODE_LABELS)
                && !sizing
                    .options
                    .contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG)
                && let Some(containers) = self.outside_containers
            {
                width = f64::max(width, self.cells.minimum_width(containers[0]));
                width = f64::max(width, self.cells.minimum_width(containers[2]));
            }
            if let Some(minimum) = minimum_node_size(&sizing) {
                width = f64::max(width, minimum.width);
            }
            width
        };
        if self.node.fixed_graph_size {
            self.node_size.width = f64::max(self.node_size.width, width);
        } else {
            self.node_size.width = width;
        }
        let container = self.node_container;
        self.cells.cells[container].rect.x = 0.0;
        self.cells.cells[container].rect.width = width;
        self.cells.layout_children_horizontally(container);
    }

    /// Source: `NodeSizeCalculator.setNodeHeight`.
    fn set_node_height(&mut self) {
        let height = if self.size_constraints_fixed() {
            self.node_size.height
        } else {
            let mut height = self.cells.minimum_height(self.node_container);
            let sizing = self.node.sizing;
            if sizing.constraints.contains(SizeConstraints::NODE_LABELS)
                && !sizing
                    .options
                    .contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG)
                && let Some(containers) = self.outside_containers
            {
                height = f64::max(height, self.cells.minimum_height(containers[1]));
                height = f64::max(height, self.cells.minimum_height(containers[3]));
            }
            if let Some(minimum) = minimum_node_size(&sizing) {
                height = f64::max(height, minimum.height);
            }
            height
        };
        if self.node.fixed_graph_size {
            self.node_size.height = f64::max(self.node_size.height, height);
        } else {
            self.node_size.height = height;
        }
        let container = self.node_container;
        self.cells.cells[container].rect.y = 0.0;
        self.cells.cells[container].rect.height = height;
        self.cells.layout_children_vertically(container);
    }

    /// Source: `LabelPlacer.placeLabels`, without port labels.
    fn place_labels(&mut self) -> Vec<Option<LPoint>> {
        if let Some(containers) = self.outside_containers {
            let overhang = self
                .node
                .sizing
                .options
                .contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG);
            self.place_horizontal_outer_container(containers[0], overhang, true);
            self.place_horizontal_outer_container(containers[2], overhang, false);
            self.place_vertical_outer_container(containers[1], overhang, true);
            self.place_vertical_outer_container(containers[3], overhang, false);
        }
        let mut positions = vec![None; self.node.labels.len()];
        // `nodeLabelCells` is an `EnumMap`, so its values come in location order.
        let mut label_cells = self.label_cells.clone();
        label_cells.sort_by_key(|(location, _)| location_order(*location));
        for (_, cell) in label_cells {
            self.cells.apply_label_layout(cell, &mut positions);
        }
        positions
    }

    /// Source: `LabelPlacer.placeHorizontalOuterNodeLabelContainer`.
    fn place_horizontal_outer_container(&mut self, id: CellId, overhang: bool, north: bool) {
        let node_size = self.node_size;
        let mut rect = Rect {
            width: self.cells.minimum_width(id),
            height: self.cells.minimum_height(id),
            ..Rect::default()
        };
        rect.width = f64::max(rect.width, node_size.width);
        if rect.width > node_size.width && !overhang {
            rect.width = node_size.width;
        }
        rect.x = -(rect.width - node_size.width) / 2.0;
        rect.y = if north {
            -rect.height
        } else {
            node_size.height
        };
        self.cells.cells[id].rect = rect;
        self.cells.layout_children_horizontally(id);
        self.cells.layout_children_vertically(id);
    }

    /// Source: `LabelPlacer.placeVerticalOuterNodeLabelContainer`.
    fn place_vertical_outer_container(&mut self, id: CellId, overhang: bool, east: bool) {
        let node_size = self.node_size;
        let mut rect = Rect {
            width: self.cells.minimum_width(id),
            height: self.cells.minimum_height(id),
            ..Rect::default()
        };
        rect.height = f64::max(rect.height, node_size.height);
        if rect.height > node_size.height && !overhang {
            rect.height = node_size.height;
        }
        rect.y = -(rect.height - node_size.height) / 2.0;
        rect.x = if east { node_size.width } else { -rect.width };
        self.cells.cells[id].rect = rect;
        self.cells.layout_children_horizontally(id);
        self.cells.layout_children_vertically(id);
    }

    /// Source: `NodeLabelAndSizeUtilities.setNodePadding`.
    fn node_padding(&self) -> Option<ElkPadding> {
        if !self
            .node
            .sizing
            .options
            .contains(SizeOptions::COMPUTE_PADDING)
        {
            return None;
        }
        let node_rect = self.cells.cells[self.node_container].rect;
        let Kind::Grid {
            center_cell_rect, ..
        } = self.cells.cells[self.inside_node_label_container].kind
        else {
            return None;
        };
        // `getCenterCellRectangle` returns a copy.
        let client = center_cell_rect;
        Some(ElkPadding {
            left: client.x - node_rect.x,
            top: client.y - node_rect.y,
            right: (node_rect.x + node_rect.width) - (client.x + client.width),
            bottom: (node_rect.y + node_rect.height) - (client.y + client.height),
        })
    }
}

fn set_strip_cell(cells: &mut Cells<'_>, container: CellId, area: usize, cell: CellId) {
    if let Kind::Strip { cells: slots, .. } = &mut cells.cells[container].kind {
        slots[area] = Some(cell);
    }
}

/// The position of a location in ELK's `NodeLabelLocation.values()`.
fn location_order(location: NodeLabelLocation) -> usize {
    use NodeLabelLocation as L;
    [
        L::OutsideTopLeft,
        L::OutsideTopCenter,
        L::OutsideTopRight,
        L::OutsideBottomLeft,
        L::OutsideBottomCenter,
        L::OutsideBottomRight,
        L::OutsideLeftTop,
        L::OutsideLeftCenter,
        L::OutsideLeftBottom,
        L::OutsideRightTop,
        L::OutsideRightCenter,
        L::OutsideRightBottom,
        L::InsideTopLeft,
        L::InsideTopCenter,
        L::InsideTopRight,
        L::InsideCenterLeft,
        L::InsideCenter,
        L::InsideCenterRight,
        L::InsideBottomLeft,
        L::InsideBottomCenter,
        L::InsideBottomRight,
    ]
    .iter()
    .position(|candidate| *candidate == location)
    .unwrap_or(usize::MAX)
}

/// Source: `NodeLabelAndSizeUtilities.getMinimumNodeSize`.
fn minimum_node_size(sizing: &NodeSizing) -> Option<LSize> {
    if sizing.constraints.contains(SizeConstraints::MINIMUM_SIZE)
        && !sizing
            .options
            .contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
    {
        return Some(minimum_node_or_client_area_size(sizing));
    }
    None
}

/// Source: `NodeLabelAndSizeUtilities.getMinimumNodeOrClientAreaSize`.
fn minimum_node_or_client_area_size(sizing: &NodeSizing) -> LSize {
    let mut minimum = sizing.minimum;
    if sizing.options.contains(SizeOptions::DEFAULT_MINIMUM_SIZE) {
        if minimum.width <= 0.0 {
            minimum.width = 20.0;
        }
        if minimum.height <= 0.0 {
            minimum.height = 20.0;
        }
    }
    minimum
}

/// Sizes a node and places its labels.
///
/// Source: `NodeLabelAndSizeCalculator.process(graph, node, applyStuff, false)`. With
/// `apply` false, ELK stops once it knows the size, so no label moves.
pub(crate) fn process(node: &CellNode, horizontal_layout_mode: bool, apply: bool) -> CellResult {
    let mut context = Context::new(node, false, horizontal_layout_mode);
    context.create_inside_port_label_cells();
    context.setup_minimum_client_area_size();
    context.configure_size_contributions();
    context.set_node_width();
    context.set_node_height();
    if !apply {
        return CellResult {
            size: context.node_size,
            label_positions: vec![None; node.labels.len()],
            padding: None,
        };
    }
    let label_positions = context.place_labels();
    let padding = context.node_padding();
    CellResult {
        size: context.node_size,
        label_positions,
        padding,
    }
}

/// The padding a hierarchical node keeps free for its inside labels.
///
/// Source: `NodeLabelAndSizeCalculator.computeInsideNodeLabelPadding`. ELK calls it with
/// `Direction.RIGHT`, so labels stack vertically.
pub(crate) fn inside_node_label_padding(node: &CellNode) -> ElkPadding {
    let context = Context::new(node, true, true);
    let Kind::Grid { cells, gap, .. } =
        &context.cells.cells[context.inside_node_label_container].kind
    else {
        return zero_padding();
    };
    let container_padding = context.cells.cells[context.inside_node_label_container].padding;
    let height = |cell: Option<CellId>| cell.map(|id| context.cells.minimum_height(id));
    let width = |cell: Option<CellId>| cell.map(|id| context.cells.minimum_width(id));
    let mut padding = zero_padding();
    for (top, bottom) in cells[0].iter().zip(&cells[2]) {
        if let Some(h) = height(*top) {
            padding.top = f64::max(padding.top, h);
        }
        if let Some(h) = height(*bottom) {
            padding.bottom = f64::max(padding.bottom, h);
        }
    }
    for row in cells {
        if let Some(w) = width(row[0]) {
            padding.left = f64::max(padding.left, w);
        }
        if let Some(w) = width(row[2]) {
            padding.right = f64::max(padding.right, w);
        }
    }
    if padding.top > 0.0 {
        padding.top += container_padding.top;
        padding.top += gap;
    }
    if padding.bottom > 0.0 {
        padding.bottom += container_padding.bottom;
        padding.bottom += gap;
    }
    if padding.left > 0.0 {
        padding.left += container_padding.left;
        padding.left += gap;
    }
    if padding.right > 0.0 {
        padding.right += container_padding.right;
        padding.right += gap;
    }
    padding
}

#[cfg(test)]
mod tests {
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
}
