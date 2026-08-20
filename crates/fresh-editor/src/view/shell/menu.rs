//! Menu-bar dropdowns as `Layer`s — the second overlay in the tree (wave M3).
//!
//! A dropdown chain is several boxes at once: the menu's own list, plus one
//! box per open submenu level, each placed against the one before it. Under
//! the old renderer that chain was a loop that painted, recorded hit rects and
//! decided placement in the same pass; here each level is a `Layer`, out of
//! flow and painted in declaration order, and the chain is just their order.
//!
//! **Paint only, for now.** The levels carry no modality, no dismissal and no
//! handlers: pointer input still runs through `chrome::Menu`'s boxes and the
//! full-frame `chrome:menu_close_guard`, and each level is anchored at the
//! rectangle `MenuRenderer::fit_dropdown_area` already chose. That is the same
//! bridge the context-menu wave used — land the cells first, then move input,
//! then let the layer's own `fit` decide placement — and it is what keeps the
//! not-yet-migrated hit-testing agreeing with what is drawn.

use fresh_ui::{col, layer, row, text, text_runs, Anchor, Node, Run, Sizing};

use super::msg::UiMsg;

/// One label on the menu bar: `" Label "`, cut into runs so a mnemonic
/// character can be underlined inside the label rather than beside it.
///
/// Cut here rather than in the description because the cut is the renderer's
/// decision — which character the mnemonic resolver picked — and the shell
/// only needs to know that a run exists and what to call it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarItem {
    /// `(text, theme name)`, in order. Usually one run; three when a mnemonic
    /// splits the label.
    pub runs: Vec<(String, &'static str)>,
}

/// The menu bar row: its labels, and the ground they sit on.
///
/// Empty is meaningful — the row still exists and still has a rectangle, which
/// is what the frame's other regions are measured against.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuBar {
    pub items: Vec<BarItem>,
}

/// The bar row as a description.
///
/// A background surface: it paints in the fold's `Background` band, under
/// every legacy painter, which is what the two-pass fold made possible. Its
/// own dropdowns are `Layer`s and paint in the other band, over them.
pub fn menu_bar(bar: &MenuBar) -> Node<UiMsg> {
    let labels: Vec<Node<UiMsg>> = bar
        .items
        .iter()
        .map(|it| {
            let runs: Vec<Run> = it
                .runs
                .iter()
                .map(|(t, theme)| Run::themed(t.clone(), *theme))
                .collect();
            text_runs(runs).into()
        })
        .collect();

    // The row names its own ground, so the cells between and after the labels
    // carry the bar's background — the `Paragraph`'s `.style(bg)` did that.
    row().theme("menu.bar").children(labels)
}

/// One row of one dropdown: what it says, and the name of how it looks.
///
/// Both halves come from the renderer's own derivation
/// (`MenuRenderer::dropdown_item_text` and `MenuRowStyle`), so a row here says
/// character-for-character what the old painter wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownRow {
    pub text: String,
    pub theme: &'static str,
}

/// One level of an open dropdown chain: the bordered box and its rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropdownLevel {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub rows: Vec<DropdownRow>,
}

/// The open chain, outermost level first.
///
/// Declaration order is paint order, so a submenu lands on top of the level it
/// opened from — which is what the old loop achieved by painting in the same
/// order, and what a z-rank would otherwise have to state.
pub fn dropdown_chain(levels: &[DropdownLevel]) -> Vec<Node<UiMsg>> {
    levels
        .iter()
        .enumerate()
        .map(|(depth, l)| dropdown(depth, l))
        .collect()
}

