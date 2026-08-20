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

use fresh_ui::{col, gesture, layer, text, Anchor, Dismiss, Modality, Node, Sizing};

use super::msg::{UiFact, UiMsg};

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
///
/// The rows answer the pointer themselves, and the layer's own properties do
/// what a full-frame guard box used to: `Modality::Inert` makes everything
/// outside non-interactive, and `OUTSIDE_POINTER` dismissal turns a click out
/// there into a close. Neither is a rule anyone wrote down for this surface —
/// they are declared properties of the layer, which is the whole argument for
/// moving overlays into the tree.
pub fn context_menu(menu: &Menu) -> Node<UiMsg> {
    let rows: Vec<Node<UiMsg>> = menu
        .items
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let theme = if i == menu.highlighted {
                "menu.item.highlighted"
            } else {
                "menu.item"
            };
            gesture(
                text(row_label(label, menu.width))
                    .theme(theme)
                    .h(Sizing::Cells(1)),
            )
            // A click moves the highlight and activates, exactly as the old
            // click handler did — activation runs the same path Enter does.
            .on_click(move |_| UiMsg::Ui(UiFact::ActivateContextMenuItem(i)))
            .on_enter(std::rc::Rc::new(move |_: &fresh_ui::Event| {
                Some(UiMsg::Ui(UiFact::HighlightContextMenuItem(i)))
            }))
        })
        .collect();

    layer()
        .key("context_menu")
        // The point the old code clamped to, not a fresh placement: the menu
        // must land where hit-testing still expects it.
        .anchor(Anchor::Point(menu.x, menu.y))
        // Everything outside is non-interactive while the menu is up. This is
        // the full-frame close-guard box, expressed as a property.
        .modality(Modality::Inert)
        // Escape stays with the legacy key handler for now, so only the
        // pointer half is declared here.
        .dismiss(Dismiss::OUTSIDE_POINTER)
        .on_dismiss(|_| UiMsg::Ui(UiFact::CloseContextMenu))
        .child(
            gesture(
                col()
                    .border()
                    .theme("menu")
                    .w(Sizing::Cells(menu.width))
                    .children(rows),
            )
            // A right-click inside an open menu is swallowed so the menu stays
            // put rather than being re-opened or re-targeted. Claiming and
            // reporting are separate in the library, so this says both.
            .on_secondary_click(|e| {
                e.stop();
                UiMsg::Ui(UiFact::Consumed)
            }),
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
        let mut ui: Ui<UiMsg> = Ui::new();
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

#[cfg(test)]
mod input_tests {
    use super::*;
    use crate::view::shell::frame::{frame_tree, Frame};
    use fresh_ui::{Input, Mods, MouseButton, Point, Size, Ui};

    fn menu() -> Menu {
        Menu {
            x: 2,
            y: 1,
            width: 10,
            highlighted: 0,
            items: vec!["Copy".into(), "Paste".into()],
        }
    }

    fn open(w: u16, h: u16) -> Ui<UiMsg> {
        let mut ui: Ui<UiMsg> = Ui::new();
        ui.frame(
            frame_tree(Frame {
                menu: Some(menu()),
                ..Frame::default()
            }),
            Size::new(w, h),
        );
        ui
    }

    /// Both halves of the click. Dismissal is evaluated on the *press* (as the
    /// close-guard box also was) while activation lands on the release, so a
    /// helper that watched only one would miss half the behaviour.
    fn click(ui: &mut Ui<UiMsg>, x: i32, y: i32) -> Vec<UiMsg> {
        let pos = Point::new(x, y);
        let mut out = ui.dispatch(Input::Press {
            pos,
            button: MouseButton::Left,
            mods: Mods::NONE,
        });
        out.extend(ui.dispatch(Input::Release {
            pos,
            button: MouseButton::Left,
            mods: Mods::NONE,
        }));
        out
    }

    fn facts(msgs: Vec<UiMsg>) -> Vec<UiFact> {
        msgs.into_iter()
            .map(|m| match m {
                UiMsg::Ui(f) => f,
                other => panic!("unexpected message {other:?}"),
            })
            .collect()
    }

    /// Clicking a row activates it, and the row it names is the row under the
    /// pointer — the box's border is one cell, so the first item is at y+1.
    #[test]
    fn clicking_a_row_activates_that_row() {
        let mut ui = open(20, 8);
        let got = facts(click(&mut ui, 4, 2));
        assert!(
            matches!(got.as_slice(), [UiFact::ActivateContextMenuItem(0)]),
            "got {got:?}"
        );

        let mut ui = open(20, 8);
        let got = facts(click(&mut ui, 4, 3));
        assert!(
            matches!(got.as_slice(), [UiFact::ActivateContextMenuItem(1)]),
            "got {got:?}"
        );
    }

    /// **The close-guard box, replaced by a property.** A click outside the
    /// menu dismisses it — declared as `OUTSIDE_POINTER` on the layer rather
    /// than simulated with a full-frame box that has to be pushed, ranked and
    /// kept in sync.
    #[test]
    fn clicking_outside_dismisses() {
        let mut ui = open(20, 8);
        let got = facts(click(&mut ui, 18, 7));
        assert!(
            got.contains(&UiFact::CloseContextMenu),
            "a click outside must close the menu, got {got:?}"
        );
    }

    /// Hovering a row moves the highlight, which the old component did through
    /// a hover-target walk and a `HoverTarget::ContextMenuItem` round trip.
    #[test]
    fn hovering_a_row_highlights_it() {
        let mut ui = open(20, 8);
        let got = facts(ui.dispatch(Input::Move {
            pos: Point::new(4, 3),
            mods: Mods::NONE,
        }));
        assert!(
            got.contains(&UiFact::HighlightContextMenuItem(1)),
            "got {got:?}"
        );
    }

    /// A right-click inside is swallowed so the menu stays put rather than
    /// being re-opened or re-targeted.
    #[test]
    fn a_right_click_inside_is_swallowed() {
        let mut ui = open(20, 8);
        let pos = Point::new(4, 2);
        let mut msgs = ui.dispatch(Input::Press {
            pos,
            button: MouseButton::Right,
            mods: Mods::NONE,
        });
        msgs.extend(ui.dispatch(Input::Release {
            pos,
            button: MouseButton::Right,
            mods: Mods::NONE,
        }));
        let got = facts(msgs);
        assert!(got.contains(&UiFact::Consumed), "got {got:?}");
    }
}
