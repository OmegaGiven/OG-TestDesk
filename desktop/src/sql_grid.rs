use iced::widget::{button, container, mouse_area, row, scrollable, text};
use iced::{Element, Length};
use iced_table::table;
use std::collections::HashSet;

pub type Row = Vec<String>;

#[derive(Debug, Clone)]
pub enum GridMessage {
    ColumnResizing(usize, f32),
    ColumnResized,
    SyncHeader(scrollable::AbsoluteOffset),
    SortBy(usize),
    /// (row_index, col_index) into the currently displayed (possibly sorted) rows.
    CellClicked(usize, usize),
}

pub struct GridColumn {
    pub index: usize,
    pub title: String,
    pub width: f32,
    pub resize_offset: Option<f32>,
    /// Columns considered foreign keys get an underline + click-to-navigate cursor.
    pub is_fk: bool,
}

impl<'a> table::Column<'a, GridMessage, iced::Theme, iced::Renderer> for GridColumn {
    type Row = Row;

    fn header(&'a self, _col_index: usize) -> Element<'a, GridMessage> {
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
        self.width
    }

    fn resize_offset(&self) -> Option<f32> {
        self.resize_offset
    }
}

pub struct ResultsGrid {
    pub columns: Vec<GridColumn>,
    pub rows: Vec<Row>,
    pub sort: Option<(usize, bool)>,
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
                }
            })
            .collect();
        Self {
            columns,
            rows,
            sort: None,
            header: scrollable::Id::unique(),
            body: scrollable::Id::unique(),
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
                iced::Task::none()
            }
            // Real handling (FK navigation / start-edit) happens in
            // `SqlTab::update`, which needs schema/connection context this
            // grid doesn't have; nothing to do here.
            GridMessage::CellClicked(..) => iced::Task::none(),
        }
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
            &self.rows,
            GridMessage::SyncHeader,
        )
        .on_column_resize(GridMessage::ColumnResizing, GridMessage::ColumnResized)
        .min_width(600.0);

        let sort_indicator = match self.sort {
            Some((index, ascending)) => format!(
                "Sorted by column {} ({})",
                index + 1,
                if ascending { "asc" } else { "desc" }
            ),
            None => "Click a column header to sort".to_string(),
        };

        iced::widget::column![
            row![text(sort_indicator).size(12)].padding(4),
            container(table).width(Length::Fill).height(Length::Fill),
        ]
        .into()
    }
}
