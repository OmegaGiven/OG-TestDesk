use iced::widget::{button, checkbox, column, container, mouse_area, row, scrollable, text, text_input};
use iced::{Element, Length};
use iced_table::table;
use std::collections::HashSet;

pub type Row = Vec<String>;

const MIN_COLUMN_WIDTH: f32 = 60.0;
const MAX_COLUMN_WIDTH: f32 = 400.0;
const PIXELS_PER_CHAR: f32 = 7.0;
const WIDTH_PADDING: f32 = 24.0;

#[derive(Debug, Clone)]
pub enum GridMessage {
    ColumnResizing(usize, f32),
    ColumnResized,
    SyncHeader(scrollable::AbsoluteOffset),
    SortBy(usize),
    /// (row_index, col_index) into the currently displayed (filtered,
    /// possibly sorted) rows — use [`ResultsGrid::original_row_index`] to
    /// translate back to an index into `ResultsGrid::rows` before using it
    /// for anything that indexes the full row set (edit/FK navigation).
    CellClicked(usize, usize),
    FilterChanged(String),
    ToggleColumnsPopup,
    ToggleColumnVisibility(usize),
    WidenColumns,
}

pub struct GridColumn {
    pub index: usize,
    pub title: String,
    pub width: f32,
    pub resize_offset: Option<f32>,
    /// Columns considered foreign keys get an underline + click-to-navigate cursor.
    pub is_fk: bool,
    /// Hidden columns stay present in the table (so column indices used to
    /// look up `row_data` cells stay aligned) but render as zero-width with
    /// empty header/cell content.
    pub visible: bool,
}

impl<'a> table::Column<'a, GridMessage, iced::Theme, iced::Renderer> for GridColumn {
    type Row = Row;

    fn header(&'a self, _col_index: usize) -> Element<'a, GridMessage> {
        if !self.visible {
            return text("").into();
        }
        let label = if self.is_fk {
            format!("{} 🔗", self.title)
        } else {
            self.title.clone()
        };
        button(text(label))
            .on_press(GridMessage::SortBy(self.index))
            .width(Length::Fill)
            .into()
    }

    fn cell(&'a self, col_index: usize, row_index: usize, row_data: &'a Self::Row) -> Element<'a, GridMessage> {
        if !self.visible {
            return text("").into();
        }
        let content: Element<'a, GridMessage> = match row_data.get(col_index) {
            Some(value) if !value.is_empty() => {
                if self.is_fk {
                    text(value.clone())
                        .color(iced::Color::from_rgb8(0x4d, 0xa6, 0xff))
                        .into()
                } else {
                    text(value.clone()).into()
                }
            }
            _ => text("NULL")
                .color(iced::Color::from_rgba8(0x88, 0x88, 0x88, 0.6))
                .into(),
        };

        mouse_area(content)
            .on_press(GridMessage::CellClicked(row_index, col_index))
            .into()
    }

    fn width(&self) -> f32 {
        if self.visible { self.width } else { 0.0 }
    }

    fn resize_offset(&self) -> Option<f32> {
        self.resize_offset
    }
}

pub struct ResultsGrid {
    pub columns: Vec<GridColumn>,
    pub rows: Vec<Row>,
    pub sort: Option<(usize, bool)>,
    pub filter: String,
    /// Indices into `rows` for the rows currently matching `filter`,
    /// recomputed whenever `rows`, `sort`, or `filter` change.
    /// `GridMessage::CellClicked`'s row index is an index into this list,
    /// translated back via [`Self::original_row_index`].
    display_indices: Vec<usize>,
    /// Cached clones of `rows` at `display_indices`, kept as a field (not
    /// computed locally in `view()`) because `iced_table::table` borrows
    /// its row slice for the lifetime of the returned `Element` — a local
    /// `Vec` in `view()` would be dropped too early (same constraint
    /// `text_editor::Content` has elsewhere in this codebase).
    display_rows: Vec<Row>,
    pub columns_popup_open: bool,
    header: scrollable::Id,
    body: scrollable::Id,
}

impl ResultsGrid {
    pub fn new(headers: Vec<String>, rows: Vec<Row>) -> Self {
        Self::with_fk_columns(headers, rows, &HashSet::new())
    }

    pub fn with_fk_columns(headers: Vec<String>, rows: Vec<Row>, fk_columns: &HashSet<String>) -> Self {
        let columns = headers
            .into_iter()
            .enumerate()
            .map(|(index, title)| {
                let is_fk = fk_columns.contains(&title);
                GridColumn {
                    index,
                    title,
                    width: 160.0,
                    resize_offset: None,
                    is_fk,
                    visible: true,
                }
            })
            .collect();
        let mut grid = Self {
            columns,
            rows,
            sort: None,
            filter: String::new(),
            display_indices: Vec::new(),
            display_rows: Vec::new(),
            columns_popup_open: false,
            header: scrollable::Id::unique(),
            body: scrollable::Id::unique(),
        };
        grid.recompute_filter();
        grid
    }

