# Migrating the Fresh editor UI onto `fresh-ui`

> _AI-generated. **Current-state survey is IMPLEMENTED; the design and plan are
> PLANNED.** This is the editor-side companion to the two library documents:
> [`widget-library-design.md`](widget-library-design.md) (the architecture
> authority for `fresh-ui`) and
> [`widget-library-implementation-plan.md`](widget-library-implementation-plan.md)
> (which builds the library in Part 1 and sketches the migration as Part 2,
> M0–M9). Those docs describe the target and the waves in the abstract. This one
> grounds them in the editor as it exists today — what each surface actually is,
> where its state and geometry live, how input reaches it — and turns the
> abstract waves into concrete, file-level moves. Where this doc and the two
> library docs disagree about the target, they win; where they disagree about
> the current editor, the source wins._

---

## 0. Situation

`fresh-ui` is **built and standalone** (PR #3024 plus the R1–R11 and Part 1c–1e
follow-ups). It is a retained, reconciling UI tree — immutable descriptions →
persistent elements matched by `(type, key)` → render objects holding geometry
and focus — that emits a backend-independent `LayoutSpec` display list. Its only
runtime dependency is `unicode-width`. It has the real `RenderObject` layer, a
retained focus tree, ambients, `Tasks`, layers with modality/dismissal, pointer
capture, and the full widget set (`Button`, `Toggle`, `TextField`, `List`
eager+windowed, `Tree`, `Dropdown`, `RadioGroup`, `Number`, `DualList`). The
demo under `crates/fresh-ui/tests/support/demo/` and `examples/interactive.rs`
exercise every capability against a terminal backend.

**It is not yet wired into the editor.** No file under `crates/fresh-editor`,
`crates/fresh-core`, or `crates/fresh-plugin-runtime` references `fresh_ui`.
Part 2 has not started. So this is a genuine greenfield adoption, not a
course-correction — which is the good case: the library was finished and frozen
before any surface depended on it.

The editor's UI, meanwhile, is **already halfway to this model on its own
terms** and stopped at exactly the wall `fresh-ui` was built to get past. The
`ChromeComponent` registry, the `Scene` projection, the shared
`LayoutBox`/`hit_stack` primitive, and the derived `overlay_stack()` are a
proto-retained-widget tree. What they lack — real containment, one precedence
order, per-node pointer capture, one focus ring — is precisely what `fresh-ui`
supplies. The migration is less a rewrite than **finishing a refactor the
codebase already committed to** (the forward arc named in
[`chrome-event-model-plan.md`](chrome-event-model-plan.md) and
[`widget-framework-v2-review.md`](widget-framework-v2-review.md)).

---

## 1. The one hard constraint: the keep/migrate boundary

The optimized, file-backed text buffers and the text-rendering pipeline **do not
migrate**. They keep their existing logic and are reached from `fresh-ui` through
a `Host` leaf. The boundary is not fuzzy — it is a single function signature.

### The line is `SplitRenderer::render_content`

`view/ui/split_rendering/mod.rs` exposes:

```rust
pub fn render_content(&self, buf: &mut ratatui::buffer::Buffer, area: Rect, …)
    -> /* per-leaf layout caches */
pub fn render_phantom_leaf(…)   // off-tree previews (palette preview, web slices)
```

`render_content` already paints into an **arbitrary `Buffer` at an arbitrary
`Rect`** — its module doc calls it "composable into any buffer: offscreen
previews, tests, and the web bridge." Everything it touches is *keep*; everything
that carves the rect it is handed and reads the caches it returns is *migrate*.