fn dropdown(depth: usize, level: &DropdownLevel) -> Node<UiMsg> {
    let rows: Vec<Node<UiMsg>> = level
        .rows
        .iter()
        .map(|r| {
            text(r.text.clone())
                .theme(r.theme)
                .h(Sizing::Cells(1))
                .into()
        })
        .collect();

    layer()
        .key(fresh_ui::Key::Pair("menu_dropdown".into(), depth as u64))
        // The rectangle the old placement walk chose, not a fresh one: while
        // hit-testing is still legacy it must keep agreeing with the cells.
        .anchor(Anchor::Point(level.x, level.y))
        .child(
            col()
                .border()
                // Border ink over the dropdown ground; the fill draws spaces,
                // so only the background of this key reaches the eye there.
                .theme("menu.dropdown")
                .w(Sizing::Cells(level.width))
                .children(rows),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::shell::fold::{fold_native, Band};
    use crate::view::shell::frame::{frame_tree, Frame};
    use crate::view::shell::msg::UiMsg;
    use fresh_ui::{Size, ThemeKey, Ui};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn plain(_: &ThemeKey) -> Style {
        Style::default()
    }

    fn row_of(text: &str) -> DropdownRow {
        DropdownRow {
            text: text.to_string(),
            theme: "menu.item",
        }
    }

    fn render(levels: Vec<DropdownLevel>, w: u16, h: u16) -> Buffer {
        let mut ui: Ui<UiMsg> = Ui::new();
        let frame = Frame {
            dropdowns: levels,
            ..Frame::default()
        };
        let spec = ui.frame(frame_tree(frame), Size::new(w, h)).clone();
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        // Both bands: a test that renders the whole frame wants the whole
        // display list, and the cut only matters where legacy painters go
        // between them.
        fold_native(&spec, &mut buf, &plain, Band::Background);
        fold_native(&spec, &mut buf, &plain, Band::Overlay);
        buf
    }

    fn line(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    /// A level lands on the cells its rectangle names, with the plain border
    /// glyphs the ratatui `Block` drew and its rows already padded to the
    /// box's content width — which is what `dropdown_item_text` produced for
    /// the painter this replaces.
    #[test]
    fn a_level_paints_its_box_where_it_was_placed() {
        let buf = render(
            vec![DropdownLevel {
                x: 1,
                y: 1,
                width: 12,
                rows: vec![row_of(" New      "), row_of(" Open     ")],
            }],
            20,
            8,
        );
        assert_eq!(line(&buf, 1), " ┌──────────┐       ", "top border");
        assert_eq!(line(&buf, 2), " │ New      │       ", "first row");
        assert_eq!(line(&buf, 3), " │ Open     │       ", "second row");
        assert_eq!(line(&buf, 4), " └──────────┘       ", "bottom border");
    }

    /// The bar row: `" Label "` per menu with a space between, on the bar's
    /// own ground — character for character what the `Paragraph` wrote.
    #[test]
    fn the_bar_paints_its_labels() {
        let mut ui: Ui<UiMsg> = Ui::new();
        let frame = Frame {
            menu_bar_items: MenuBar {
                items: vec![
                    BarItem {
                        runs: vec![
                            (" ".into(), "menu.bar.item"),
                            ("File".into(), "menu.bar.item"),
                            (" ".into(), "menu.bar.item"),
                            (" ".into(), "menu.bar"),
                        ],
                    },
                    BarItem {
                        runs: vec![
                            (" ".into(), "menu.bar.item"),
                            ("Edit".into(), "menu.bar.item"),
                            (" ".into(), "menu.bar.item"),
                            (" ".into(), "menu.bar"),
                        ],
                    },
                ],
            },
            ..Frame::default()
        };
        let spec = ui.frame(frame_tree(frame), Size::new(20, 4)).clone();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        fold_native(&spec, &mut buf, &plain, Band::Background);
        // `" File "` plus the separator space is 7 cells, exactly the stride
        // the label-area walk advances by.
        assert_eq!(line(&buf, 0), " File   Edit        ");
    }

    /// **A style inside a run.** The mnemonic is one underlined character in
    /// the middle of a label — text styled *within* itself, which is what
    /// `text_runs` exists for. Laying the three pieces out as siblings would
    /// let them wrap and truncate independently.
    #[test]
    fn a_mnemonic_is_its_own_run_inside_the_label() {
        let bar = MenuBar {
            items: vec![BarItem {
                runs: vec![
                    (" ".into(), "menu.bar.item"),
                    ("F".into(), "menu.bar.item.mnemonic"),
                    ("ile".into(), "menu.bar.item"),
                    (" ".into(), "menu.bar.item"),
                    (" ".into(), "menu.bar"),
                ],
            }],
        };
        let mut ui: Ui<UiMsg> = Ui::new();
        let spec = ui
            .frame(
                frame_tree(Frame {
                    menu_bar_items: bar,
                    ..Frame::default()
                }),
                Size::new(20, 4),
            )
            .clone();
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 4));
        fold_native(&spec, &mut buf, &plain, Band::Background);
        assert_eq!(line(&buf, 0), " File               ");
        // The underline is a theme name, not a glyph: the run carrying it is
        // its own item, so a backend can style it alone.
        let items = spec.items_for(&crate::view::shell::frame::region_key(
            crate::view::shell::frame::HostRegion::MenuBar,
        ));
        assert!(
            items
                .iter()
                .any(|i| i.theme.as_str() == "menu.bar.item.mnemonic"),
            "the mnemonic run must reach the display list under its own name"
        );
    }

    /// A migrated region is still a region: everything that asks for the menu
    /// bar's rectangle by name keeps getting an answer, now that no
    /// `Draw::Host` announces it.
    #[test]
    fn the_bar_still_reports_its_rectangle() {
        use crate::view::shell::frame::{region_rects, HostRegion};
        let rects = region_rects(Frame::default(), Rect::new(0, 0, 30, 8));
        let bar = rects
            .iter()
            .find(|(r, _)| *r == HostRegion::MenuBar)
            .expect("the menu bar still has a rectangle")
            .1;
        assert_eq!(bar, Rect::new(0, 0, 30, 1));
    }

    /// **Declaration order is paint order.** A submenu opens to the right of
    /// the level it came from and overlaps its edge by one column; the deeper
    /// box must win those cells. The old renderer got this from painting the
    /// chain in order, and so does this — no rank states it.
    #[test]
    fn a_submenu_paints_over_the_level_it_opened_from() {
        let buf = render(
            vec![
                DropdownLevel {
                    x: 0,
                    y: 0,
                    width: 10,
                    rows: vec![row_of(" File   "), row_of(" More  >")],
                },
                DropdownLevel {
                    x: 9,
                    y: 1,
                    width: 10,
                    rows: vec![row_of(" Deep   ")],
                },
            ],
            22,
            8,
        );
        // Column 9 is the parent's right border on row 1, and the submenu's
        // left border lands on it.
        assert_eq!(line(&buf, 1), "│ File   ┌────────┐   ");
        assert_eq!(line(&buf, 2), "│ More  >│ Deep   │   ");
    }

    /// An overlay is out of flow: opening a menu does not move the frame
    /// underneath it.
    #[test]
    fn a_dropdown_does_not_move_the_frame() {
        use crate::view::shell::frame::{region_rects, HostRegion};
        let size = Rect::new(0, 0, 30, 8);
        let without = region_rects(Frame::default(), size);
        let with = region_rects(
            Frame {
                dropdowns: vec![DropdownLevel {
                    x: 2,
                    y: 1,
                    width: 10,
                    rows: vec![row_of(" New     ")],
                }],
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
