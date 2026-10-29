use ratatui::layout::Rect;

use crate::pane::PaneSize;

pub const LIST_WIDTH: u16 = 28;
pub const SLOTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub list: Rect,
    pub panes: [Rect; SLOTS],
}

/// Session list on the left, then two panes sharing the rest of the width.
pub fn split(screen: Rect) -> Areas {
    let list_width = LIST_WIDTH.min(screen.width / 3);
    let rest = screen.width - list_width;
    let left = rest / 2;
    let list = Rect::new(screen.x, screen.y, list_width, screen.height);
    let first = Rect::new(screen.x + list_width, screen.y, left, screen.height);
    let second = Rect::new(first.x + left, screen.y, rest - left, screen.height);
    Areas { list, panes: [first, second] }
}

/// Inside of a block with a one-cell border.
pub fn inner(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

pub fn pane_size(area: Rect) -> PaneSize {
    let inner = inner(area);
    PaneSize { columns: inner.width.max(1), lines: inner.height.max(1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_screen_gets_full_list_and_equal_panes() {
        let areas = split(Rect::new(0, 0, 160, 45));
        assert_eq!(areas.list, Rect::new(0, 0, 28, 45));
        assert_eq!(areas.panes[0], Rect::new(28, 0, 66, 45));
        assert_eq!(areas.panes[1], Rect::new(94, 0, 66, 45));
        assert_eq!(pane_size(areas.panes[0]), PaneSize { columns: 64, lines: 43 });
    }

    #[test]
    fn odd_width_gives_the_extra_column_to_the_right_pane() {
        let areas = split(Rect::new(0, 0, 161, 10));
        assert_eq!(areas.panes[0].width + 1, areas.panes[1].width);
        assert_eq!(areas.panes[1].right(), 161);
    }

    #[test]
    fn narrow_screen_shrinks_the_list() {
        let areas = split(Rect::new(0, 0, 60, 10));
        assert_eq!(areas.list.width, 20);
        assert_eq!(areas.panes[0].width + areas.panes[1].width, 40);
    }

    #[test]
    fn tiny_area_never_gives_a_zero_pane() {
        assert_eq!(pane_size(Rect::new(0, 0, 1, 1)), PaneSize { columns: 1, lines: 1 });
    }
}
