use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::vte::ansi::{Color as TermColor, CursorShape, NamedColor};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

/// Copies the visible grid of `term` into `area` and returns where the cursor sits, if shown.
pub fn draw_term<L: EventListener>(term: &Term<L>, area: Rect, buf: &mut Buffer) -> Option<Position> {
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    for indexed in content.display_iter {
        if indexed.cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
            continue;
        }
        let Some(position) = area_position(area, indexed.point.line.0 + offset, indexed.point.column.0)
        else {
            continue;
        };
        if let Some(cell) = buf.cell_mut(position) {
            cell.set_symbol(&symbol(indexed.cell));
            cell.set_style(style(indexed.cell));
        }
    }
    if content.cursor.shape == CursorShape::Hidden {
        return None;
    }
    area_position(area, content.cursor.point.line.0 + offset, content.cursor.point.column.0)
}

fn area_position(area: Rect, row: i32, column: usize) -> Option<Position> {
    let row = u16::try_from(row).ok().filter(|row| *row < area.height)?;
    let column = u16::try_from(column).ok().filter(|column| *column < area.width)?;
    Some(Position::new(area.x + column, area.y + row))
}

fn symbol(cell: &TermCell) -> String {
    let base = if cell.c.is_control() { ' ' } else { cell.c };
    let mut symbol = String::from(base);
    if let Some(zerowidth) = cell.zerowidth() {
        symbol.extend(zerowidth);
    }
    symbol
}

fn style(cell: &TermCell) -> Style {
    Style::default()
        .fg(color(cell.fg))
        .bg(color(cell.bg))
        .add_modifier(modifiers(cell.flags))
}

fn modifiers(flags: Flags) -> Modifier {
    let pairs = [
        (Flags::BOLD, Modifier::BOLD),
        (Flags::ITALIC, Modifier::ITALIC),
        (Flags::DIM, Modifier::DIM),
        (Flags::ALL_UNDERLINES, Modifier::UNDERLINED),
        (Flags::INVERSE, Modifier::REVERSED),
        (Flags::HIDDEN, Modifier::HIDDEN),
        (Flags::STRIKEOUT, Modifier::CROSSED_OUT),
    ];
    pairs
        .into_iter()
        .filter(|(flag, _)| flags.intersects(*flag))
        .fold(Modifier::empty(), |all, (_, modifier)| all | modifier)
}

/// Named and indexed colors stay palette indexes, so the host terminal theme applies.
fn color(color: TermColor) -> Color {
    match color {
        TermColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        TermColor::Indexed(index) => Color::Indexed(index),
        TermColor::Named(named) => named_color(named),
    }
}

fn named_color(named: NamedColor) -> Color {
    let index = named as usize;
    if index < 16 {
        return Color::Indexed(index as u8);
    }
    let dim_black = NamedColor::DimBlack as usize;
    let dim_white = NamedColor::DimWhite as usize;
    if (dim_black..=dim_white).contains(&index) {
        return Color::Indexed((index - dim_black) as u8);
    }
    Color::Reset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::PaneSize;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn render_into(bytes: &[u8], area: Rect, mut buf: Buffer) -> (Buffer, Option<Position>) {
        let size = PaneSize { columns: area.width, lines: area.height };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes);
        let cursor = draw_term(&term, area, &mut buf);
        (buf, cursor)
    }

    fn render(bytes: &[u8], area: Rect) -> (Buffer, Option<Position>) {
        let buf = Buffer::empty(Rect::new(0, 0, area.x + area.width, area.y + area.height));
        render_into(bytes, area, buf)
    }

    #[test]
    fn colored_text_lands_in_the_area() {
        let area = Rect::new(1, 1, 10, 3);
        let (buf, cursor) = render(b"\x1b[31mhi\x1b[0m", area);
        assert_eq!(buf[(1, 1)].symbol(), "h");
        assert_eq!(buf[(1, 1)].fg, Color::Indexed(1));
        assert_eq!(buf[(2, 1)].symbol(), "i");
        assert_eq!(cursor, Some(Position::new(3, 1)));
    }

    #[test]
    fn wide_char_spacer_cell_is_left_untouched() {
        let area = Rect::new(0, 0, 10, 2);
        let mut buf = Buffer::empty(area);
        buf[(1, 0)].set_symbol("#");
        let (buf, _) = render_into("界x".as_bytes(), area, buf);
        assert_eq!(buf[(0, 0)].symbol(), "界");
        assert_eq!(buf[(1, 0)].symbol(), "#");
        assert_eq!(buf[(2, 0)].symbol(), "x");
    }

    #[test]
    fn hidden_cursor_is_not_reported() {
        let (_, cursor) = render(b"\x1b[?25l", Rect::new(0, 0, 10, 2));
        assert_eq!(cursor, None);
    }

    #[test]
    fn truecolor_and_attributes() {
        let (buf, _) = render(b"\x1b[1;7;38;2;10;20;30mX", Rect::new(0, 0, 4, 1));
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(10, 20, 30));
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD | Modifier::REVERSED));
    }
}
