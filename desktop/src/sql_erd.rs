//! Relationship diagram (ERD) for the SQL Workspace: a simple grid layout of
//! table "cards" with cubic-bezier connector lines drawn between foreign-key
//! columns, rendered with `iced::widget::canvas`. No force-directed layout —
//! a grid is a perfectly adequate v1 per the design doc.

use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{mouse, Color, Pixels, Point, Rectangle, Renderer, Size, Theme};

use og_testdesk_core::sql::models::{SqlForeignKeyInfo, SqlRelationshipSchema, SqlTableInfo};

const CARD_WIDTH: f32 = 220.0;
const CARD_GAP_X: f32 = 60.0;
const CARD_GAP_Y: f32 = 50.0;
const TITLE_HEIGHT: f32 = 26.0;
const ROW_HEIGHT: f32 = 18.0;
const CARD_PADDING: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct ColumnRow {
    pub name: String,
    pub primary_key: bool,
    pub is_fk: bool,
    pub bounds: Rectangle,
}

#[derive(Debug, Clone)]
pub struct CardLayout {
    pub table: String,
    pub bounds: Rectangle,
    pub columns: Vec<ColumnRow>,
}

/// Lay out table cards on a simple `ceil(sqrt(n))`-column grid. Card height
/// is derived from each table's column count, so rows in the grid can have
/// uneven heights; each grid row's height is the tallest card in that row.
pub fn layout_cards(tables: &[&SqlTableInfo]) -> Vec<CardLayout> {
    if tables.is_empty() {
        return Vec::new();
    }

    let grid_cols = (tables.len() as f32).sqrt().ceil().max(1.0) as usize;
    let mut layouts = Vec::with_capacity(tables.len());
    let mut row_heights: Vec<f32> = Vec::new();

    for (index, table) in tables.iter().enumerate() {
        let row = index / grid_cols;
        let card_height =
            TITLE_HEIGHT + table.columns.len() as f32 * ROW_HEIGHT + CARD_PADDING * 2.0;
        if row_heights.len() <= row {
            row_heights.push(card_height);
        } else if card_height > row_heights[row] {
            row_heights[row] = card_height;
        }
    }

    for (index, table) in tables.iter().enumerate() {
        let col = index % grid_cols;
        let row = index / grid_cols;

        let x = col as f32 * (CARD_WIDTH + CARD_GAP_X) + CARD_GAP_X;
        let y: f32 = row_heights[..row].iter().sum::<f32>()
            + row as f32 * CARD_GAP_Y
            + CARD_GAP_Y;

        let card_height =
            TITLE_HEIGHT + table.columns.len() as f32 * ROW_HEIGHT + CARD_PADDING * 2.0;

        let mut columns = Vec::with_capacity(table.columns.len());
        for (col_index, column) in table.columns.iter().enumerate() {
            let row_y = y + TITLE_HEIGHT + CARD_PADDING + col_index as f32 * ROW_HEIGHT;
            columns.push(ColumnRow {
                name: column.name.clone(),
                primary_key: column.primary_key,
                is_fk: false, // filled in by caller once relationships are known
                bounds: Rectangle::new(Point::new(x, row_y), Size::new(CARD_WIDTH, ROW_HEIGHT)),
            });
        }

        layouts.push(CardLayout {
            table: table.name.clone(),
            bounds: Rectangle::new(Point::new(x, y), Size::new(CARD_WIDTH, card_height)),
            columns,
        });
    }

    layouts
}

/// Mark columns that are the "from" side of a foreign key as `is_fk`.
pub fn mark_fk_columns(layouts: &mut [CardLayout], relationships: &[SqlForeignKeyInfo]) {
    for layout in layouts.iter_mut() {
        for column in layout.columns.iter_mut() {
            column.is_fk = relationships
                .iter()
                .any(|rel| rel.from_table == layout.table && rel.from_column == column.name);
        }
    }
}

/// Cubic-bezier control points for a connector between two points: a
/// horizontal S-curve whose control-point offset scales with distance so
/// short and long connectors both look reasonable.
pub fn bezier_control_points(from: Point, to: Point) -> (Point, Point) {
    let dx = ((to.x - from.x).abs() * 0.5).max(40.0);
    let c1 = Point::new(from.x + dx, from.y);
    let c2 = Point::new(to.x - dx, to.y);
    (c1, c2)
}