    fn recompute_filter(&mut self) {
        let filter = self.filter.to_lowercase();
        self.display_indices = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                filter.is_empty() || row.iter().any(|cell| cell.to_lowercase().contains(&filter))
            })
            .map(|(i, _)| i)
            .collect();
        self.display_rows = self
            .display_indices
            .iter()
            .filter_map(|&i| self.rows.get(i).cloned())
            .collect();
    }

    /// Translates a `GridMessage::CellClicked` row index (into the
    /// currently displayed/filtered rows) back into an index into `rows`.
    pub fn original_row_index(&self, display_index: usize) -> Option<usize> {
        self.display_indices.get(display_index).copied()
    }

    /// Auto-fits every column's width to its longest current header/cell
    /// value, using a simple character-count estimate rather than real
    /// font-metrics measurement.
    pub fn widen_columns(&mut self) {
        for column in &mut self.columns {
            let header_len = column.title.chars().count();
            let max_cell_len = self
                .rows
                .iter()
                .filter_map(|row| row.get(column.index))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0);
            let max_len = header_len.max(max_cell_len) as f32;
            column.width = (max_len * PIXELS_PER_CHAR + WIDTH_PADDING).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
        }
    }

    pub fn update(&mut self, message: GridMessage) -> iced::Task<GridMessage> {
        match message {
            GridMessage::ColumnResizing(index, offset) => {
                if let Some(col) = self.columns.get_mut(index) {
                    col.resize_offset = Some(offset);
                }
                iced::Task::none()
            }
            GridMessage::ColumnResized => {
                for col in &mut self.columns {
                    if let Some(offset) = col.resize_offset.take() {
                        col.width = (col.width + offset).max(40.0);
                    }
                }
                iced::Task::none()
            }
            GridMessage::SyncHeader(offset) => scrollable::scroll_to(self.header.clone(), offset),
            GridMessage::SortBy(index) => {
                let ascending = match self.sort {
                    Some((current, asc)) if current == index => !asc,
                    _ => true,
                };
                self.rows.sort_by(|a, b| {
                    let empty = String::new();
                    let av = a.get(index).unwrap_or(&empty);
                    let bv = b.get(index).unwrap_or(&empty);
                    let ordering = match (av.parse::<f64>(), bv.parse::<f64>()) {
                        (Ok(an), Ok(bn)) => an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal),
                        _ => av.cmp(bv),
                    };
                    if ascending { ordering } else { ordering.reverse() }
                });
                self.sort = Some((index, ascending));
                self.recompute_filter();
                iced::Task::none()
            }
            // Real handling (FK navigation / start-edit) happens in
            // `SqlTab::update`, which needs schema/connection context this
            // grid doesn't have; nothing to do here.
            GridMessage::CellClicked(..) => iced::Task::none(),
            GridMessage::FilterChanged(filter) => {
                self.filter = filter;
                self.recompute_filter();
                iced::Task::none()
            }
            GridMessage::ToggleColumnsPopup => {
                self.columns_popup_open = !self.columns_popup_open;
                iced::Task::none()
            }
            GridMessage::ToggleColumnVisibility(index) => {
                if let Some(col) = self.columns.iter_mut().find(|c| c.index == index) {
                    col.visible = !col.visible;
                }
                iced::Task::none()
            }
            GridMessage::WidenColumns => {
                self.widen_columns();
                iced::Task::none()
            }
        }
    }

    /// Toolbar row: filter/search input, Columns show/hide popup, Widen
    /// columns. Kept separate from `view()` so callers (e.g. `SqlTab`) can
    /// compose it alongside export/revert/running-queries controls that
    /// live outside this widget's own knowledge (schema/connection state).
    pub fn view_toolbar(&self) -> Element<'_, GridMessage> {
        let filter_input = text_input("Filter rows...", &self.filter)
            .on_input(GridMessage::FilterChanged)
            .width(Length::Fixed(200.0));

        let columns_button = button(text("Columns")).on_press(GridMessage::ToggleColumnsPopup);
        let widen_button = button(text("Widen columns")).on_press(GridMessage::WidenColumns);

        let mut bar = row![filter_input, columns_button, widen_button].spacing(8);

        if self.columns_popup_open {
            let mut popup = column![text("Show/hide columns").size(12)].spacing(4);
            for col in &self.columns {
                popup = popup.push(
                    checkbox(col.title.clone(), col.visible)
                        .on_toggle(move |_| GridMessage::ToggleColumnVisibility(col.index)),
                );
            }
            bar = bar.push(container(popup).padding(6));
        }

        bar.into()
    }

    pub fn view(&self) -> Element<'_, GridMessage> {
        if self.columns.is_empty() {
            return container(text("Run a query to see results"))
                .width(Length::Fill)
                .into();
        }

        let table = table(
            self.header.clone(),
            self.body.clone(),
            &self.columns,
            &self.display_rows,
            GridMessage::SyncHeader,
        )
        .on_column_resize(GridMessage::ColumnResizing, GridMessage::ColumnResized)
        .min_width(600.0);

        let sort_indicator = match self.sort {
            Some((index, ascending)) => format!(
                "Sorted by column {} ({}) — {} of {} rows shown",
                index + 1,
                if ascending { "asc" } else { "desc" },
                self.display_rows.len(),
                self.rows.len(),
            ),
            None => format!(
                "Click a column header to sort — {} of {} rows shown",
                self.display_rows.len(),
                self.rows.len(),
            ),
        };

        iced::widget::column![
            row![text(sort_indicator).size(12)].padding(4),
            container(table).width(Length::Fill).height(Length::Fill),
        ]
        .into()
    }
}

