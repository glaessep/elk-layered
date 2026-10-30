//! ELK's cell system for node labels, ports, port labels and node sizes.
//!
//! This ports `NodeLabelAndSizeCalculator`: the cells that hold node labels and port labels, the
//! node size they ask for, where the ports go, and where the labels go.
//!
//! Source references, all under
//! `plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/nodespacing/`:
//! - `NodeLabelAndSizeCalculator.java`
//! - `cellsystem/{Cell,AtomicCell,LabelCell,ContainerCell,GridContainerCell,StripContainerCell}.java`
//! - `internal/{NodeContext,PortContext,NodeLabelLocation}.java`
//! - `internal/algorithm/{PortContextCreator,NodeLabelCellCreator,InsidePortLabelCellCreator,
//!   HorizontalPortPlacementSizeCalculator,VerticalPortPlacementSizeCalculator,
//!   CellSystemConfigurator,NodeSizeCalculator,PortPlacementCalculator,
//!   PortLabelPlacementCalculator,NodeLabelAndSizeUtilities,LabelPlacer}.java`
//!
//! and `plugins/org.eclipse.elk.alg.common/src/org/eclipse/elk/alg/common/overlaps/`
//! `{RectangleStripOverlapRemover,GreedyRectangleStripOverlapRemover}.java`.

use crate::graph::{LPoint, LSize, PortSide};
use crate::options::{
    ElkPadding, NodeLabelLocation, NodeLabelPlacement, NodeSizing, PortAlignment, PortConstraints,
    PortLabelPlacement, SizeConstraints, SizeOptions,
};

/// Source: `LabelCell.addLabel`, the part that grows the cell's minimum content size.
/// `SelfHyperLoopLabels.updateSize` repeats the same arithmetic. One function serves every
/// caller, so all of them add the floats in the order Java does.
///
/// `label_count` counts the label being added, so it is 1 for the first. In horizontal layout
/// mode the cell stacks its labels one below the other; otherwise it puts them side by side.
pub(crate) fn grow_label_cell(
    content: &mut LSize,
    label: LSize,
    label_count: usize,
    gap: f64,
    horizontal_layout_mode: bool,
) {
    if horizontal_layout_mode {
        content.width = f64::max(content.width, label.width);
        content.height += label.height;
        if label_count > 1 {
            content.height += gap;
        }
    } else {
        content.width += label.width;
        content.height = f64::max(content.height, label.height);
        if label_count > 1 {
            content.width += gap;
        }
    }
}

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
    /// The node's ports and what `NodeContext` reads about them.
    pub(crate) ports: CellPorts,
}

/// One label of a port, as the cell system sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellPortLabel {
    pub(crate) size: LSize,
    /// Relative to the port. `ElkUtil.getLabelsBounds` reads it when the port labels are fixed.
    pub(crate) position: LPoint,
}

/// One port, as `PortContext` reads it through its `PortAdapter`.
#[derive(Debug, Clone)]
pub(crate) struct CellPort {
    /// Never `Undefined`: `PortContextCreator` throws for such a port, and every caller assigns
    /// sides first.
    pub(crate) side: PortSide,
    pub(crate) size: LSize,
    /// Relative to the node. A port the cells do not place keeps it.
    pub(crate) position: LPoint,
    /// `port.borderOffset`, when set.
    pub(crate) border_offset: Option<f64>,
    /// `PortPlacementCalculator.PORT_RATIO_OR_POSITION`.
    pub(crate) ratio_or_position: f64,
    pub(crate) labels: Vec<CellPortLabel>,
    /// Whether an edge starts or ends at the port.
    pub(crate) has_edges: bool,
    /// `PortAdapter.hasCompoundConnections`: an edge connects the port to a descendant of its node.
    pub(crate) has_compound_connections: bool,
}

/// What `NodeContext` reads about a node's ports.
#[derive(Debug, Clone)]
pub(crate) struct CellPorts {
    /// In the order of the node's port list.
    pub(crate) ports: Vec<CellPort>,
    pub(crate) constraints: PortConstraints,
    /// `portLabels.placement`.
    pub(crate) label_placement: PortLabelPlacement,
    /// `portLabels.treatAsGroup`.
    pub(crate) treat_as_group: bool,
    /// `NodeContext.treatAsCompoundNode`: the node has children.
    pub(crate) compound: bool,
    /// `spacing.portPort`.
    pub(crate) port_port_spacing: f64,
    /// `spacing.labelPortHorizontal`.
    pub(crate) label_port_horizontal: f64,
    /// `spacing.labelPortVertical`.
    pub(crate) label_port_vertical: f64,
    /// `spacing.portsSurrounding`.
    pub(crate) surrounding: ElkPadding,
    /// `portAlignment.default`. The alignments by side are refused before this runs.
    pub(crate) alignment: PortAlignment,
}

#[cfg(test)]
impl Default for CellPorts {
    /// No ports, with ELK's defaults for the rest.
    fn default() -> Self {
        let spacing = crate::options::SpacingOptions::default();
        Self {
            ports: Vec::new(),
            constraints: PortConstraints::Free,
            label_placement: PortLabelPlacement::OUTSIDE,
            treat_as_group: true,
            compound: false,
            port_port_spacing: spacing.port_port,
            label_port_horizontal: spacing.label_port_horizontal,
            label_port_vertical: spacing.label_port_vertical,
            surrounding: spacing.ports_surrounding.into(),
            alignment: PortAlignment::Distributed,
        }
    }
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
    /// The position of each port, by port index, relative to the node. Empty when the cells
    /// stopped before they placed the ports.
    pub(crate) port_positions: Vec<LPoint>,
    /// A new position for each label of each port, relative to the port, by port index and
    /// label index. `None` for a label the cells did not place.
    pub(crate) port_label_positions: Vec<Vec<Option<LPoint>>>,
}

/// Source: `ElkRectangle`. `NodeMarginCalculator` in `nodespacing.rs` uses it too.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Rect {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