/// A table matches a search term if its name or any of its column names
/// contains it (case-insensitive substring match). Empty term matches all.
pub fn table_matches_filter(table: &SqlTableInfo, term: &str) -> bool {
    if term.trim().is_empty() {
        return true;
    }
    let term = term.to_lowercase();
    table.name.to_lowercase().contains(&term)
        || table
            .columns
            .iter()
            .any(|c| c.name.to_lowercase().contains(&term))
}

fn find_column_anchor(layouts: &[CardLayout], table: &str, column: &str) -> Option<(Point, Point)> {
    let card = layouts.iter().find(|c| c.table == table)?;
    let row = card.columns.iter().find(|c| c.name == column)?;
    let left = Point::new(row.bounds.x, row.bounds.y + row.bounds.height / 2.0);
    let right = Point::new(
        row.bounds.x + row.bounds.width,
        row.bounds.y + row.bounds.height / 2.0,
    );
    Some((left, right))
}

#[derive(Debug, Clone)]
pub enum ErdMessage {
    TableClicked(String),
    FkColumnClicked(String, String),
}

pub struct ErdProgram<'a> {
    pub schema: &'a SqlRelationshipSchema,
    pub layout: Vec<CardLayout>,
    pub highlighted: &'a Option<String>,
}

impl<'a> canvas::Program<ErdMessage> for ErdProgram<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<ErdMessage>) {
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return (canvas::event::Status::Ignored, None);
        };
        let Some(position) = cursor.position_in(bounds) else {
            return (canvas::event::Status::Ignored, None);
        };

        for card in &self.layout {
            if !card.bounds.contains(position) {
                continue;
            }
            for column in &card.columns {
                if column.is_fk && column.bounds.contains(position) {
                    if let Some(rel) = self
                        .schema
                        .relationships
                        .iter()
                        .find(|r| r.from_table == card.table && r.from_column == column.name)
                    {
                        return (
                            canvas::event::Status::Captured,
                            Some(ErdMessage::FkColumnClicked(
                                rel.to_table.clone(),
                                rel.to_column.clone(),
                            )),
                        );
                    }
                }
            }
            return (
                canvas::event::Status::Captured,
                Some(ErdMessage::TableClicked(card.table.clone())),
            );
        }

        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        for rel in &self.schema.relationships {
            let Some((from_left, from_right)) =
                find_column_anchor(&self.layout, &rel.from_table, &rel.from_column)
            else {
                continue;
            };
            let Some((to_left, to_right)) =
                find_column_anchor(&self.layout, &rel.to_table, &rel.to_column)
            else {
                continue;
            };
            // Connect from the FK column's right edge to the PK column's
            // left edge if the target is to the right, else left-to-left.
            let (from_point, to_point) = if to_left.x >= from_right.x {
                (from_right, to_left)
            } else {
                (from_left, to_right)
            };
            let (c1, c2) = bezier_control_points(from_point, to_point);
            let path = Path::new(|p| {
                p.move_to(from_point);
                p.bezier_curve_to(c1, c2, to_point);
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(Color::from_rgb8(0x62, 0x72, 0xa4))
                    .with_width(1.5),
            );
        }

        for card in &self.layout {
            let highlighted = self.highlighted.as_deref() == Some(card.table.as_str());
            let border_color = if highlighted {
                Color::from_rgb8(0x4d, 0xa6, 0xff)
            } else {
                Color::from_rgb8(0x55, 0x55, 0x55)
            };

            let card_rect = Path::rectangle(
                Point::new(card.bounds.x, card.bounds.y),
                Size::new(card.bounds.width, card.bounds.height),
            );
            frame.fill(&card_rect, Color::from_rgb8(0x22, 0x22, 0x22));
            frame.stroke(
                &card_rect,
                Stroke::default()
                    .with_color(border_color)
                    .with_width(if highlighted { 2.5 } else { 1.0 }),
            );

            frame.fill_text(Text {
                content: card.table.clone(),
                position: Point::new(card.bounds.x + 8.0, card.bounds.y + 6.0),
                color: Color::WHITE,
                size: Pixels(14.0),
                ..Text::default()
            });

            for column in &card.columns {
                let color = if column.primary_key {
                    Color::from_rgb8(0xf1, 0xfa, 0x8c)
                } else if column.is_fk {
                    Color::from_rgb8(0x8b, 0xe9, 0xfd)
                } else {
                    Color::from_rgb8(0xdd, 0xdd, 0xdd)
                };
                let label = if column.primary_key {
                    format!("PK  {}", column.name)
                } else if column.is_fk {
                    format!("FK  {}", column.name)
                } else {
                    column.name.clone()
                };
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(column.bounds.x + 8.0, column.bounds.y + 2.0),
                    color,
                    size: Pixels(12.0),
                    ..Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(position) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if self.layout.iter().any(|card| card.bounds.contains(position)) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_testdesk_core::sql::models::SqlColumnInfo;

    fn table(name: &str, columns: &[(&str, bool)]) -> SqlTableInfo {
        SqlTableInfo {
            name: name.to_string(),
            columns: columns
                .iter()
                .map(|(col_name, pk)| SqlColumnInfo {
                    name: col_name.to_string(),
                    data_type: "text".to_string(),
                    nullable: true,
                    default: None,
                    primary_key: *pk,
                })
                .collect(),
        }
    }

    #[test]
    fn grid_layout_positions_cards_without_overlap() {
        let tables = vec![
            table("authors", &[("id", true), ("name", false)]),
            table("books", &[("id", true), ("author_id", false)]),
            table("genres", &[("id", true)]),
            table("tags", &[("id", true)]),
        ];
        let refs: Vec<&SqlTableInfo> = tables.iter().collect();
        let layouts = layout_cards(&refs);
        assert_eq!(layouts.len(), 4);

        for i in 0..layouts.len() {
            for j in (i + 1)..layouts.len() {
                let a = layouts[i].bounds;
                let b = layouts[j].bounds;
                let overlap_x = a.x < b.x + b.width && b.x < a.x + a.width;
                let overlap_y = a.y < b.y + b.height && b.y < a.y + a.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "cards {} and {} overlap: {a:?} vs {b:?}",
                    layouts[i].table,
                    layouts[j].table
                );
            }
        }
    }

    #[test]
    fn mark_fk_columns_flags_from_column_only() {
        let tables = vec![
            table("authors", &[("id", true)]),
            table("books", &[("id", true), ("author_id", false)]),
        ];
        let refs: Vec<&SqlTableInfo> = tables.iter().collect();
        let mut layouts = layout_cards(&refs);
        let relationships = vec![SqlForeignKeyInfo {
            name: "fk_books_author".to_string(),
            from_table: "books".to_string(),
            from_column: "author_id".to_string(),
            to_table: "authors".to_string(),
            to_column: "id".to_string(),
        }];
        mark_fk_columns(&mut layouts, &relationships);

        let books = layouts.iter().find(|c| c.table == "books").unwrap();
        let author_id = books.columns.iter().find(|c| c.name == "author_id").unwrap();
        assert!(author_id.is_fk);

        let authors = layouts.iter().find(|c| c.table == "authors").unwrap();
        let id = authors.columns.iter().find(|c| c.name == "id").unwrap();
        assert!(!id.is_fk);
    }

    #[test]
    fn bezier_control_points_are_between_endpoints_horizontally() {
        let from = Point::new(0.0, 10.0);
        let to = Point::new(300.0, 50.0);
        let (c1, c2) = bezier_control_points(from, to);
        assert!(c1.x > from.x && c1.x < to.x);
        assert!(c2.x > from.x && c2.x < to.x);
        assert_eq!(c1.y, from.y);
        assert_eq!(c2.y, to.y);
    }

    #[test]
    fn bezier_control_points_have_minimum_spread_for_close_points() {
        let from = Point::new(0.0, 0.0);
        let to = Point::new(5.0, 0.0);
        let (c1, _c2) = bezier_control_points(from, to);
        assert!(c1.x - from.x >= 40.0);
    }

    #[test]
    fn table_matches_filter_checks_name_and_columns() {
        let t = table("books", &[("id", true), ("author_id", false)]);
        assert!(table_matches_filter(&t, ""));
        assert!(table_matches_filter(&t, "book"));
        assert!(table_matches_filter(&t, "AUTHOR_ID"));
        assert!(!table_matches_filter(&t, "nonexistent"));
    }

    #[test]
    fn find_column_anchor_returns_none_for_missing_table_or_column() {
        let tables = vec![table("authors", &[("id", true)])];
        let refs: Vec<&SqlTableInfo> = tables.iter().collect();
        let layouts = layout_cards(&refs);
        assert!(find_column_anchor(&layouts, "missing", "id").is_none());
        assert!(find_column_anchor(&layouts, "authors", "missing").is_none());
        assert!(find_column_anchor(&layouts, "authors", "id").is_some());
    }
}
