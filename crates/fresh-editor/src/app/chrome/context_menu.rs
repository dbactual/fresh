//! Context menu (right-click menus): the menu box and its full-frame
//! close guard.

use anyhow::Result as AnyhowResult;

use super::{ChromeComponent, ChromeTreeBuilder, Editor};

pub(crate) struct ContextMenu;

impl ChromeComponent for ContextMenu {
    fn collect(&self, _ed: &Editor, _t: &mut ChromeTreeBuilder) {
        // Nothing. The menu is a `Layer` in the shell's tree, and its pointer
        // behaviour comes from properties rather than boxes: `Modality::Inert`
        // makes everything outside non-interactive — which is what the
        // full-frame close-guard box simulated — and `OUTSIDE_POINTER`
        // dismissal closes it. The shell is offered the pointer before this
        // walk runs, so neither box has anything left to do.
        //
        // The keyboard grab (`on_key`) and the layer entry below have not
        // migrated yet.
    }

    /// The open native context menu (tab / "+" new-tab /
    /// file-explorer / close-split) grabs the keyboard: navigation
    /// and activation on unmodified keys, everything else swallowed
    /// (#2587). One handler covers all of them via the shared
    /// geometry core.
    fn on_key(
        &self,
        ed: &mut Editor,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<AnyhowResult<()>> {
        ed.handle_context_menu_key(code, modifiers)
    }

    fn layers(&self, ed: &Editor, out: &mut Vec<(u16, crate::app::overlay::Layer)>) {
        use crate::app::overlay::{Layer, LayerKind};
        // The native context menus are modal chrome: while one is open
        // it owns the keyboard via the custom dispatcher (`on_key`
        // above), so no `KeyContext` is exposed. Like any covering
        // overlay it blocks PTY routing. Ranked below `Popup` so the
        // unfocused-popup `take_while` guard is unaffected. One entry
        // covers all four menus — they share the geometry core and are
        // mutually exclusive (opening one closes the others).
        if ed.active_window().open_context_menu().is_some() {
            out.push((
                super::layer_rank::CONTEXT_MENU,
                Layer {
                    kind: LayerKind::ContextMenu,
                    owns_keyboard: true,
                    key_context: None,
                    blocks_terminal_input: true,
                },
            ));
        }
    }
}

/// Behavior owned by this component (moved from mouse_input.rs —
/// the handlers its arms dispatch to).
impl Editor {
    /// Handle a key event while a native context menu (tab / "+" new-tab /
    /// file-explorer) is open — the one keyboard handler for all three.
    ///
    /// The open menu **grabs the keyboard**: Up/Down move the highlight,
    /// Enter activates the highlighted item, Esc dismisses, and every other
    /// key — printable characters, Backspace, modified chords — is swallowed
    /// so it can't leak into the buffer or the explorer's type-ahead find
    /// underneath and silently retarget the selection the menu acts on
    /// (#2587). Navigation/activation act only on *unmodified* keys; a
    /// modified chord is swallowed like any other non-menu key.
    ///
    /// Returns `Some` whenever a menu is open (the key is always consumed),
    /// `None` when no menu is open so normal dispatch continues.
    pub(super) fn handle_context_menu_key(
        &mut self,
        code: crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<AnyhowResult<()>> {
        use crossterm::event::{KeyCode, KeyModifiers};

        let kind = self.active_window().open_context_menu().map(|(k, _)| k)?;

        if modifiers == KeyModifiers::NONE {
            match code {
                KeyCode::Up => {
                    if let Some(core) = self.active_window_mut().context_menu_core_mut() {
                        core.prev_item();
                    }
                    return Some(Ok(()));
                }
                KeyCode::Down => {
                    if let Some(core) = self.active_window_mut().context_menu_core_mut() {
                        core.next_item();
                    }
                    return Some(Ok(()));
                }
                KeyCode::Enter => {
                    return Some(self.activate_highlighted_context_menu(kind));
                }
                KeyCode::Esc => {
                    self.active_window_mut().close_context_menus();
                    return Some(Ok(()));
                }
                _ => {}
            }
        }

        // Modal: swallow every other key while a menu is open.
        Some(Ok(()))
    }

    /// Activate the highlighted item of the open context menu: resolve the
    /// item + its payload from the concrete menu, dismiss the menu, then run
    /// the matching `execute_*` action. Shared by both the keyboard (Enter)
    /// and mouse (click) paths so activation lives in exactly one place — the
    /// pointer path now reaches it from the shell (`apply_ui_fact`), the
    /// keyboard path still from this component.
    pub(crate) fn activate_highlighted_context_menu(
        &mut self,
        kind: crate::app::types::ContextMenuKind,
    ) -> AnyhowResult<()> {
        use crate::app::types::ContextMenuKind;
        match kind {
            ContextMenuKind::Tab => {
                let selected = self
                    .active_window()
                    .tab_context_menu
                    .as_ref()
                    .map(|m| (m.highlighted_item(), m.buffer_id, m.split_id));
                self.active_window_mut().close_context_menus();
                if let Some((item, buffer_id, split_id)) = selected {
                    return self.execute_tab_context_menu_action(item, buffer_id, split_id);
                }
            }
            ContextMenuKind::NewTab => {
                let selected = self
                    .active_window()
                    .new_tab_menu
                    .as_ref()
                    .map(|m| (m.highlighted_item(), m.split_id));
                self.active_window_mut().close_context_menus();
                if let Some((item, split_id)) = selected {
                    return self.execute_new_tab_menu_action(item, split_id);
                }
            }
            ContextMenuKind::FileExplorer => {
                let selected = self
                    .active_window()
                    .file_explorer_context_menu
                    .as_ref()
                    .map(|m| m.highlighted_item());
                self.active_window_mut().close_context_menus();
                if let Some(item) = selected {
                    self.execute_file_explorer_context_menu_action(item);
                }
            }
            ContextMenuKind::CloseSplit => {
                let selected = self
                    .active_window()
                    .close_split_menu
                    .as_ref()
                    .map(|m| (m.highlighted_item(), m.split_id));
                self.active_window_mut().close_context_menus();
                if let Some((item, split_id)) = selected {
                    self.execute_close_split_menu_action(item, split_id);
                }
            }
        }
        Ok(())
    }
}