impl Rect {
    /// Source: `ElkRectangle.union`.
    pub(super) fn union(&mut self, other: Self) {
        let mut x1 = f64::min(self.x, other.x);
        let mut y1 = f64::min(self.y, other.y);
        let mut x2 = f64::max(self.x + self.width, other.x + other.width);
        let mut y2 = f64::max(self.y + self.height, other.y + other.height);
        if x2 < x1 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if y2 < y1 {
            std::mem::swap(&mut y1, &mut y2);
        }
        *self = Self {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        };
    }
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
struct Cells {
    cells: Vec<Cell>,
    /// The sizes of the node's labels, then those of each port's labels, port by port.
    label_sizes: Vec<LSize>,
}

impl Cells {
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
        let label_size = self.label_sizes[label];
        if let Kind::Label {
            horizontal_layout_mode,
            gap,
            labels,
            minimum_content,
            ..
        } = &mut self.cells[id].kind
        {
            labels.push(label);
            grow_label_cell(
                minimum_content,
                label_size,
                labels.len(),
                *gap,
                *horizontal_layout_mode,
            );
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
                let size = self.label_sizes[label];
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
                let size = self.label_sizes[label];
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

/// Whether ports on the side line up along the width of the node.
fn is_horizontal_side(side: PortSide) -> bool {
    matches!(side, PortSide::North | PortSide::South)
}

/// Source: `HorizontalPortPlacementSizeCalculator.minSizeRequiredToRespectSpacing`.
fn min_size_required_to_respect_spacing(spacing: f64, first_ratio: f64, second_ratio: f64) -> f64 {
    if fuzzy_equals(first_ratio, second_ratio, 0.01) {
        0.0
    } else {
        spacing / (second_ratio - first_ratio)
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
    cells: Cells,
    node_container: CellId,
    node_container_middle_row: CellId,
    inside_node_label_container: CellId,
    /// North, east, south, west.
    outside_containers: Option<[CellId; 4]>,
    /// Indexed by `NodeLabelLocation as usize`; the order of `values()` is ELK's.
    label_cells: Vec<(NodeLabelLocation, CellId)>,
    /// `NodeContext.portContexts`: sorted by side, then as `comparePortContexts` orders them.
    port_contexts: Vec<PortContext>,
    /// The range of `port_contexts` that holds the ports of each side, by `side_ordinal`.
    side_ranges: [std::ops::Range<usize>; 5],
    /// `NodeContext.insidePortLabelCells`, by `side_slot`.
    inside_port_label_cells: [CellId; 4],
    /// `NodeContext.labelCellSpacing`.
    label_cell_spacing: f64,
}

/// Source: `PortContext`.
#[derive(Debug, Clone)]
struct PortContext {
    /// The index of the port in `CellPorts::ports`.
    port: usize,
    position: LPoint,
    labels_next_to_port: bool,
    /// `portMargin`, an `ElkMargin`.
    margin: ElkPadding,
    /// `portLabelCell`, absent when the labels are fixed or ignored.
    label_cell: Option<CellId>,
}

/// The slot of a side in `Context::inside_port_label_cells`.
fn side_slot(side: PortSide) -> usize {
    match side {
        PortSide::North => 0,
        PortSide::East => 1,
        PortSide::South => 2,
        PortSide::West | PortSide::Undefined => 3,
    }
}

/// Source: `NodeContext.comparePortSides`, by `PortSide.ordinal()`.
fn side_ordinal(side: PortSide) -> usize {
    match side {
        PortSide::Undefined => 0,
        PortSide::North => 1,
        PortSide::East => 2,
        PortSide::South => 3,
        PortSide::West => 4,
    }
}

/// Source: Guava's `DoubleMath.fuzzyEquals`.
fn fuzzy_equals(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance || a == b
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
        let mut label_sizes: Vec<LSize> = node.labels.iter().map(|label| label.size).collect();
        for port in &node.ports.ports {
            label_sizes.extend(port.labels.iter().map(|label| label.size));
        }
        let mut cells = Cells {
            cells: Vec::new(),
            label_sizes,
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
            port_contexts: Vec::new(),
            side_ranges: Default::default(),
            inside_port_label_cells: [0; 4],
            label_cell_spacing,
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
            let Some(cell) = context.label_cell(location, horizontal_layout_mode) else {
                continue;
            };
            context.cells.add_label(cell, index);
        }
        context
    }

    /// Source: `NodeLabelCellCreator.retrieveNodeLabelCell`.
    ///
    /// A fixed location has no cell, so it gives `None`.
    fn label_cell(
        &mut self,
        location: NodeLabelLocation,
        horizontal_layout_mode: bool,
    ) -> Option<CellId> {
        if let Some((_, cell)) = self.label_cells.iter().find(|(l, _)| *l == location) {
            return Some(*cell);
        }
        let (horizontal, vertical, row, column) = location_cell(location)?;
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
        Some(cell)
    }

    /// Source: `PortContextCreator.createPortContexts` and `PortContext`'s constructor.
    fn create_port_contexts(&mut self, ignore_inside_port_labels: bool) {
        let ports = &self.node.ports;
        let placement = ports.label_placement;
        let im_port_labels =
            !ignore_inside_port_labels || !placement.contains(PortLabelPlacement::INSIDE);
        let next_to_port = placement.contains(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);
        let mut label_id = self.node.labels.len();
        let mut contexts = Vec::with_capacity(ports.ports.len());
        for (index, port) in ports.ports.iter().enumerate() {
            let labels_next_to_port = if placement.contains(PortLabelPlacement::INSIDE) {
                if ports.compound {
                    next_to_port && !port.has_compound_connections
                } else {
                    true
                }
            } else if placement.contains(PortLabelPlacement::OUTSIDE) {
                next_to_port && !port.has_edges
            } else {
                false
            };
            let label_cell = (im_port_labels && !placement.is_fixed()).then(|| {
                let cell = self.cells.add(Kind::Label {
                    horizontal_layout_mode: true,
                    horizontal: HAlign::Center,
                    vertical: VAlign::Center,
                    gap: self.node.label_label_spacing,
                    labels: Vec::new(),
                    minimum_content: LSize::default(),
                });
                for label in 0..port.labels.len() {
                    self.cells.add_label(cell, label_id + label);
                }
                cell
            });
            label_id += port.labels.len();
            contexts.push(PortContext {
                port: index,
                position: port.position,
                labels_next_to_port,
                margin: zero_padding(),
                label_cell,
            });
        }
        // `TreeMultimap` with `comparePortSides` and `comparePortContexts`: north and east ports
        // in list order, south and west ports in reverse list order.
        contexts.sort_by(|a, b| {
            let side_a = ports.ports[a.port].side;
            let side_b = ports.ports[b.port].side;
            side_ordinal(side_a)
                .cmp(&side_ordinal(side_b))
                .then_with(|| match side_a {
                    PortSide::South | PortSide::West => b.port.cmp(&a.port),
                    _ => a.port.cmp(&b.port),
                })
        });
        let mut ranges: [std::ops::Range<usize>; 5] = Default::default();
        let mut start = 0;
        for (ordinal, range) in ranges.iter_mut().enumerate() {
            let end = start
                + contexts[start..]
                    .iter()
                    .take_while(|context| side_ordinal(ports.ports[context.port].side) == ordinal)
                    .count();
            *range = start..end;
            start = end;
        }
        self.side_ranges = ranges;
        self.port_contexts = contexts;
    }

    /// The indices into `port_contexts` of the ports on one side, in order:
    /// `portContexts.get(side)`. The contexts are sorted by side, so each side is one range.
    fn side_contexts(&self, side: PortSide) -> Vec<usize> {
        self.side_ranges[side_ordinal(side)].clone().collect()
    }

    /// The port of a context. It borrows from the node, not from `self`, so the cells and the
    /// contexts stay free to change.
    fn port(&self, context: usize) -> &'a CellPort {
        let node: &'a CellNode = self.node;
        &node.ports.ports[self.port_contexts[context].port]
    }

    fn inside_cell(&self, side: PortSide) -> CellId {
        self.inside_port_label_cells[side_slot(side)]
    }

    fn atomic_content(&mut self, cell: CellId) -> &mut LSize {
        match &mut self.cells.cells[cell].kind {
            Kind::Atomic { minimum_content }
            | Kind::Label {
                minimum_content, ..
            } => minimum_content,
            Kind::Strip { .. } | Kind::Grid { .. } => {
                unreachable!("the inside port label cells are atomic")
            }
        }
    }

    fn content_of(&self, cell: CellId) -> LSize {
        match &self.cells.cells[cell].kind {
            Kind::Atomic { minimum_content }
            | Kind::Label {
                minimum_content, ..
            } => *minimum_content,
            Kind::Strip { .. } | Kind::Grid { .. } => LSize::default(),
        }
    }

    /// Source: `InsidePortLabelCellCreator.createInsidePortLabelCells`.
    fn create_inside_port_label_cells(&mut self) {
        for (container, area, side) in [
            (self.node_container, 0, PortSide::North),
            (self.node_container, 2, PortSide::South),
            (self.node_container_middle_row, 0, PortSide::West),
            (self.node_container_middle_row, 2, PortSide::East),
        ] {
            let cell = self.cells.add(Kind::Atomic {
                minimum_content: LSize::default(),
            });
            set_strip_cell(&mut self.cells, container, area, cell);
            self.inside_port_label_cells[side_slot(side)] = cell;
        }
        let ports = &self.node.ports;
        // `setupNorthOrSouthPortLabelCell`.
        for side in [PortSide::North, PortSide::South] {
            let cell = self.inside_cell(side);
            let padding = &mut self.cells.cells[cell].padding;
            if ports.label_port_vertical >= 0.0 {
                if side == PortSide::North {
                    padding.top = ports.label_port_vertical;
                } else {
                    padding.bottom = ports.label_port_vertical;
                }
            }
            padding.left = ports.surrounding.left;
            padding.right = ports.surrounding.right;
        }
        // `setupEastOrWestPortLabelCell`.
        for side in [PortSide::East, PortSide::West] {
            let cell = self.inside_cell(side);
            if ports.label_placement.contains(PortLabelPlacement::INSIDE) {
                // `calculateWidthDueToLabels`.
                let mut width = self.content_of(cell).width;
                for context in self.side_contexts(side) {
                    if let Some(label_cell) = self.port_contexts[context].label_cell {
                        width = f64::max(width, self.cells.minimum_width(label_cell));
                    }
                }
                self.atomic_content(cell).width = width;
                if width > 0.0 {
                    let padding = &mut self.cells.cells[cell].padding;
                    if side == PortSide::East {
                        padding.right = ports.label_port_horizontal;
                    } else {
                        padding.left = ports.label_port_horizontal;
                    }
                }
            }
            // `setupTopAndBottomPadding`.
            let padding = &mut self.cells.cells[cell].padding;
            padding.top = ports.surrounding.top;
            padding.bottom = ports.surrounding.bottom;
        }
    }

    /// Source: `ElkUtil.getLabelsBounds`: the union of the port's labels, relative to the port,
    /// or an empty rectangle.
    fn labels_bounds(port: &CellPort) -> Rect {
        let mut bounds: Option<Rect> = None;
        for label in &port.labels {
            let rect = Rect {
                x: label.position.x,
                y: label.position.y,
                width: label.size.width,
                height: label.size.height,
            };
            bounds = Some(match bounds {
                None => rect,
                Some(mut bounds) => {
                    bounds.union(rect);
                    bounds
                }
            });
        }
        bounds.unwrap_or_default()
    }

    /// Source: `ElkUtil.computeInsidePart(PortAdapter, double)`, which hands the bounds of the
    /// labels to the overload for a position and a size.
    fn compute_inside_part(port: &CellPort, port_border_offset: f64) -> f64 {
        let bounds = Self::labels_bounds(port);
        crate::graph::compute_inside_part(
            LPoint {
                x: bounds.x,
                y: bounds.y,
            },
            LSize {
                width: bounds.width,
                height: bounds.height,
            },
            port.size,
            port_border_offset,
            port.side,
        )
    }

    /// Source: `NodeLabelAndSizeUtilities.setupNodePaddingForPortsWithOffset`.
    fn setup_node_padding_for_ports_with_offset(&mut self) {
        let symmetry = !self.node.sizing.options.contains(SizeOptions::ASYMMETRICAL);
        let fixed = self.node.ports.label_placement.is_fixed();
        for context in 0..self.port_contexts.len() {
            let port = self.port(context);
            let padding = &mut self.cells.cells[self.node_container].padding;
            let mut port_border_offset = 0.0;
            if let Some(offset) = port.border_offset {
                port_border_offset = offset;
                if offset < 0.0 {
                    match port.side {
                        PortSide::North => padding.top = f64::max(padding.top, -offset),
                        PortSide::South => padding.bottom = f64::max(padding.bottom, -offset),
                        PortSide::East => padding.right = f64::max(padding.right, -offset),
                        PortSide::West => padding.left = f64::max(padding.left, -offset),
                        PortSide::Undefined => {}
                    }
                }
            }
            if fixed {
                let inside_part = Self::compute_inside_part(port, port_border_offset);
                match port.side {
                    PortSide::North => {
                        let bigger = inside_part > padding.top;
                        padding.top = f64::max(padding.top, inside_part);
                        if symmetry && bigger {
                            padding.top = f64::max(padding.top, padding.bottom);
                            padding.bottom = padding.top + port_border_offset;
                        }
                    }
                    PortSide::South => {
                        let bigger = inside_part > padding.bottom;
                        padding.bottom = f64::max(padding.bottom, inside_part);
                        if symmetry && bigger {
                            padding.bottom = f64::max(padding.bottom, padding.top);
                            padding.top = padding.bottom + port_border_offset;
                        }
                    }
                    PortSide::East => {
                        let bigger = inside_part > padding.right;
                        padding.right = f64::max(padding.right, inside_part);
                        if symmetry && bigger {
                            padding.right = f64::max(padding.left, padding.right);
                            padding.left = padding.right + port_border_offset;
                        }
                    }
                    PortSide::West => {
                        let bigger = inside_part > padding.left;
                        padding.left = f64::max(padding.left, inside_part);
                        if symmetry && bigger {
                            padding.left = f64::max(padding.left, padding.right);
                            padding.right = padding.left + port_border_offset;
                        }
                    }
                    PortSide::Undefined => {}
                }
            }
        }
    }

    /// Source: `HorizontalPortPlacementSizeCalculator.calculateHorizontalPortPlacementSize` and
    /// `VerticalPortPlacementSizeCalculator.calculateVerticalPortPlacementSize`.
    fn calculate_port_placement_size(&mut self, horizontal: bool) {
        let sides = if horizontal {
            [PortSide::North, PortSide::South]
        } else {
            [PortSide::East, PortSide::West]
        };
        for side in sides {
            match self.node.ports.constraints {
                PortConstraints::FixedPos => self.size_required_by_fixed_pos_ports(side),
                PortConstraints::FixedRatio => self.size_required_by_fixed_ratio_ports(side),
                _ => self.size_required_by_free_ports(side),
            }
        }
    }

    /// Source: `calculate{Horizontal,Vertical}NodeSizeRequiredByFixedPosPorts`.
    fn size_required_by_fixed_pos_ports(&mut self, side: PortSide) {
        let horizontal = is_horizontal_side(side);
        let mut far_border: f64 = 0.0;
        for context in self.side_contexts(side) {
            let position = self.port_contexts[context].position;
            let size = self.port(context).size;
            far_border = if horizontal {
                f64::max(far_border, position.x + size.width)
            } else {
                f64::max(far_border, position.y + size.height)
            };
        }
        let cell = self.inside_cell(side);
        if horizontal {
            self.cells.cells[cell].padding.left = 0.0;
            self.atomic_content(cell).width = far_border;
        } else {
            self.cells.cells[cell].padding.top = 0.0;
            self.atomic_content(cell).height = far_border;
        }
    }

    /// Source: `calculate{Horizontal,Vertical}NodeSizeRequiredByFixedRatioPorts`.
    fn size_required_by_fixed_ratio_ports(&mut self, side: PortSide) {
        let horizontal = is_horizontal_side(side);
        let cell = self.inside_cell(side);
        let contexts = self.side_contexts(side);
        if contexts.is_empty() {
            let padding = &mut self.cells.cells[cell].padding;
            if horizontal {
                padding.left = 0.0;
                padding.right = 0.0;
            } else {
                padding.top = 0.0;
                padding.bottom = 0.0;
            }
            return;
        }
        let ports = &self.node.ports;
        let port_labels_inside = ports.label_placement.contains(PortLabelPlacement::INSIDE);
        let port_labels = self
            .node
            .sizing
            .constraints
            .contains(SizeConstraints::PORT_LABELS);
        let surrounding = ports.surrounding;
        let spacing = ports.port_port_spacing;
        // The vertical calculator sets up the margins once, before the loop. The horizontal one
        // does it again for every port, which gives the same margins each time, so here both do
        // it once.
        if port_labels {
            self.setup_port_margins(side);
        }
        let (near_surrounding, far_surrounding) = if horizontal {
            (surrounding.left, surrounding.right)
        } else {
            (surrounding.top, surrounding.bottom)
        };
        let near = |margin: &ElkPadding| if horizontal { margin.left } else { margin.top };
        let far = |margin: &ElkPadding| {
            if horizontal {
                margin.right
            } else {
                margin.bottom
            }
        };
        let mut min_size: f64 = 0.0;
        let mut previous: Option<usize> = None;
        let mut previous_ratio = 0.0;
        let mut previous_extent = 0.0;
        for &context in &contexts {
            let port = self.port(context);
            let current_ratio = port.ratio_or_position;
            let current_extent = if horizontal {
                port.size.width
            } else {
                port.size.height
            };
            let current_margin = self.port_contexts[context].margin;
            match previous {
                None => {
                    if near_surrounding > 0.0 {
                        min_size = f64::max(
                            min_size,
                            min_size_required_to_respect_spacing(
                                near_surrounding + near(&current_margin),
                                0.0,
                                current_ratio,
                            ),
                        );
                    }
                }
                Some(previous) => {
                    let required = previous_extent
                        + far(&self.port_contexts[previous].margin)
                        + spacing
                        + near(&current_margin);
                    min_size = f64::max(
                        min_size,
                        min_size_required_to_respect_spacing(
                            required,
                            previous_ratio,
                            current_ratio,
                        ),
                    );
                }
            }
            previous = Some(context);
            previous_ratio = current_ratio;
            previous_extent = current_extent;
        }
        if far_surrounding > 0.0 {
            let mut required = previous_extent + far_surrounding;
            if port_labels_inside && let Some(previous) = previous {
                required += far(&self.port_contexts[previous].margin);
            }
            min_size = f64::max(
                min_size,
                min_size_required_to_respect_spacing(required, previous_ratio, 1.0),
            );
        }
        if horizontal {
            self.cells.cells[cell].padding.left = 0.0;
            self.atomic_content(cell).width = min_size;
        } else {
            self.cells.cells[cell].padding.top = 0.0;
            self.atomic_content(cell).height = min_size;
        }
    }

    /// Source: `calculate{Horizontal,Vertical}NodeSizeRequiredByFreePorts`.
    fn size_required_by_free_ports(&mut self, side: PortSide) {
        let horizontal = is_horizontal_side(side);
        let cell = self.inside_cell(side);
        let contexts = self.side_contexts(side);
        let surrounding = self.node.ports.surrounding;
        let padding = &mut self.cells.cells[cell].padding;
        if contexts.is_empty() {
            if horizontal {
                padding.left = 0.0;
                padding.right = 0.0;
            } else {
                padding.top = 0.0;
                padding.bottom = 0.0;
            }
            return;
        }
        if horizontal {
            padding.left = surrounding.left;
            padding.right = surrounding.right;
        } else {
            padding.top = surrounding.top;
            padding.bottom = surrounding.bottom;
        }
        if self
            .node
            .sizing
            .constraints
            .contains(SizeConstraints::PORT_LABELS)
        {
            self.setup_port_margins(side);
        }
        // `portWidthPlusPortPortSpacing` and `portHeightPlusPortPortSpacing`.
        let mut extent = 0.0;
        for (position, &context) in contexts.iter().enumerate() {
            let margin = self.port_contexts[context].margin;
            let size = self.port(context).size;
            extent += if horizontal {
                margin.left + size.width + margin.right
            } else {
                margin.top + size.height + margin.bottom
            };
            if position + 1 < contexts.len() {
                extent += self.node.ports.port_port_spacing;
            }
        }
        if self.node.ports.alignment == PortAlignment::Distributed {
            extent += 2.0 * self.node.ports.port_port_spacing;
        }
        if horizontal {
            self.atomic_content(cell).width = extent;
        } else {
            self.atomic_content(cell).height = extent;
        }
    }

    /// Source: `{Horizontal,Vertical}PortPlacementSizeCalculator.setupPortMargins`.
    fn setup_port_margins(&mut self, side: PortSide) {
        let horizontal = is_horizontal_side(side);
        let contexts = self.side_contexts(side);
        let placement = self.node.ports.label_placement;
        let outside = placement.contains(PortLabelPlacement::OUTSIDE);
        let always_same_side = placement.contains(PortLabelPlacement::ALWAYS_SAME_SIDE);
        let always_other_same_side = placement.contains(PortLabelPlacement::ALWAYS_OTHER_SAME_SIDE);
        let space_efficient = placement.contains(PortLabelPlacement::SPACE_EFFICIENT);
        let uniform = self
            .node
            .sizing
            .options
            .contains(SizeOptions::UNIFORM_PORT_SPACING);
        let space_efficient_labels = !always_same_side
            && !always_other_same_side
            && (space_efficient || contexts.len() == 2);
        if horizontal {
            self.compute_horizontal_port_margins(&contexts);
        } else {
            self.compute_vertical_port_margins(&contexts);
        }
        let (first, last) = (contexts[0], contexts[contexts.len() - 1]);
        let clear_ends = |this: &mut Self| {
            if horizontal {
                this.port_contexts[first].margin.left = 0.0;
                this.port_contexts[last].margin.right = 0.0;
            } else {
                this.port_contexts[first].margin.top = 0.0;
                this.port_contexts[last].margin.bottom = 0.0;
            }
        };
        if outside {
            clear_ends(self);
            if space_efficient_labels && !self.port_contexts[first].labels_next_to_port {
                if horizontal {
                    self.port_contexts[first].margin.right = 0.0;
                } else {
                    self.port_contexts[first].margin.bottom = 0.0;
                }
            }
        }
        if uniform {
            // `unifyPortMargins`.
            let (mut max_near, mut max_far): (f64, f64) = (0.0, 0.0);
            for &context in &contexts {
                let margin = self.port_contexts[context].margin;
                if horizontal {
                    max_near = max_near.max(margin.left);
                    max_far = max_far.max(margin.right);
                } else {
                    max_near = max_near.max(margin.top);
                    max_far = max_far.max(margin.bottom);
                }
            }
            for &context in &contexts {
                let margin = &mut self.port_contexts[context].margin;
                if horizontal {
                    margin.left = max_near;
                    margin.right = max_far;
                } else {
                    margin.top = max_near;
                    margin.bottom = max_far;
                }
            }
            if outside {
                clear_ends(self);
            }
        }
    }

    /// Source: `HorizontalPortPlacementSizeCalculator.computeHorizontalPortMargins`.
    fn compute_horizontal_port_margins(&mut self, contexts: &[usize]) {
        let fixed = self.node.ports.label_placement.is_fixed();
        for &context in contexts {
            let label_width = self.port_contexts[context]
                .label_cell
                .map_or(0.0, |cell| self.cells.minimum_width(cell));
            let port = self.port(context);
            let next_to_port = self.port_contexts[context].labels_next_to_port;
            let margin = &mut self.port_contexts[context].margin;
            if label_width > 0.0 {
                if next_to_port {
                    if label_width > port.size.width {
                        let overhang = (label_width - port.size.width) / 2.0;
                        margin.left = overhang;
                        margin.right = overhang;
                    }
                } else {
                    margin.right = self.node.ports.label_port_horizontal + label_width;
                }
            } else if fixed {
                let bounds = Self::labels_bounds(port);
                if bounds.x < 0.0 {
                    margin.left = -bounds.x;
                }
                if bounds.x + bounds.width > port.size.width {
                    margin.right = bounds.x + bounds.width - port.size.width;
                }
            }
        }
    }

    /// Source: `VerticalPortPlacementSizeCalculator.computeVerticalPortMargins`.
    fn compute_vertical_port_margins(&mut self, contexts: &[usize]) {
        let fixed = self.node.ports.label_placement.is_fixed();
        let treat_as_group = self.node.ports.treat_as_group;
        for &context in contexts {
            let label_cell = self.port_contexts[context].label_cell;
            let label_height = label_cell.map_or(0.0, |cell| self.cells.minimum_height(cell));
            let port = self.port(context);
            if label_height > 0.0 {
                if self.port_contexts[context].labels_next_to_port {
                    let port_height = port.size.height;
                    if label_height > port_height {
                        let labels = label_cell.map_or(&[][..], |cell| self.cell_labels(cell));
                        let margin = if treat_as_group || labels.len() == 1 {
                            let overhang = (label_height - port_height) / 2.0;
                            (overhang, overhang)
                        } else {
                            let first_label_height = self.cells.label_sizes[labels[0]].height;
                            let first_label_overhang = (first_label_height - port_height) / 2.0;
                            (
                                f64::max(0.0, first_label_overhang),
                                label_height - first_label_overhang - port_height,
                            )
                        };
                        let target = &mut self.port_contexts[context].margin;
                        target.top = margin.0;
                        target.bottom = margin.1;
                    }
                } else {
                    self.port_contexts[context].margin.bottom =
                        self.node.ports.label_port_vertical + label_height;
                }
            } else if fixed {
                let bounds = Self::labels_bounds(port);
                let margin = &mut self.port_contexts[context].margin;
                if bounds.y < 0.0 {
                    margin.top = -bounds.y;
                }
                if bounds.y + bounds.height > port.size.height {
                    margin.bottom = bounds.y + bounds.height - port.size.height;
                }
            }
        }
    }

    /// The label ids of a label cell.
    fn cell_labels(&self, cell: CellId) -> &[usize] {
        match &self.cells.cells[cell].kind {
            Kind::Label { labels, .. } => labels,
            _ => &[],
        }
    }

    /// Source: `NodeLabelAndSizeUtilities.setupMinimumClientAreaSize` and
    /// `getMinimumClientAreaSize`.
    fn setup_minimum_client_area_size(&mut self) {
        let sizing = self.node.sizing;
        if sizing
            .options
            .contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
            && let Some(minimum) = sizing.effective_minimum()
            && let Kind::Grid {
                center_cell_minimum_size,
                ..
            } = &mut self.cells.cells[self.inside_node_label_container].kind
        {
            *center_cell_minimum_size = Some(minimum);
        }
    }

    /// Source: `CellSystemConfigurator.configureCellSystemSizeContributions`.
    fn configure_size_contributions(&mut self) {
        let sizing = self.node.sizing;
        if sizing.constraints.is_empty() {
            return;
        }
        if sizing.constraints.contains(SizeConstraints::PORTS) {
            let [north, east, south, west] = self.inside_port_label_cells;
            self.cells.cells[north].contributes_to_minimum_width = true;
            self.cells.cells[south].contributes_to_minimum_width = true;
            let free = !matches!(
                self.node.ports.constraints,
                PortConstraints::FixedRatio | PortConstraints::FixedPos
            );
            self.cells.cells[east].contributes_to_minimum_height = free;
            self.cells.cells[west].contributes_to_minimum_height = free;
            self.cells.cells[self.node_container_middle_row].contributes_to_minimum_height = free;
            if sizing.constraints.contains(SizeConstraints::PORT_LABELS) {
                self.cells.cells[north].contributes_to_minimum_height = true;
                self.cells.cells[south].contributes_to_minimum_height = true;
                self.cells.cells[east].contributes_to_minimum_width = true;
                self.cells.cells[west].contributes_to_minimum_width = true;
                self.cells.cells[self.node_container_middle_row].contributes_to_minimum_width =
                    true;
            }
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
            if sizing.constraints.contains(SizeConstraints::PORTS)
                && matches!(
                    self.node.ports.constraints,
                    PortConstraints::FixedRatio | PortConstraints::FixedPos
                )
            {
                height = f64::max(
                    height,
                    self.cells.minimum_height(self.inside_cell(PortSide::East)),
                );
                height = f64::max(
                    height,
                    self.cells.minimum_height(self.inside_cell(PortSide::West)),
                );
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

    /// Source: `CellSystemConfigurator.updateVerticalInsidePortLabelCellPadding`.
    fn update_vertical_inside_port_label_cell_padding(&mut self) {
        if matches!(
            self.node.ports.constraints,
            PortConstraints::FixedRatio | PortConstraints::FixedPos
        ) {
            return;
        }
        let container_padding = self.cells.cells[self.node_container].padding;
        let top_border_offset = container_padding.top
            + self.cells.minimum_height(self.inside_cell(PortSide::North))
            + self.label_cell_spacing;
        let bottom_border_offset = container_padding.bottom
            + self.cells.minimum_height(self.inside_cell(PortSide::South))
            + self.label_cell_spacing;
        let east = self.inside_cell(PortSide::East);
        let west = self.inside_cell(PortSide::West);
        let east_padding = self.cells.cells[east].padding;
        let west_padding = self.cells.cells[west].padding;
        let mut top = f64::max(0.0, east_padding.top - top_border_offset);
        top = f64::max(top, west_padding.top - top_border_offset);
        let mut bottom = f64::max(0.0, east_padding.bottom - bottom_border_offset);
        bottom = f64::max(bottom, west_padding.bottom - bottom_border_offset);
        for cell in [east, west] {
            self.cells.cells[cell].padding.top = top;
            self.cells.cells[cell].padding.bottom = bottom;
        }
    }

    /// Source: `NodeLabelAndSizeUtilities.offsetSouthernPortsByNodeSize`.
    fn offset_southern_ports_by_node_size(&mut self) {
        let height = self.node_size.height;
        for context in self.side_contexts(PortSide::South) {
            self.port_contexts[context].position.y += height;
        }
    }

    /// Source: `PortPlacementCalculator.placeHorizontalPorts` and `placeVerticalPorts`.
    fn place_ports(&mut self, horizontal: bool) {
        let sides = if horizontal {
            [PortSide::North, PortSide::South]
        } else {
            [PortSide::East, PortSide::West]
        };
        for side in sides {
            match self.node.ports.constraints {
                PortConstraints::FixedPos => {
                    for context in self.side_contexts(side) {
                        let port = self.port(context);
                        if horizontal {
                            self.port_contexts[context].position.y = horizontal_port_y(port);
                        } else {
                            self.port_contexts[context].position.x =
                                vertical_port_x(port, self.node_size.width);
                        }
                    }
                }
                PortConstraints::FixedRatio => {
                    for context in self.side_contexts(side) {
                        let port = self.port(context);
                        let position = &mut self.port_contexts[context].position;
                        if horizontal {
                            position.x = self.node_size.width * port.ratio_or_position;
                            position.y = horizontal_port_y(port);
                        } else {
                            position.x = vertical_port_x(port, self.node_size.width);
                            position.y = self.node_size.height * port.ratio_or_position;
                        }
                    }
                }
                _ => self.place_free_ports(side),
            }
        }
    }

    /// Source: `PortPlacementCalculator.placeHorizontalFreePorts` and `placeVerticalFreePorts`.
    fn place_free_ports(&mut self, side: PortSide) {
        let contexts = self.side_contexts(side);
        if contexts.is_empty() {
            return;
        }
        let horizontal = is_horizontal_side(side);
        let cell = self.inside_cell(side);
        let rect = self.cells.cells[cell].rect;
        let padding = self.cells.cells[cell].padding;
        let content = self.content_of(cell);
        let mut alignment = self.node.ports.alignment;
        let (available_space, mut calculated, mut current) = if horizontal {
            (
                rect.width - padding.left - padding.right,
                content.width,
                rect.x + padding.left,
            )
        } else {
            (
                rect.height - padding.top - padding.bottom,
                content.height,
                rect.y + padding.top,
            )
        };
        let port_port_spacing = self.node.ports.port_port_spacing;
        let mut space_between_ports = port_port_spacing;
        let count = contexts.len() as f64;
        // `modifiedPortPlacementSize`.
        let modified = |alignment: PortAlignment, size: f64| {
            if alignment == PortAlignment::Distributed {
                size - 2.0 * port_port_spacing
            } else {
                size
            }
        };
        if matches!(
            alignment,
            PortAlignment::Distributed | PortAlignment::Justified
        ) && contexts.len() == 1
        {
            calculated = modified(alignment, calculated);
            alignment = PortAlignment::Center;
        }
        if available_space < calculated
            && !self
                .node
                .sizing
                .options
                .contains(SizeOptions::PORTS_OVERHANG)
        {
            // With one port and an alignment other than distributed, Java divides by zero. The
            // spacing it gets is never used, because it only goes after the last port.
            if alignment == PortAlignment::Distributed {
                space_between_ports += (available_space - calculated) / (count + 1.0);
                current += space_between_ports;
            } else {
                space_between_ports += (available_space - calculated) / (count - 1.0);
            }
        } else {
            if available_space < calculated {
                calculated = modified(alignment, calculated);
                alignment = PortAlignment::Center;
            }
            match alignment {
                PortAlignment::Begin => {}
                PortAlignment::Center => current += (available_space - calculated) / 2.0,
                PortAlignment::End => current += available_space - calculated,
                PortAlignment::Distributed => {
                    let additional = (available_space - calculated) / (count + 1.0);
                    space_between_ports += f64::max(0.0, additional);
                    current += space_between_ports;
                }
                PortAlignment::Justified => {
                    let additional = (available_space - calculated) / (count - 1.0);
                    space_between_ports += f64::max(0.0, additional);
                }
            }
        }
        let node_width = self.node_size.width;
        for context in contexts {
            let port = self.port(context);
            let margin = self.port_contexts[context].margin;
            let position = &mut self.port_contexts[context].position;
            if horizontal {
                position.x = current + margin.left;
                position.y = horizontal_port_y(port);
                current += margin.left + port.size.width + margin.right + space_between_ports;
            } else {
                position.x = vertical_port_x(port, node_width);
                position.y = current + margin.top;
                current += margin.top + port.size.height + margin.bottom + space_between_ports;
            }
        }
    }

    /// Source: `PortLabelPlacementCalculator.placeHorizontalPortLabels` and
    /// `placeVerticalPortLabels`.
    fn place_port_labels(&mut self, horizontal: bool) {
        let sides = if horizontal {
            [PortSide::North, PortSide::South]
        } else {
            [PortSide::East, PortSide::West]
        };
        for side in sides {
            // `placePortLabels`.
            let constrained = !self
                .node
                .sizing
                .constraints
                .contains(SizeConstraints::PORT_LABELS)
                || self.node.ports.constraints == PortConstraints::FixedPos;
            let placement = self.node.ports.label_placement;
            if placement.contains(PortLabelPlacement::INSIDE) {
                if constrained {
                    self.constrained_inside_port_label_placement(side);
                } else {
                    self.simple_inside_port_label_placement(side);
                }
            } else if placement.contains(PortLabelPlacement::OUTSIDE) {
                if constrained {
                    self.constrained_outside_port_label_placement(side);
                } else {
                    self.simple_outside_port_label_placement(side);
                }
            }
        }
    }

    /// The label cell of a port context, when it holds labels: `portLabelCell != null &&
    /// portLabelCell.hasLabels()`.
    fn filled_label_cell(&self, context: usize) -> Option<CellId> {
        self.port_contexts[context]
            .label_cell
            .filter(|&cell| !self.cell_labels(cell).is_empty())
    }

    /// Sets the size of a port label cell to its minimum size, and its alignments.
    fn size_label_cell(&mut self, cell: CellId, horizontal: HAlign, vertical: VAlign) -> Rect {
        let width = self.cells.minimum_width(cell);
        let height = self.cells.minimum_height(cell);
        let data = &mut self.cells.cells[cell];
        data.rect.width = width;
        data.rect.height = height;
        set_label_alignment(data, horizontal, vertical);
        data.rect
    }

    /// The height of the label next to the port that `labelsNextToPort` centres: all labels, or
    /// only the first.
    fn centred_label_height(&self, cell: CellId) -> f64 {
        if self.node.ports.treat_as_group {
            self.cells.cells[cell].rect.height
        } else {
            self.cells.label_sizes[self.cell_labels(cell)[0]].height
        }
    }

    /// Source: `PortLabelPlacementCalculator.simpleInsidePortLabelPlacement`.
    fn simple_inside_port_label_placement(&mut self, side: PortSide) {
        let mut inside_area_height: f64 = 0.0;
        let label_border_offset = self.port_label_border_offset(side);
        let spacing_horizontal = self.node.ports.label_port_horizontal;
        let spacing_vertical = self.node.ports.label_port_vertical;
        for context in self.side_contexts(side) {
            let Some(cell) = self.filled_label_cell(context) else {
                continue;
            };
            let port = self.port(context);
            let next_to_port = self.port_contexts[context].labels_next_to_port;
            let port_border_offset = port.border_offset.unwrap_or(0.0);
            let (h, v) = match side {
                PortSide::North | PortSide::South => (
                    HAlign::Center,
                    if side == PortSide::North {
                        VAlign::Top
                    } else {
                        VAlign::Bottom
                    },
                ),
                PortSide::East => (HAlign::Right, VAlign::Center),
                _ => (HAlign::Left, VAlign::Center),
            };
            let mut rect = self.size_label_cell(cell, h, v);
            match side {
                PortSide::North | PortSide::South => {
                    rect.x = if next_to_port {
                        (port.size.width - rect.width) / 2.0
                    } else {
                        port.size.width + spacing_horizontal
                    };
                    rect.y = if side == PortSide::North {
                        port.size.height + port_border_offset + label_border_offset
                    } else {
                        -port_border_offset - label_border_offset - rect.height
                    };
                }
                _ => {
                    rect.x = if side == PortSide::East {
                        -port_border_offset - label_border_offset - rect.width
                    } else {
                        port.size.width + port_border_offset + label_border_offset
                    };
                    rect.y = if next_to_port {
                        (port.size.height - self.centred_label_height(cell)) / 2.0
                    } else {
                        port.size.height + spacing_vertical
                    };
                }
            }
            self.cells.cells[cell].rect = rect;
            if is_horizontal_side(side) {
                inside_area_height = f64::max(inside_area_height, rect.height);
            }
        }
        if inside_area_height > 0.0 {
            let inside = self.inside_cell(side);
            self.atomic_content(inside).height = inside_area_height;
        }
    }

    /// Source: `PortLabelPlacementCalculator.portLabelBorderOffsetForPortSide`.
    fn port_label_border_offset(&self, side: PortSide) -> f64 {
        let padding = self.cells.cells[self.node_container].padding;
        let ports = &self.node.ports;
        match side {
            PortSide::North => padding.top + ports.label_port_vertical,
            PortSide::South => padding.bottom + ports.label_port_vertical,
            PortSide::East => padding.right + ports.label_port_horizontal,
            PortSide::West => padding.left + ports.label_port_horizontal,
            PortSide::Undefined => 0.0,
        }
    }

    /// Source: `PortLabelPlacementCalculator.constrainedInsidePortLabelPlacement`.
    fn constrained_inside_port_label_placement(&mut self, side: PortSide) {
        if !is_horizontal_side(side) {
            self.simple_inside_port_label_placement(side);
            return;
        }
        let north = side == PortSide::North;
        let direction = if north {
            OverlapRemovalDirection::Down
        } else {
            OverlapRemovalDirection::Up
        };
        let vertical = if north { VAlign::Top } else { VAlign::Bottom };
        let inside = self.inside_cell(side);
        let container_rect = self.cells.cells[inside].rect;
        let container_padding = self.cells.cells[inside].padding;
        let ports = &self.node.ports;
        let spacing_vertical = ports.label_port_vertical;
        // `ElkMath.maxd`.
        let left_border = container_rect.x
            + container_padding
                .left
                .max(ports.surrounding.left)
                .max(self.node.label_node_spacing);
        let right_border = container_rect.x + container_rect.width
            - container_padding
                .right
                .max(ports.surrounding.right)
                .max(self.node.label_node_spacing);
        let mut rectangles = Vec::new();
        let mut start = if north {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
        let contexts = self.side_contexts(side);
        for &context in &contexts {
            let Some(cell) = self.filled_label_cell(context) else {
                continue;
            };
            let port = self.port(context);
            let position = self.port_contexts[context].position;
            let mut rect = self.size_label_cell(cell, HAlign::Right, vertical);
            center_port_label(&mut rect, position, port.size, left_border, right_border);
            self.cells.cells[cell].rect = rect;
            rectangles.push(cell);
            start = if north {
                f64::max(start, position.y + port.size.height)
            } else {
                f64::min(start, position.y)
            };
        }
        start += if north {
            spacing_vertical
        } else {
            -spacing_vertical
        };
        let strip_height = remove_overlaps(
            &mut self.cells,
            &rectangles,
            direction,
            spacing_vertical,
            start,
        );
        if strip_height > 0.0 {
            self.atomic_content(inside).height = strip_height;
        }
        self.make_label_cells_relative(&contexts);
    }

    /// The overlap removal works in node coordinates; label cells are relative to their port.
    fn make_label_cells_relative(&mut self, contexts: &[usize]) {
        for &context in contexts {
            let Some(cell) = self.filled_label_cell(context) else {
                continue;
            };
            let position = self.port_contexts[context].position;
            let rect = &mut self.cells.cells[cell].rect;
            rect.x -= position.x;
            rect.y -= position.y;
        }
    }

    /// Source: `PortLabelPlacementCalculator.simpleOutsidePortLabelPlacement`.
    fn simple_outside_port_label_placement(&mut self, side: PortSide) {
        let contexts = self.side_contexts(side);
        let mut place_first_differently = self.is_first_outside_port_label_placed_differently(side);
        let always_above = self
            .node
            .ports
            .label_placement
            .contains(PortLabelPlacement::ALWAYS_OTHER_SAME_SIDE);
        let spacing_horizontal = self.node.ports.label_port_horizontal;
        let spacing_vertical = self.node.ports.label_port_vertical;
        for context in contexts {
            let Some(cell) = self.filled_label_cell(context) else {
                continue;
            };
            let port_size = self.port(context).size;
            let next_to_port = self.port_contexts[context].labels_next_to_port;
            let other_side = place_first_differently || always_above;
            let (h, v) = match side {
                PortSide::North | PortSide::South => (
                    if next_to_port {
                        HAlign::Center
                    } else if other_side {
                        HAlign::Right
                    } else {
                        HAlign::Left
                    },
                    if side == PortSide::North {
                        VAlign::Bottom
                    } else {
                        VAlign::Top
                    },
                ),
                _ => (
                    if side == PortSide::East {
                        HAlign::Left
                    } else {
                        HAlign::Right
                    },
                    if next_to_port {
                        VAlign::Center
                    } else if other_side {
                        VAlign::Bottom
                    } else {
                        VAlign::Top
                    },
                ),
            };
            let mut rect = self.size_label_cell(cell, h, v);
            match side {
                PortSide::North | PortSide::South => {
                    rect.x = if next_to_port {
                        (port_size.width - rect.width) / 2.0
                    } else if other_side {
                        -rect.width - spacing_horizontal
                    } else {
                        port_size.width + spacing_horizontal
                    };
                    rect.y = if side == PortSide::North {
                        -rect.height - spacing_vertical
                    } else {
                        port_size.height + spacing_vertical
                    };
                }
                _ => {
                    rect.y = if next_to_port {
                        (port_size.height - self.centred_label_height(cell)) / 2.0
                    } else if other_side {
                        -rect.height - spacing_vertical
                    } else {
                        port_size.height + spacing_vertical
                    };
                    rect.x = if side == PortSide::East {
                        port_size.width + spacing_horizontal
                    } else {
                        -rect.width - spacing_horizontal
                    };
                }
            }
            self.cells.cells[cell].rect = rect;
            place_first_differently = false;
        }
    }

    /// Source: `NodeLabelAndSizeUtilities.isFirstOutsidePortLabelPlacedDifferently`.
    fn is_first_outside_port_label_placed_differently(&self, side: PortSide) -> bool {
        let contexts = self.side_contexts(side);
        if contexts.len() < 2 {
            return false;
        }
        let placement = self.node.ports.label_placement;
        !self.port_contexts[contexts[0]].labels_next_to_port
            && !placement.contains(PortLabelPlacement::ALWAYS_SAME_SIDE)
            && (contexts.len() == 2 || placement.contains(PortLabelPlacement::SPACE_EFFICIENT))
    }

    /// Source: `PortLabelPlacementCalculator.constrainedOutsidePortLabelPlacement`.
    fn constrained_outside_port_label_placement(&mut self, side: PortSide) {
        let contexts = self.side_contexts(side);
        if contexts.len() <= 2 || !is_horizontal_side(side) {
            self.simple_outside_port_label_placement(side);
            return;
        }
        let north = side == PortSide::North;
        let mut special_needs = self
            .node
            .ports
            .label_placement
            .contains(PortLabelPlacement::SPACE_EFFICIENT);
        let direction = if north {
            OverlapRemovalDirection::Up
        } else {
            OverlapRemovalDirection::Down
        };
        let vertical = if north { VAlign::Bottom } else { VAlign::Top };
        let spacing_horizontal = self.node.ports.label_port_horizontal;
        let spacing_vertical = self.node.ports.label_port_vertical;
        let mut rectangles = Vec::new();
        let mut start = if north {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
        for &context in &contexts {
            let Some(cell) = self.filled_label_cell(context) else {
                continue;
            };
            let port_size = self.port(context).size;
            let position = self.port_contexts[context].position;
            let mut rect = self.size_label_cell(cell, HAlign::Right, vertical);
            if special_needs {
                rect.x = position.x - rect.width - spacing_horizontal;
                special_needs = false;
            } else {
                rect.x = position.x + port_size.width + spacing_horizontal;
            }
            self.cells.cells[cell].rect = rect;
            rectangles.push(cell);
            start = if north {
                f64::min(start, position.y)
            } else {
                f64::max(start, position.y + port_size.height)
            };
        }
        start += if north {
            -spacing_vertical
        } else {
            spacing_vertical
        };
        // `withGap(portLabelSpacingVertical, portLabelSpacingHorizontal)`: the vertical gap the
        // greedy strategy reads is the horizontal label spacing.
        remove_overlaps(
            &mut self.cells,
            &rectangles,
            direction,
            spacing_horizontal,
            start,
        );
        self.make_label_cells_relative(&contexts);
    }

    /// Source: `LabelPlacer.placeLabels`.
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
        let mut positions = vec![None; self.cells.label_sizes.len()];
        // `nodeLabelCells` is an `EnumMap`, so its values come in location order.
        let mut label_cells = self.label_cells.clone();
        label_cells.sort_by_key(|(location, _)| location_order(*location));
        for (_, cell) in label_cells {
            self.cells.apply_label_layout(cell, &mut positions);
        }
        for context in &self.port_contexts {
            if let Some(cell) = context.label_cell {
                self.cells.apply_label_layout(cell, &mut positions);
            }
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

/// Source: `PortPlacementCalculator.calculateHorizontalPortYCoordinate`.
fn horizontal_port_y(port: &CellPort) -> f64 {
    let offset = port.border_offset.unwrap_or(0.0);
    if port.side == PortSide::North {
        -port.size.height - offset
    } else {
        offset
    }
}

/// Source: `PortPlacementCalculator.calculateVerticalPortXCoordinate`.
fn vertical_port_x(port: &CellPort, node_width: f64) -> f64 {
    let offset = port.border_offset.unwrap_or(0.0);
    if port.side == PortSide::West {
        -port.size.width - offset
    } else {
        node_width + offset
    }
}

/// Source: `PortLabelPlacementCalculator.centerPortLabel`.
fn center_port_label(rect: &mut Rect, position: LPoint, port_size: LSize, min_x: f64, max_x: f64) {
    rect.x = position.x - (rect.width - port_size.width) / 2.0;
    let actual_min_x = f64::min(min_x, position.x);
    let actual_max_x = f64::max(max_x, position.x + port_size.width);
    if rect.x < actual_min_x {
        rect.x = actual_min_x;
    } else if rect.x + rect.width > actual_max_x {
        rect.x = actual_max_x - rect.width;
    }
}

/// Source: `LabelCell.setHorizontalAlignment` and `setVerticalAlignment`.
fn set_label_alignment(cell: &mut Cell, h: HAlign, v: VAlign) {
    if let Kind::Label {
        horizontal,
        vertical,
        ..
    } = &mut cell.kind
    {
        *horizontal = h;
        *vertical = v;
    }
}

/// Source: `RectangleStripOverlapRemover.OverlapRemovalDirection`, the two directions the port
/// labels use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlapRemovalDirection {
    Up,
    Down,
}

/// Moves the rectangles of the given cells apart along a strip, and gives the strip's size.
///
/// Source: `RectangleStripOverlapRemover.removeOverlaps` with its default strategy,
/// `GreedyRectangleStripOverlapRemover`. For up and down, `importRectangle` hands back the
/// rectangle itself, so the cells' own rectangles move.
fn remove_overlaps(
    cells: &mut Cells,
    rectangles: &[CellId],
    direction: OverlapRemovalDirection,
    vertical_gap: f64,
    start: f64,
) -> f64 {
    let rect = |cells: &Cells, node: usize| cells.cells[rectangles[node]].rect;
    // `List.sort` is stable, in Java and in GWT.
    let mut nodes: Vec<usize> = (0..rectangles.len()).collect();
    nodes.sort_by(|&a, &b| rect(cells, a).x.total_cmp(&rect(cells, b).x));
    // `computeOverlaps`. The scanline set is a `TreeSet` ordered by the right border: a
    // rectangle whose right border equals one already in the set is not added.
    let right = |cells: &Cells, node: usize| {
        let r = rect(cells, node);
        r.x + r.width
    };
    let mut overlapping: Vec<Vec<usize>> = vec![Vec::new(); rectangles.len()];
    let mut intersecting: Vec<usize> = Vec::new();
    for &current in &nodes {
        let scanline = rect(cells, current).x;
        while let Some(&first) = intersecting.first() {
            if right(cells, first) < scanline {
                intersecting.remove(0);
            } else {
                break;
            }
        }
        for &other in &intersecting {
            overlapping[other].push(current);
            overlapping[current].push(other);
        }
        let key = right(cells, current);
        match intersecting.binary_search_by(|&other| right(cells, other).total_cmp(&key)) {
            Ok(_) => {}
            Err(at) => intersecting.insert(at, current),
        }
    }
    // `GreedyRectangleStripOverlapRemover.removeOverlaps`.
    let mut placed = vec![false; rectangles.len()];
    let mut strip_size: f64 = 0.0;
    for &current in &nodes {
        let mut y_pos = 0.0;
        let mut others = std::mem::take(&mut overlapping[current]);
        others.sort_by(|&a, &b| rect(cells, a).y.total_cmp(&rect(cells, b).y));
        let height = rect(cells, current).height;
        for &other in &others {
            if placed[other] {
                let other_rect = rect(cells, other);
                if y_pos < other_rect.y + other_rect.height + vertical_gap
                    && y_pos + height + vertical_gap > other_rect.y
                {
                    y_pos = other_rect.y + other_rect.height + vertical_gap;
                }
            }
        }
        overlapping[current] = others;
        cells.cells[rectangles[current]].rect.y = y_pos;
        placed[current] = true;
        strip_size = f64::max(strip_size, y_pos + height);
    }
    // `exportRectangle`.
    for &node in &nodes {
        let r = &mut cells.cells[rectangles[node]].rect;
        match direction {
            OverlapRemovalDirection::Up => r.y = start - r.height - r.y,
            OverlapRemovalDirection::Down => r.y += start,
        }
    }
    strip_size
}

fn set_strip_cell(cells: &mut Cells, container: CellId, area: usize, cell: CellId) {
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
    if sizing
        .options
        .contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
    {
        return None;
    }
    // `NodeLabelAndSizeUtilities.getMinimumNodeOrClientAreaSize`, behind the constraint check.
    sizing.effective_minimum()
}

/// Sizes a node and places its ports and labels.
///
/// Source: `NodeLabelAndSizeCalculator.process(graph, node, applyStuff, ignoreInsidePortLabels)`.
/// With `apply` false, ELK stops once it knows the size, so no port or label moves.
pub(crate) fn process(
    node: &CellNode,
    horizontal_layout_mode: bool,
    apply: bool,
    ignore_inside_port_labels: bool,
) -> CellResult {
    let mut context = Context::new(node, false, horizontal_layout_mode);
    context.create_port_contexts(ignore_inside_port_labels);
    context.create_inside_port_label_cells();
    context.setup_minimum_client_area_size();
    context.setup_node_padding_for_ports_with_offset();
    context.calculate_port_placement_size(true);
    context.calculate_port_placement_size(false);
    context.configure_size_contributions();
    context.set_node_width();
    context.place_ports(true);
    context.place_port_labels(true);
    context.update_vertical_inside_port_label_cell_padding();
    context.set_node_height();
    if !apply {
        return CellResult {
            size: context.node_size,
            label_positions: vec![None; node.labels.len()],
            padding: None,
            port_positions: Vec::new(),
            port_label_positions: Vec::new(),
        };
    }
    context.offset_southern_ports_by_node_size();
    context.place_ports(false);
    context.place_port_labels(false);
    let mut positions = context.place_labels();
    let padding = context.node_padding();
    let mut port_positions: Vec<LPoint> = node.ports.ports.iter().map(|p| p.position).collect();
    for port_context in &context.port_contexts {
        port_positions[port_context.port] = port_context.position;
    }
    let mut port_label_positions = Vec::with_capacity(node.ports.ports.len());
    let mut rest = positions.split_off(node.labels.len());
    for port in &node.ports.ports {
        let tail = rest.split_off(port.labels.len());
        port_label_positions.push(rest);
        rest = tail;
    }
    CellResult {
        size: context.node_size,
        label_positions: positions,
        padding,
        port_positions,
        port_label_positions,
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
mod tests;
