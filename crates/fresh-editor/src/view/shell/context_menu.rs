//! Context menus as real `Layer`s — the first overlay in the tree (wave M2).
//!
//! The plan calls this wave the go/no-go: it is the first to use a layer, its
//! modality and its dismissal together, and if the model holds here the later
//! surfaces apply the same mechanisms.
//!
//! This step moves **paint** only. The layer is anchored at the position the
//! existing code already computed (`ContextMenu::clamped_position`) rather than
//! letting `fit` place it, so the menu lands on exactly the cells it landed on
//! before and the still-legacy hit-testing keeps agreeing with what is drawn.
//! Input, dismissal and the guard box move next; that is when the old
//! precedence entries can be deleted.

use fresh_ui::{col, layer, text, Anchor, Modality, Node, Sizing};

/// What one menu needs to draw: where it goes, what is in it, and which row is
/// highlighted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub highlighted: usize,
    pub items: Vec<String>,
}

/// The label as the old painter wrote it: one leading space, left-aligned, and
/// padded to the box's inner width.
///
/// Reproduced verbatim rather than re-derived — the row is what the cells
/// actually contain, and this is the migration's acceptance bar.
fn row_label(label: &str, width: u16) -> String {
    let content_width = (width as usize).saturating_sub(2);
    format!(" {:<pad$}", label, pad = content_width.saturating_sub(1))
}

/// One menu, as a description.
pub fn context_menu<M: 'static>(menu: &Menu) -> Node<M> {
    let rows: Vec<Node<M>> = menu
        .items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let theme = if i == menu.highlighted {
                "menu.item.highlighted"
            } else {
                "menu.item"
            };
            text(row_label(label, menu.width))
                .theme(theme)
                .h(Sizing::Cells(1))
        })
        .collect();

    layer()
        .key("context_menu")
        // The point the old code clamped to, not a fresh placement: the menu
        // must land where hit-testing still expects it.
        .anchor(Anchor::Point(menu.x, menu.y))
        // Paint-only for now — the legacy path still routes input, so claiming
        // it here would take events away from the code that still handles them.
        .modality(Modality::None)
        .child(
            col()
                .border()
                .theme("menu")
                .w(Sizing::Cells(menu.width))
                .children(rows),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The padding is the old painter's, character for character.
    #[test]
    fn a_row_is_padded_exactly_as_before() {
        // width 12 => content_width 10 => " " + label padded to 9.
        assert_eq!(row_label("Copy", 12), " Copy     ");
        assert_eq!(row_label("Copy", 12).chars().count(), 10);
    }

    /// Degenerate widths must not panic or produce a negative pad.
    #[test]
    fn narrow_menus_do_not_underflow() {
        for w in 0u16..4 {
            let _ = row_label("x", w);
        }
    }
}

#[cfg(test)]
mod paint_tests {
    use super::*;
    use crate::view::shell::fold::fold_native;
    use crate::view::shell::frame::{frame_tree, Frame};
    use fresh_ui::{Size, ThemeKey, Ui};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn plain(_: &ThemeKey) -> Style {
        Style::default()
    }

    fn render(menu: Menu, w: u16, h: u16) -> Buffer {
        let mut ui: Ui<()> = Ui::new();
        let frame = Frame {
            menu: Some(menu),
            ..Frame::default()
        };
        let spec = ui.frame(frame_tree(frame), Size::new(w, h)).clone();
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        fold_native(&spec, &mut buf, &plain);
        buf
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    /// The menu paints where it was told to, with the plain border glyphs the
    /// rest of the editor draws, and its labels padded as before.
    #[test]
    fn a_menu_paints_a_bordered_box_at_its_point() {
        let buf = render(
            Menu {
                x: 2,
                y: 1,
                width: 10,
                highlighted: 0,
                items: vec!["Copy".into(), "Paste".into()],
            },
            20,
            6,
        );
        assert_eq!(row(&buf, 1), "  ┌────────┐        ", "top border");
        assert_eq!(row(&buf, 2), "  │ Copy   │        ", "first item");
        assert_eq!(row(&buf, 3), "  │ Paste  │        ", "second item");
        assert_eq!(row(&buf, 4), "  └────────┘        ", "bottom border");
    }

    /// An overlay is out of flow: it does not disturb the regions around it.
    #[test]
    fn a_menu_does_not_move_the_frame() {
        use crate::view::shell::frame::{region_rects, HostRegion};
        let size = Rect::new(0, 0, 30, 8);
        let without = region_rects(Frame::default(), size);
        let with = region_rects(
            Frame {
                menu: Some(Menu {
                    x: 3,
                    y: 2,
                    width: 8,
                    highlighted: 0,
                    items: vec!["One".into()],
                }),
                ..Frame::default()
            },
            size,
        );
        for region in [HostRegion::Body, HostRegion::StatusBar, HostRegion::MenuBar] {
            let a = without.iter().find(|(r, _)| *r == region).unwrap().1;
            let b = with.iter().find(|(r, _)| *r == region).unwrap().1;
            assert_eq!(a, b, "{region:?} moved when a menu opened");
        }
    }
}