#[cfg(test)]
mod phase5_tests {
    use super::*;

    fn sample_grid() -> ResultsGrid {
        ResultsGrid::new(
            vec!["id".to_string(), "name".to_string()],
            vec![
                vec!["1".to_string(), "Alice".to_string()],
                vec!["2".to_string(), "Bob".to_string()],
                vec!["3".to_string(), "Carol".to_string()],
            ],
        )
    }

    #[test]
    fn empty_filter_shows_all_rows() {
        let grid = sample_grid();
        assert_eq!(grid.display_rows.len(), 3);
    }

    #[test]
    fn filter_narrows_display_rows_case_insensitively() {
        let mut grid = sample_grid();
        grid.update(GridMessage::FilterChanged("bob".to_string()));
        assert_eq!(grid.display_rows.len(), 1);
        assert_eq!(grid.display_rows[0][1], "Bob");
    }

    #[test]
    fn filter_matches_any_cell_in_row() {
        let mut grid = sample_grid();
        grid.update(GridMessage::FilterChanged("2".to_string()));
        assert_eq!(grid.display_rows.len(), 1);
        assert_eq!(grid.display_rows[0][1], "Bob");
    }

    #[test]
    fn original_row_index_translates_through_filter() {
        let mut grid = sample_grid();
        grid.update(GridMessage::FilterChanged("carol".to_string()));
        assert_eq!(grid.display_rows.len(), 1);
        // "Carol" is row index 2 in the unfiltered `rows`, but the only
        // (index 0) row in the filtered display.
        assert_eq!(grid.original_row_index(0), Some(2));
        assert_eq!(grid.original_row_index(1), None);
    }

    #[test]
    fn clearing_filter_restores_all_rows() {
        let mut grid = sample_grid();
        grid.update(GridMessage::FilterChanged("bob".to_string()));
        assert_eq!(grid.display_rows.len(), 1);
        grid.update(GridMessage::FilterChanged(String::new()));
        assert_eq!(grid.display_rows.len(), 3);
    }

    #[test]
    fn toggle_column_visibility_flips_the_flag() {
        let mut grid = sample_grid();
        assert!(grid.columns[1].visible);
        grid.update(GridMessage::ToggleColumnVisibility(1));
        assert!(!grid.columns[1].visible);
        grid.update(GridMessage::ToggleColumnVisibility(1));
        assert!(grid.columns[1].visible);
    }

    #[test]
    fn widen_columns_grows_width_for_longer_content() {
        let mut grid = ResultsGrid::new(
            vec!["short".to_string()],
            vec![vec!["a very long cell value indeed".to_string()]],
        );
        let before = grid.columns[0].width;
        grid.widen_columns();
        assert!(grid.columns[0].width > before);
    }

    #[test]
    fn widen_columns_uses_header_when_longer_than_cells() {
        let mut grid = ResultsGrid::new(
            vec!["a_much_longer_header_than_any_cell".to_string()],
            vec![vec!["x".to_string()]],
        );
        grid.widen_columns();
        // Width should reflect the header length, not just the tiny cell.
        assert!(grid.columns[0].width > MIN_COLUMN_WIDTH);
    }

    #[test]
    fn widen_columns_clamps_to_max_width() {
        let mut grid = ResultsGrid::new(
            vec!["c".to_string()],
            vec![vec!["x".repeat(1000)]],
        );
        grid.widen_columns();
        assert_eq!(grid.columns[0].width, MAX_COLUMN_WIDTH);
    }

    #[test]
    fn sort_then_filter_keeps_indices_consistent() {
        let mut grid = sample_grid();
        grid.update(GridMessage::SortBy(1)); // sort by name ascending
        grid.update(GridMessage::FilterChanged("a".to_string())); // Alice, Carol
        assert_eq!(grid.display_rows.len(), 2);
        for i in 0..grid.display_rows.len() {
            let original = grid.original_row_index(i).unwrap();
            assert_eq!(grid.rows[original], grid.display_rows[i]);
        }
    }
}
