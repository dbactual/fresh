//! The editor frame as a `fresh-ui` description.
//!
//! Every region is a `Host` leaf: the shell owns the *layout*, the existing
//! painters keep owning the *content*. Regions are then replaced by native
//! descriptions one at a time (stages S2–S5 of the migration doc).
//!
//! The rectangles this produces are asserted equal to the ones
//! `Editor::render`'s ratatui `Layout` calls produce, over a sweep of sizes and
//! visibility combinations, in `tests/ui_shell_frame_parity.rs`.

use fresh_ui::{col, host, row, HostId, Node, Sizing};

/// A region of the frame the host still paints itself.
///
/// The discriminants are the `HostId` values carried in `Draw::Host`, so the
/// fold can map an item straight back to the painter that owns it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u64)]
pub enum HostRegion {
    Dock = 1,
    MenuBar = 2,
    Explorer = 3,
    /// The split grid: buffers, terminals, tabs, scrollbars. The last region
    /// that will still be a `Host` when the migration finishes.
    Body = 4,
    StatusBar = 5,
    SearchOptions = 6,
    PromptLine = 7,
}

impl HostRegion {
    pub const ALL: [HostRegion; 7] = [
        HostRegion::Dock,
        HostRegion::MenuBar,
        HostRegion::Explorer,
        HostRegion::Body,
        HostRegion::StatusBar,
        HostRegion::SearchOptions,
        HostRegion::PromptLine,
    ];

    pub fn from_host_id(id: HostId) -> Option<HostRegion> {
        HostRegion::ALL.into_iter().find(|r| r.id() == id.0)
    }

    pub fn id(self) -> u64 {
        self as u64
    }
}

impl From<HostRegion> for HostId {
    fn from(r: HostRegion) -> HostId {
        HostId(r.id())
    }
}

/// Which regions are visible, and how wide the sized ones are.
///
/// Every field here is *app state*: `build()` cannot read geometry, so
/// decisions that today read `size` at the top of `render` — the dock's
/// bail-out, the explorer's column count — are resolved from state before the
/// description is built. See [`Frame::resolve_dock`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub menu_bar: bool,
    pub status_bar: bool,
    pub search_options: bool,
    pub prompt_line: bool,
    /// Column width, already resolved against the frame width.
    pub dock: Option<u16>,
    /// (columns, on_left)
    pub explorer: Option<(u16, bool)>,
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            menu_bar: true,
            status_bar: true,
            search_options: false,
            prompt_line: false,
            dock: None,
            explorer: None,
        }
    }
}

impl Frame {
    /// The dock's bail-out rules from `compute_dock_split`.
    ///
    /// This is **app logic keyed on the frame width**, not a layout
    /// constraint — it decides whether a dock exists at all. `build()` cannot
    /// read geometry, so it is resolved here, from the last known frame width,
    /// before the description is built.
    pub fn resolve_dock(mut self, frame_width: u16) -> Frame {
        const EDITOR_MIN: u16 = 20;
        const DOCK_MIN: u16 = 24;
        self.dock = self.dock.and_then(|requested| {
            let max_dock = frame_width.saturating_sub(EDITOR_MIN);
            (max_dock >= DOCK_MIN).then(|| requested.min(max_dock).max(1))
        });
        self
    }

    /// Rows whose height is fixed at one cell when visible.
    ///
    /// When the frame is shorter than this, `fresh-ui` and ratatui starve
    /// *different* rows (pinned by `squeeze_band_starves_a_different_row_than_ratatui`).
    /// Callers that care decide which rows to drop themselves rather than
    /// inheriting either engine's starvation order.
    pub fn fixed_rows(&self) -> u16 {
        self.menu_bar as u16
            + self.status_bar as u16
            + self.search_options as u16
            + self.prompt_line as u16
    }
}

/// The frame description: one `Host` per visible region.
pub fn frame_tree<M: 'static>(f: Frame) -> Node<M> {
    let mut rows: Vec<Node<M>> = Vec::new();
    if f.menu_bar {
        rows.push(region(HostRegion::MenuBar).h(Sizing::Cells(1)));
    }
    rows.push(match f.explorer {
        Some((cols, true)) => row().flex(1).children([
            region(HostRegion::Explorer).w(Sizing::Cells(cols)),
            region(HostRegion::Body).flex(1),
        ]),
        Some((cols, false)) => row().flex(1).children([
            region(HostRegion::Body).flex(1),
            region(HostRegion::Explorer).w(Sizing::Cells(cols)),
        ]),
        None => region(HostRegion::Body).flex(1),
    });
    if f.status_bar {
        rows.push(region(HostRegion::StatusBar).h(Sizing::Cells(1)));
    }
    if f.search_options {
        rows.push(region(HostRegion::SearchOptions).h(Sizing::Cells(1)));
    }
    if f.prompt_line {
        rows.push(region(HostRegion::PromptLine).h(Sizing::Cells(1)));
    }
    let chrome = col().flex(1).children(rows);
    match f.dock {
        Some(w) => row().children([region(HostRegion::Dock).w(Sizing::Cells(w)), chrome]),
        None => chrome,
    }
}

fn region<M: 'static>(r: HostRegion) -> Node<M> {
    host(r.id())
}