| KEEP — behind a `Host` leaf, logic untouched | MIGRATE — onto `fresh-ui` |
|---|---|
| `view/ui/split_rendering/**` (token IR → `ViewLine`; `render_line/**` and its `CellPass` per-char state machine; gutter, folding, conceal, virtual-text, soft-break, scrollbar glyphs, composite view) | `app/chrome/**` (the whole `ChromeComponent` registry) |
| `view/line_wrap_cache.rs`, `view/wrap_index.rs`, `view/wrap_machine.rs` (tier-1/tier-2 wrap caches) | `view/scene.rs` (the `Scene` projections become the tree's data model) |
| `view/viewport.rs`, `view/folding.rs`, `view/conceal.rs`, `view/soft_break.rs`, `view/virtual_text.rs`, `view/margin.rs`, `view/composite_view.rs` | `view/ui/{menu,tabs,status_bar,suggestions,scrollbar,scroll_panel,file_explorer,file_browser,focus,layout}.rs` |
| `EditorState`, `Buffer`, the piece tree, markers, undo (see [`text-model.md`](text-model.md), [`buffers-splits-undo.md`](buffers-splits-undo.md)) | `view/popup.rs` + `view/popup/**`; `view/settings/**`; `view/controls/**`; `view/keybinding_editor.rs`; `view/workspace_trust_dialog.rs` |
| `Viewport` scroll math, `view_line_mappings` (mouse→byte hit-test slices), the per-cell theme-provenance map | The frame layout, dock/sidebar carve, and the modal z-band in `app/render.rs` |

**What crosses the seam.** The text view is not a black box; three of its outputs
are read by chrome and must keep flowing:

1. `view_line_mappings` — per-visual-row `ViewLineMapping` slices used for O(1)
   screen→byte hit-testing and click-to-cursor. The `Host` leaf must surface
   these after layout.
2. The per-cell theme-provenance map (`CellThemeInfo`) — read by the Scene/web
   projection and the theme inspector.
3. The **caret**. The hardware cursor is committed at end-of-frame and arbitrated
   against late overlays (`cursor_suppressed_by_late_overlay`). `fresh-ui` already
   has `LayoutSpec.cursor` (a `TextField` sets it); the `Host` leaf must be able
   to place it too, and the frame needs one "who owns the caret" arbitration.

**Ordering the seam must preserve.** Today geometry is "painted, then projected":
`render_content` writes the layout caches *during* paint, and chrome/Scene read
them *after*. `fresh-ui` inverts this for its own tree — layout is a distinct pass
before paint, and geometry comes from the layout pass, never from paint (design
§8.2). The `Host` leaf is where the two orderings meet: the leaf's `layout`
produces the rect, the leaf's `paint` runs `render_content` into that rect and
captures the caches, and event handlers read the caches back. This is legal
because the caches are the leaf's own render-object state, not framework geometry.

---

## 2. The current UI, as it actually is

This section is the survey the migration needs. It samples the surfaces the
request named — editor/window, menus, file explorer, the widget system, settings,
splits, prompts, the plugin widget API — plus the mouse/keyboard event dispatch
that ties them together.

### 2.1 The frame and the `Editor` object

`Editor` (`app/mod.rs`) is the central object but **not** a buffer owner. Buffers
and splits live on `Window` (`app/window/**`); `Editor` holds a `windows` map and
an `active_window`, and derives the active buffer/splits/explorer/prompt/popups
through accessors (`active_window()`, `active_state()`, `split_manager()`,
`active_chrome()`, `active_layout()`). It also holds the cross-cutting services:
config, theme (`Arc<RwLock<Theme>>`), registries, keybindings, clipboard, the
plugin manager and async bridge, the dock and floating-panel state, and the
widget registry.

`Editor::render(&mut self, frame: &mut Frame)` (`app/render.rs`) is **immediate
mode**: the whole screen is re-derived every frame; the runtime loop decides
*when* to call it. The ordered flow is, in essence:

1. drain pre-layout plugin commands (the one place inside draw that takes the
   plugin lock);
2. `compute_dock_split` → carve the left dock column;
3. animation snapshot; reset the cell-theme map; scroll-sync; request semantic
   ranges for visible splits;
4. a ratatui `Layout` splits the chrome area into
   `[menu_bar, main_content, status_bar, search_options, prompt_line]`;
5. carve the file-explorer sidebar out of `main_content`; render it;
6. **`SplitRenderer::render_content(frame.buffer_mut(), editor_content_area, …)`**
   inside a single `WindowBuffers::with_all_mut` split-borrow — this is the text
   pipeline, and it returns the per-leaf layout caches
   (`split_areas`, `tab_layouts`, `view_line_mappings`, scrollbar/separator
   areas) onto `active_layout_mut()`;
7. paint the chrome on top, in a fixed order: status bar, search options, prompt
   line, prompt/buffer/global popups, menu bar (last), context menus, tab-drag,
   software cursor, deferred hardware-cursor commit, frame-buffer animations;
8. `render_panels_and_modals` — dock, full-screen modals, floating panel, trust
   modal (the topmost z-band);
9. `convert_buffer_colors` (256/16 fallback) over the finished buffer;
   `bump_ui_gen()`.

The whole of steps 2, 4, 5, 7, 8 is chrome composition — the part that migrates.
Step 6 is the `Host` leaf.

### 2.2 Chrome: a registry that is a proto-widget-tree

There **is** a central abstraction, and it is already component-shaped.

- `trait ChromeComponent: Sync` (`app/chrome/mod.rs`) — one ZST implementor per
  surface. Its methods are the parallel dispatch interface: `collect` (contribute
  geometry boxes), `hover`/`on_hover_change`, `on_pointer`, `on_wheel`/`on_hwheel`,
  `capture_mouse`, `on_key` (pre-band grab), `on_layer_key` (the keyboard walk),
  and `layers` (precedence contribution).
- `components() -> &'static [&dyn ChromeComponent]` — **the** registry, 17
  entries: Settings, KeybindingEditor, CalibrationWizard, WorkspaceTrust,
  ThemeInfo, ContextMenu, Prompt, Popups, FileBrowser, FloatingModal, Dock,
  Splits, Menu, FileExplorer, StatusBar, SearchOptions, Base.

Notably, **the text view is itself a component** (`Splits`, contributing a
`chrome:editor` box) and the keyboard/pointer floor is a component (`Base`,
z-0). The editor content is the lowest-precedence participant in the same tree,
not a privileged root — which is exactly the `fresh-ui` stance (no privileged
internal surface) and makes the mapping natural.

Per-surface, the pattern is uniform: `collect` reads a geometry rect (from a
paint cache or a live derivation) and pushes a kind-tagged `LayoutBox` stamped
with the component's registry index; the handlers delegate to `impl Editor`
methods that hold the real behavior and mutate `Editor`/`Window` fields. The
components are stateless; **all state is on `Editor` or `Window`.**

### 2.3 Two decoupled precedence systems

This is the single most important structural fact for the migration, and the
thing `fresh-ui` collapses. Pointer stacking and keyboard/modal precedence are
**two separate orderings that deliberately disagree**:

- **Pointer z** — each `LayoutBox` carries a `z` on an ×10 band scheme (context
  menu ~180 … tabs 60 … scrollbars 50 … status 40 … editor 10 … base 0). The
  pointer walk is `hit_stack` (effective-z desc, then depth desc, then document
  order), with registry order as the intra-band tiebreak.
- **Keyboard / modal rank** — a *separate* hand-tuned constant table,
  `chrome::layer_rank`: `SETTINGS=900 … MENU=860 PROMPT=850 POPUP=840
  CONTEXT_MENU=830 FLOATING_MODAL=820 DOCK=810 EDITOR_BASE=0`. Each component
  contributes `(rank, Layer)` via `layers()`; `Editor::overlay_stack()`
  concatenates all contributions (plus a hardcoded EventDebug head at 1000) and
  stable-sorts rank-descending into **the** single ordered `OwnedLayer` list.

`overlay_stack()` is consumed by the keyboard walk, the mouse-capture band, the
PTY-input gate (`presents_blocking_overlay`), `modal_overlay_active`,
`popup_blocked_by_higher_modal`, and `get_key_context`. The two orderings
intentionally diverge (e.g. a menu's *keyboard* layer outranks the prompt, but
its *boxes* sit in a lower pointer band; context menus rank below popups for
keyboard but their boxes sit at the top pointer band), and the relationships are
pinned by tests. Precedence is therefore **data spread across 17 `layers()`
impls and two constant tables**, not a property of a tree.

### 2.4 Geometry: two sources, one deliberate seam

Chrome geometry comes from two places, and a migration underway (slice 7 of the
chrome-event-model plan) is moving surfaces from the first to the second:

- **Paint-recorded caches** — `ChromeLayout` (editor-global) and
  `WindowLayoutCache` (per-window splits/tabs/explorer). `render_content` and the
  popup painters write rects here *during* paint; `collect` reads them at event
  time.
- **Live-derived** — `status_bar_layout_now()`, `search_options_layout_now()`,
  `menu_layout_now()` recompute geometry from state at event time, and the paint
  pass debug-asserts paint == derivation. Their retired cache fields are gone.

A subset **stays paint-recorded by explicit ruling**: `popup_areas`,
`global_popup_areas`, the prompt suggestions/toolbar/preview rects, the file
browser layout, the workspace-trust dialog rect, and the floating-panel paint
fields. The reason is real and constrains the migration: these anchor to
**paint-produced text layout** — the cursor's screen position, the wrap maps. Any
`fresh-ui` tree that positions a completion popup at the caret must therefore be
able to read the caret's post-layout screen position out of the `Host` leaf. This
is the §1 "what crosses the seam" requirement seen from the other side.

### 2.5 The `Scene` projection

`view/scene.rs` is **not** a retained scene graph — it is a set of
`Serialize`-deriving **semantic projections** computed once per frame from
`Editor` state plus the last-frame geometry caches: `MenuView`, `TabBarView`,
`StatusView`, `PaletteView`, `ScenePopup`, `FileExplorerView`,
`FileBrowserView`, `TrustDialogView`, `WidgetSurfaceView`, `ContextMenuView`,
`AuxModalView`, `KeybindingEditorView`. It is the single source of truth for
*what the chrome is* (which items/tabs/rows exist, enabled/checked, and their
rects), and it is consumed by both the TUI painter and the web frontend. It
deliberately **excludes buffer text** — the web bridge slices *rendered cells*
out of the framebuffer for the buffer and preview panes (the `PaletteView`
preview rect is exactly such a slice).

This is a gift to the migration: `Scene` is already "the description of the
chrome, minus geometry, minus text." It becomes the props the `fresh-ui`
components read. It is also the parity oracle — `scene_parity.rs` is the check
that the web projection has not diverged, and it must keep passing through every
wave.

### 2.6 Event dispatch — keyboard

Entry `app/input.rs`, `handle_key_press` → `handle_key`. Ordered pre-band stages,
then the derived walk:

1. `bump_ui_gen()` (invalidate per-event memos);
2. event-debug intercept;
3. `dispatch_terminal_input` (terminal mode, `terminalBypass`, scrollback) —
   short-circuits if `presents_blocking_overlay()`;
4. `try_resolve_next_key_callback` (plugin `getNextKey()`);
5. **pre-band grabs**: `for c in components() { c.on_key(...) }` — by ruling only
   two members participate (ThemeInfo as observer, ContextMenu as a
   custom-dispatch modal);
6. transient-popup dismissal observer;
7. **the walk**: `dispatch_layer_keyboard` walks `overlay_stack()` top-down,
   offering the key to each layer's owner via `on_layer_key`; first `Some` stops.
   `Base` always answers, so the walk always terminates.

The pipeline **tail** — mode bindings → composite router → chord/keybinding
resolution — lives in `Base`'s handler, reaching the keybinding resolver, which
resolves key→`Action` against the `KeyContext` from `get_key_context()`.
`KeyContext` (an enum in `fresh-core/action.rs`) is the current stand-in for a
focus ring: it is *derived* from the topmost keyboard-owning layer, not stored.

Note a deliberate asymmetry: the keyboard side does **not** get the
one-tree-per-event treatment the pointer side does, because key handlers
"mutate then decline" (a popup rung processes `ClosePopup` and falls through),
which forces per-handler `get_key_context()` re-derivation.

### 2.7 Event dispatch — pointer

Entry `app/mouse_input.rs`, `handle_mouse` → `handle_mouse_impl`:

1. **modal capture band** — walk `overlay_stack()` in rank order; the first
   component whose modal is up claims the *whole* mouse channel via
   `capture_mouse`. (This replaced a deleted `dispatch_modal_mouse` ladder.)
2. pre-walk observers (LSP-rename cancel, GPM cursor);
3. **terminal forwarding gate** — suppressed when a `pointer_grab` is active, a
   context menu is open, or an opaque chrome box covers the point;
4. **one chrome tree per event** — `chrome_tree(self)` collects every component's
   boxes, validated-memoized on `(ui_gen, overlay_stack)` with a debug oracle
   that rebuilds and compares;
5. dispatch: `dispatch_pointer` for presses, `dispatch_wheel` for scroll. Both
   walk the `hit_stack`; `dispatch_pointer` dedups on `(owner, kind)` and honors
   `Disposition::{Consumed, PassAfter, Pass}` (a declining *opaque* box absorbs
   the event); `dispatch_wheel` has **no** opacity gate and **no** dedup, so a
   declining surface lets the wheel keep falling for scroll-chaining.

Fine click geometry is pure and `Editor`-free (`app/click_geometry.rs`):
`screen_to_buffer_position*` maps (col,row) + content rect + gutter +
`ViewLineMapping` → buffer byte + virtual-space overshoot.

### 2.8 Drags, hover, focus — the three ad-hoc clusters

These are the three places the flat tree cannot express what it needs, and each
is a hand-rolled substitute for a `fresh-ui` primitive:

- **Drags → `PointerGrab`.** `pointer_grab(ed)` is a hand-ordered match over ~13
  drag flags scattered across `Editor` and `mouse_state` (`dock_resizing`,
  `widget_text_drag`, `dragging_scrollbar`, `dragging_horizontal_scrollbar`,
  `selecting_in_popup`, `dragging_prompt_scrollbar`, `dragging_popup_scrollbar`,
  `dragging_separator`, `dragging_file_explorer`, `terminal_drag_pending`,
  `dragging_text_selection`, `dragging_tab`, …). The grab owns the pointer from
  press to release regardless of what is under it. This is exactly what per-node
  `cx.capture_pointer()` replaces — one slot, owned by the node that started the
  drag.
- **Modal capture → `capture_mouse`.** Full-screen modals (Settings, keybinding
  editor, calibration wizard, workspace trust) and the floating modal contribute
  **no boxes**; they swallow the entire mouse channel before the walk, and each
  has a bespoke `handle_*_mouse`. This is what `Modality::Exclusive` + a
  `FocusScope` replaces.
- **Hover** is two disjoint systems: a chrome hover-target walk
  (`update_hover_target`, memoized, offering enter/leave transitions to every
  component) and content trackers kept **outside** the walk on purpose — the LSP
  hover state machine and terminal-link hover, because their debounced
  request/keep-alive state cannot be expressed as an enter/leave diff. In
  `fresh-ui`, hover is framework state on the render object; a component
  *mirrors* it via `on_enter`/`on_leave` (the G1 finding in the implementation
  plan). The content trackers stay as they are — they live behind the `Host`
  leaf.

**Focus** today is not one thing: keyboard focus is the derived `KeyContext`;
"which split/buffer is active" is `Window` state guarded by `set_active_buffer`
and `focus_split` (with a pane-buffer invariant that once caused a panic);
per-popup `focused` flags; per-panel `widget_registry.focus_key`; and the
overlay-toolbar focus ring derived from layout boxes. `fresh-ui` unifies all of
these into one focus tree with scopes and a traversal policy.

### 2.9 The command → action → event pipeline (unchanged by this work)

Three deliberately separate vocabularies, and **all three survive the
migration**:

| Layer | Type | Role |
|---|---|---|
| Command | `Command` (`fresh-core/command.rs`) | user-facing, localized, context-filtered palette entry |
| Action | `Action` enum, ~230 variants (`fresh-core/action.rs`) | the rebinding & serialization currency; executed by `handle_action` |
| Event | `Event::{Insert,Delete,MoveCursor,BulkEdit,…}` | the buffer-mutation, undo, and plugin-hook unit |

The separation exists for rebindability (Actions round-trip to strings; Events
are position-specific and would not replay), undo/hooks (Events are the
transaction record), and layout-independence (`MoveUp` resolves to a concrete
`Event::MoveCursor` late). `fresh-ui` handlers return **messages**; in the editor
those messages are `Action`s (as in the design doc's `Node<Action>` examples).
The Shortcuts → Intents → Actions chain sits *in front of* this pipeline: a key
on the focus chain resolves to an `Intent`, an `Intent` resolves to an `Action`
at the focused node, and from there the existing `handle_action` pipeline is
unchanged. This is the single most important "keep" on the input side.

### 2.10 The plugin widget system

Plugins describe UI as a data tree and the host owns everything else. The wire
type is `WidgetSpec` (`fresh-core/api.rs`) — a serde-tagged, `#[ts(export)]`
**closed enum**, 19 kinds: containers (`Row`/`Col`/`LabeledSection`/`Overlay`/
`Component`/`Popup`), structural (`Spacer`/`Divider`/`HintBar`/`Raw`), controls
(`Toggle`/`Button`/`Number`/`Dropdown`/`DualList`), and data views
(`List`/`Tree`/`Text`). Plugins build it with `plugins/lib/widgets.ts` and call
`panel.set(spec)`, which issues `PluginCommand::MountWidgetPanel` /
`UpdateWidgetPanel`.

Host-side (`widgets/**`, `app/widget_runtime.rs`): one `match` on kind
(`kinds/mod.rs::behavior`) dispatches to a `WidgetImpl` per kind
(`collect`/`box_meta`/`on_key`/`on_pointer`/…). The central rule is
**spec/instance separation**: spec values are initial-only; after first render
host-owned `WidgetInstanceState` (list scroll, selection, tree expansion, text
edit state) is authoritative, keyed by the widget's stable string `key` in a
`HashMap`. Identity/diffing is manual string-key matching carried forward each
render; a `WidgetMutation` fast path (`SetValue`, `SetItems`, `SetExpandedKeys`,
`AppendTreeNodes`, …) mutates in place to dodge re-transmitting a large tree
(the `js_to_json` walk of a 5000-node tree blocks the JS thread ~1s). Events flow
back through the `widget_event` hook, delivered only to the owning plugin, with a
deliberate **one-frame lag** (PluginCommands drain on the next frame).

This is a hand-rolled, string-keyed, side-table version of exactly what
`fresh-ui` is: a retained tree with `(type, key)` identity, element-owned state,
and a reconciler. The `WidgetInstanceState` map is the "side-table problem" the
design doc's Appendix A calls out by name.

### 2.11 Settings and the controls library

Settings is **mid-unification** and instructive. Schema drives it: schemars runs
offline over the config struct, the committed `config-schema.json` is parsed into
a `SettingCategory` tree, and `build_pages` turns each entry into a
`SettingControl` (10 variants: Toggle, Number, Dropdown, Text, TextList, DualList,
Map, ObjectArray, Json, Complex). `x-` schema extensions carry UI hints
(`x-enum-from: "$themes"` pulls live theme options, etc.).

The important part is what has and has not been unified:

- **Rendering is already on the widget framework.** There is no `Control` trait
  and no per-control paint code — `view/settings/widget_map.rs` projects the full
  control state into a `WidgetSpec` every frame and renders through
  `widgets::render_spec`. The old `view/controls/*/render.rs` paint modules are
  **deleted**.
- **State, input, layout, and theming remain bespoke.** Each control keeps a
  `*State`/`*Colors`/`*Layout`/`*Event` module with hand-written
  `handle_key`/`handle_mouse`, and `view/settings/input.rs` routes keys through
  per-control editing handlers. So the same widgets have **two state stores**
  (the `SettingControl` states *and* a bridge `HashMap<key, WidgetInstanceState>`)
  and `widget_map.rs` re-seeds one from the other every frame.
- **The keybinding editor is a third, entirely separate hand-rolled modal** —
  table + search + edit dialog — that uses none of `view/controls`,
  `SettingControl`, or `WidgetSpec`.

The duplication a `fresh-ui` migration removes here is concentrated in the
control *state/input/layout/theming* layer, not rendering — and it folds three
systems (controls, plugin widgets, keybinding editor) into one.

---

## 3. Why migrate — the shared root cause

Every recurring UI bug class in the survey traces to one root, stated in
[`widget-framework-v2-review.md`](widget-framework-v2-review.md): **the tree is
flat.** No component sets `LayoutBox.parent`; `focusable`/`focus_trap`/`scroll`
are reserved and unset at chrome level. Containment is faked, precedence is
tabulated, drags are a flag ladder, modals bypass the walk. Concretely:

1. **~10 full-frame "guard" boxes** simulate "outside my rect" containment —
   `menu_close_guard`, `context_menu_close_guard`, `dock_blur`, `transient_guard`,
   `popup_guard`, `clear_explorer_menu`, `tab_menu_clear_guard`, and the prompt's
   **five** per-gesture full-frame boxes. Each is a real parent/clip/modal node's
   job, done by hand.
2. **Two precedence orderings** (pointer z-bands + `layer_rank`) that a real tree
   expresses once as tree order + stacking contexts + `Modality`.
3. **The `PointerGrab` flag ladder** — ~13 drag flags — that one per-node pointer
   capture replaces.
4. **`capture_mouse` modals** that bypass the walk, replaced by a focus-trap node
   + scrim.
5. **String-keyed side tables** (`WidgetInstanceState`, the Settings bridge map)
   that element identity replaces.
6. **Two/three parallel control vocabularies** (chrome, plugin widgets, settings
   controls) that one widget set replaces.

The design doc's Appendix A ("What this replaces in Fresh today") is the full
mapping; the survey above is where each row of it actually lives.

---

## 4. The target design — the whole UI as one `fresh-ui` tree

The end state is one `fresh-ui` description tree, rebuilt each frame from `Editor`
state, with the buffer/terminal panes as `Host` leaves. `Editor::render` stops
being an immediate-mode painter and becomes: build the tree, hand it to
`ui.frame(build(editor), size)`, fold the returned `LayoutSpec` into the ratatui
`Buffer`. Input becomes: translate the terminal event to `fresh_ui::Input`,
`ui.dispatch` it, apply the returned `Action`s through the existing pipeline.

### 4.1 The root tree

Sketch (message type is `Action`), mirroring the demo's shape and the design
doc's §15 examples:

```rust
fn build(ed: &Editor) -> Node<Action> {
    provide(&THEME, ed.theme_snapshot(),                     // §4.5
      col().children([
        menu_bar(ed).if_(ed.menu_bar_visible),               // M3
        row().flex(1).children([
            dock_column(ed).w(Cells(ed.dock_width)).if_(ed.dock.visible),  // M6
            file_explorer(ed).w(Cells(ed.explorer_width)).if_(ed.explorer_open), // M9
            split_grid(&ed.split_tree()).flex(1),            // M9 — Host leaves
        ]),
        search_options(ed).if_(ed.search_active),            // M1
        status_bar(ed).h(Cells(1)),                          // M1
        prompt_line(ed).if_(ed.prompt.is_some()),            // M5 (bottom mode)
      ])
      // overlays are children of the node they belong to, not a global z-stack:
      .child_if(ed.any_context_menu(), || context_menu(ed))  // M2
      .child_if(ed.palette_overlay(),  || palette(ed))       // M5
      .child_if(ed.any_modal(),        || modal(ed)))        // M7
}
```

The two precedence tables (§2.3) **do not survive**. Precedence is tree order plus
stacking contexts plus `Modality`; a submenu nests as a further `Layer` anchored
to its row; a modal declares `Modality::Exclusive` and everything else falls out.

### 4.2 Per-surface mapping

Each surface maps onto a small, already-built combination of primitives and
widgets. The `Scene` projection (§2.5) is the props each reads.

| Surface | `fresh-ui` expression | Notes |
|---|---|---|
| **Menu bar + dropdowns** (`MenuView`) | `row()` of `Dropdown`s; submenus are nested `Layer`s anchored to their row, `Modality::Inert`, `dismiss(OUTSIDE_POINTER∣ESCAPE)`. Mnemonics are `Intent`s on the root focusable (as in the demo). | Replaces the close-guard box, the dropdown z-number, the rank entry, and the hover auto-switch machine. Hover auto-switch is `on_enter` firing a `Toggle` message while a menu is open. |
| **Context menus** (`ContextMenuView`) | `Layer{ anchor: Point(click), place: Below, fit: FLIP∣CLAMP, modality: Inert, dismiss: OUTSIDE_POINTER∣ESCAPE }` wrapping a `List::keyed(...).autofocus()`. The menu is a child of the node it acts on, so its target is tree position, not stored state. | The demo's `context_menu` is this verbatim. Replaces the four `Window` context-menu structs' *highlight* (element state), the close-guard box, and the pre-band keyboard grab. |
| **Prompt / command palette** (`PaletteView`) | `Layer` (Center overlay or Bottom line) → `FocusScope(col([ TextField, toolbar?, row([ List::keyed(results).selected_id(...), preview? ]) ]))`. Re-key on `prompt_type` to reset editing state. | Query is *controlled* (committed to `prompt_histories`); caret/selection/scroll are element state. Selection stores the result **id**, not an index. Replaces the overlay toolbar ring, the click scrim, the position-blind wheel box, the `SearchPrompt`/`Prompt` context switch, and the manual-scroll latch. |
| **Info/hover/signature popups** (`ScenePopup`) | `Layer{ anchor: Point(caret_screen_pos), place: Below, fit, dismiss: ANY_KEY∣OUTSIDE_POINTER }` over `Viewport(TextRun::markdown(...)).selectable().max_h(n)`. Non-modal. | **Anchor needs the caret's post-layout screen position from the `Host` leaf** (§1, §2.4). The LSP hover *state machine* stays behind the leaf; only the rendered popup migrates. |
| **Splits / tabs / scrollbars** (`TabBarView`) | `split_grid` recursion: `Leaf → col([ tab_strip, row([ Focusable(Host::buffer(id)).flex(1), vscrollbar ]) ])`; `Split → flex_dir([ a, Gesture(Divider).on_press(capture_pointer), b ])`. | The divider captures the pointer on press — the whole drag mechanism, replacing the separator arm of `PointerGrab`. The active split's border is `focus_within`. Buffers/terminals are `Host` leaves (§4.4). |
| **File explorer** (`FileExplorerView`) | `Tree` (or `List::windowed` over `get_display_nodes()`), selection controlled by `Window` state, `expanded_dirs` controlled (it is serialized). Context menu as above. | The `FileTree` model (lazy `TreeNode`, sort/filter, incremental search, decorations) is app state the `Tree` renders; only rendering/hit-testing/scroll move onto the widget. |
| **Dock + floating plugin panels** (`WidgetSurfaceView`) | The `WidgetSpec` → `Node<Action>` translation (§4.6) mounted in a dock column or a `Layer`. | `WidgetInstanceState` dissolves into element state. This is the plugin-API-visible wave. |
| **Settings** | The schema-driven form built directly from `SettingControl` as `col()` of `Toggle`/`Number`/`Dropdown`/`TextField`/`DualList`/`List`/`Tree`, inside a `Modality::Exclusive` `Layer` + `FocusScope`. | Deletes `widget_map.rs` (no per-frame projection), the dual state store, the bespoke `input.rs` handlers, and `view/controls/*`. The keybinding editor becomes another form in the same modal. |
| **Status bar / search-options row** (`StatusView`) | `row().h(1)` of `TextRun`/`Button` segments; already live-derived, so the least coupled. | The M1 warm-up wave. |
| **Full-screen modals** (`TrustDialogView`, `AuxModalView`, `KeybindingEditorView`) | `Layer{ anchor: Screen(Center), modality: Exclusive, scrim: Dim, dismiss }` + `FocusScope`. | `Modality::Exclusive` subsumes whole-channel capture, `blocks_terminal_input`, and the hover/cursor suppression lists as one property. |

### 4.3 State homes

Every wave begins by classifying the surface's fields into four homes. This is
the discipline the implementation plan §6 sets out; the survey lets us fill the
column concretely.

| Home | Owner | Editor examples |
|---|---|---|
| **App state** (prop, passed down) | `Editor`/`Window` | which menu is *present*, which plugin/spec a panel mounts, `dock_width`, `menu_bar_visible`, the `SettingControl` values being edited |
| **Element state** (disposed with the widget) | the element | menu highlight, context-menu highlight, prompt scroll/caret, popup scroll, theme-info popup, dropdown open flag |
| **Framework state** (render objects) | `fresh-ui` | one focus position (replaces `key_context`, `dock.focused`, popup `focused`, `Prompt.toolbar_focus`), all `PointerGrab` drag flags (→ pointer capture), hover, multi-click detection |
| **Session state** (serialized ⇒ app state) | serde structs read by daemon/workspace/orchestrator | per-split scroll, `tab_scroll_offset`, `expanded_dirs`, `prompt.input` history |

**The invariant that guards this:** if a wave changes `workspace.rs`
serialization, something was misclassified. Persisted view state must be app
state because elements are disposed on unmount and do not survive a restart. The
library's `Persisted<T>` is for **new incidental state only** — it is *not* the
home for Fresh's existing typed, versioned serde view-state, which the daemon and
orchestrator read independently of any UI component. The restore suites
(`workspace_persistence_gates.rs`, `daemon_workspace_restore_parity.rs`, the
`orchestrator_*_restore` tests) are the guard.

Consequence worth stating plainly: **`Editor` gets smaller.** Most of its UI
fields are view state, so a wave mostly *deletes* fields — the god-object shrinks
as surfaces move to element/framework state.

### 4.4 The `Host` leaf — where the text pipeline plugs in

`fresh-ui` exports the general `HostLeaf`/`RenderObject` path (closed in R2/D12).
A `BufferHost` render object implements it:

- `layout` — takes the constraints, returns the size (fills its rect);
- `paint` — runs `SplitRenderer::render_content` into the `Geom`'s rect within
  the backend's shared cell buffer, and stashes the returned
  `view_line_mappings` + cell-theme map as its own state;
- `hit` — maps a local point to a buffer byte via the stashed mappings and
  `click_geometry`, so a click on buffer text is an ordinary hit that bubbles as
  an `Action`;
- caret — reports its screen position so the frame can place `LayoutSpec.cursor`
  and so caret-anchored popups (§4.2) can read it;
- raw input — reports whether it takes raw PTY input, so `Modality::Exclusive`
  above it derives PTY suppression (the D10 `raw_input_leaves()` mechanism)
  instead of the current `blocks_terminal_input` flag.

The `WindowBuffers::with_all_mut` disjoint borrow (`&mut buffers`,
`&SplitManager`, `&mut view_states`) still has to be handed to `render_content`;
the `Host` leaf's `paint` is where that borrow is taken, exactly as
`Editor::render` takes it today. The library never owns the buffers.

### 4.5 Theme integration

`fresh-ui` says only *where* appearance comes from: every `Item` carries a
`ThemeKey` string, and the backend maps it to colors (the demo's `style()` fn).
The editor already has a rich theme system (`view/theme`, syntect category
mapping, live preview) and a per-cell theme-provenance map. Integration is: the
TUI backend maps `ThemeKey → resolved Theme colors` (the same lookup
`*Colors::from_theme` does today), the theme is an **ambient** (`provide(&THEME,
…)`) so a theme change dirties only its dependents rather than forcing a root
rebuild, and buffer text keeps its own per-cell theming inside the `Host` leaf
untouched. `convert_buffer_colors` (256/16 fallback) stays a post-process over
the folded cell buffer.

### 4.6 The plugin boundary

A plugin already sends a whole description tree; that *is* layer 1 crossing a
wire (design §13). The migration keeps the wire type `WidgetSpec` where it is
(`fresh-core`, unchanged for compatibility) and adds a **host-side translation**
`WidgetSpec → Node<Action>` in `fresh-editor` (the M6 wave). The reconciler moves
host-side, so `WidgetInstanceState` (list scroll, tree expansion, selection)
becomes element state — a plugin re-sending its spec no longer loses scroll. Two
externally visible changes, both needing a release cycle:

- **Keyed builders require a key function.** This breaks `widgets.ts` `List`/
  `Tree` calls without keys. Ship the new builders one release ahead, deprecate
  the old ones with a load-time warning.
- **State survival changes.** A plugin that compensated for state loss on re-send
  now sees state persist. Changelog item.

The plugin vocabulary stays a **stable subset** of the internal one (no `Host`,
no `Modality::Exclusive`, no focus policies, no arbitrary `M`), versioned with a
`.d.ts`, so the internal vocabulary can evolve without breaking plugins.

---

## 5. The migration plan

This refines Part 2 of
[`widget-library-implementation-plan.md`](widget-library-implementation-plan.md)
(the M0–M9 waves, deletion ledger, and verification strategy hold as written)
with the concrete current-state findings. The wave order, acceptance test
(cell-identical output), and one-implementation-at-a-time rule are unchanged. The
additions below are the editor-specific mechanics each wave now needs.

### 5.0 M0 — the seam (pure plumbing; everything depends on it)

Four pieces, plus two the survey adds:

1. **TUI backend** in `fresh-editor`: `LayoutSpec` → ratatui `Buffer`, mapping
   `Item::theme` (`ThemeKey`) through the resolved `Theme` and preserving the
   per-cell theme map. (The `examples/interactive.rs` fold is the reference
   shape.)
2. **`HostLeaf` impls** — `BufferHost` and a terminal-grid host — delegating to
   `render_content` / the PTY renderer (§4.4).
3. **A mount point** — render a `fresh-ui` subtree into a given rect inside the
   current `Editor::render` frame, and route events landing in that rect to it.
   This is what lets waves land one surface at a time while the rest of the frame
   is untouched.
4. **Input adapter** — terminal event → `fresh_ui::Input`, and messages back out
   as `Action`s into the existing `handle_action` pipeline.
5. **(added) Caret arbitration** — one "who owns the caret this frame" decision,
   replacing `cursor_suppressed_by_late_overlay`, feeding `LayoutSpec.cursor`.
6. **(added) Geometry-cache bridge** — the `Host` leaf must publish
   `view_line_mappings` and the caret screen position so caret-anchored popups
   (still on the old path during early waves) keep working across the seam.

**Exit:** a one-line status segment renders and takes a click inside the real
editor, everything else untouched.

### 5.1 Waves (increasing risk)

| Wave | Surface | New mechanism exercised | Deletes (survey-grounded) |
|---|---|---|---|
| **M1** | Status bar, search-options row | static layout, click targets | the live-derived `status_bar_layout_now`/`search_options_layout_now` paths and their `StatusView` painters |
| **M2** ⟵ **go/no-go** | Context menus (tab / new-tab / explorer / close-split) | `Layer`, `Modality::Inert`, `dismiss`, list nav | `chrome/context_menu.rs`, its close-guard box, its `on_key` pre-band grab, its rank entry, the four `Window` context-menu highlight fields |
| **M3** | Menu bar, dropdowns, submenus | nested layers, hover auto-switch, mnemonics | `chrome/menu.rs`, the `view/ui/menu.rs` dispatch half, the menu close-guard box, the hover auto-switch machine |
| **M4** | Info/hover/signature popups, theme inspector | transient dismissal via observers, scroll, text selection | `chrome/popups.rs`, `chrome/theme_info.rs`, `view/popup_mouse.rs` remnants, the transient-dismiss pre-band stage (the LSP hover *state machine* stays behind the leaf) |
| **M5** | File browser, prompt / command palette | `FocusScope`, text input, results list, preview | `chrome/prompt.rs`, `chrome/file_browser.rs`, `view/prompt_input.rs`, the overlay toolbar ring, the click scrim, the position-blind wheel box, the manual-scroll latch |
| **M6** | Plugin panels: dock + floating | `WidgetSpec` → `Node` translation, element state replacing `WidgetInstanceState`, **plugin API change** | `widgets/kinds/*` dispatch, `widget_runtime.rs`, `WidgetInstanceState`, `WidgetMutation` fast path |
| **M7** | Modals: workspace trust, keybinding editor, calibration wizard | `Modality::Exclusive` | `chrome/modals.rs`, `capture_mouse`, `blocks_terminal_input`, the cursor/hover suppression lists, the bespoke `handle_*_mouse` |
| **M8** | Settings (+ keybinding editor form) | the largest interior; rendering already on `WidgetSpec` | `view/settings/*` control layer, `view/controls/*`, `widget_map.rs`, the dual state store, the bespoke settings `input.rs` |
| **M9** | Frame: splits, tabs, scrollbars, dock column, explorer pane | the frame itself; all else nests inside | `chrome/splits.rs`, `chrome/base.rs`, `chrome/mod.rs` (registry, `layer_rank`, `chrome_tree`), `mouse_input.rs` dispatch engines, `PointerGrab`, the chrome half of `render.rs`, `KeyContext` |

**M2 is the decision point.** It is the first wave using layers, modality,
dismissal and focus together. If the seam and the model hold there, the later
waves apply the same mechanisms; if not, the library is corrected before wave
three rather than after eight surfaces depend on it.

**M9 is last by construction.** Until it lands, `fresh-ui` surfaces are mounted
*into* the existing frame (the M0 mount point). M9 inverts the relationship: the
frame becomes a `fresh-ui` tree with `Host` leaves, and the chrome layout code
in `render.rs` is removed. When M9 lands, `app/chrome/` and the `LayoutBox` arena
no longer exist, and the two precedence tables are gone.

### 5.2 Verification

The ~312 e2e files are the primary mechanism, used as-is:

1. **Cell output stays byte-identical** per wave (the existing snapshot/visual
   harness). A diff is a defect or a reviewed intended change. Reproducing exact
   spacing — including cases that look wrong — is a real part of each wave; make
   any deliberate visual change *separately*, after the wave, so a regression is
   distinguishable from an intended change.
2. **`scene_parity.rs` passes** through every wave — the web projection has not
   diverged. As each surface migrates, its `Scene` projection becomes the
   component's props rather than a separate output, but the projected data must
   match until the web frontend consumes `LayoutSpec` directly.
3. **The standing parity oracles** (event-time geometry vs paint walk; focus
   ring) stay enabled until the surface they cover migrates, then are removed
   with it.
4. **Per-wave routing tests** — the existing precedence tests (clicks not
   reaching the buffer through a popup, modality, focus order) pass unchanged
   against the new implementation.
5. **New `LayoutSpec`-level assertions** by key are *added* alongside the cell
   assertions, not in place of them.

### 5.3 Risks and stop points

1. **L1/L2 semantics are fixed** (they are — Part 1 is done and its deviation
   register is closed). The library is frozen; a wave that needs a library change
   is a signal to stop and fix the library, not to fork behavior into the editor.
2. **Cell-identical output is a hard constraint**, and the biggest single cost
   per wave.
3. **M6 changes plugin-visible behavior** (state survival) and breaks the API
   (required keys). It needs a release cycle of its own.
4. **M8 (Settings) is optional as a stopping point.** It is the largest interior
   and the least coupled to dispatch; stopping after M7 with Settings still on
   the current (already-half-unified) path is a supported end state.
5. **Two implementations of one surface must not persist across waves.** A wave
   that cannot delete its predecessor indicates a defect in the seam — fix the
   seam rather than accumulate a second UI stack.
6. **The caret and the caret-anchored popups are the subtle seam.** Get the M0
   caret arbitration and geometry-cache bridge right, or M4/M5 popups mis-anchor.

---

## 6. Open questions

1. **Web frontend endgame.** Today the web bridge consumes `Scene` JSON for
   chrome and slices rendered cells for text. Post-migration, chrome geometry
   comes from `LayoutSpec`. Does the web frontend eventually consume `LayoutSpec`
   directly (the design's stated intent — TUI/web/test as three consumers of one
   display list), and if so, is that a wave after M9 or a parallel track? Until
   then, `scene_parity` keeps the `Scene` projection alive as a second output.
2. **`WidgetMutation` performance path.** Element state removes the *need* for the
   spec re-send, but a plugin streaming 5000 tree nodes still crosses the wire
   once. Does `List::windowed`/`Tree` incremental building over an
   index-into-plugin-storage interface fully replace `AppendTreeNodes`, or is a
   delta channel still needed at the plugin boundary?
3. **Keyboard walk asymmetry.** The current keyboard side deliberately re-derives
   context per handler because handlers "mutate then decline." `fresh-ui`'s
   Shortcuts → Intents → Actions along the focus chain assumes a claim stops the
   walk. Any surface that today relies on process-then-fall-through (the popup
   `ClosePopup` rung) needs its intent to resolve to an `Action` that *also*
   re-dispatches, or the behavior must be restructured. Enumerate these during
   M2/M4.
4. **Content hover trackers.** The LSP-hover and terminal-link state machines
   stay behind the `Host` leaf. Confirm the leaf can raise "show hover popup at
   caret" as a message the tree acts on, so the *popup* is a `fresh-ui` `Layer`
   while the *trigger* stays a debounced state machine in the leaf.
5. **Multi-window.** `Editor` holds a `windows` map; the design examples are
   single-root. Confirm each window is an independent `Ui` instance (independent
   focus/dirty/element arena) versus one tree with per-window subtrees.
