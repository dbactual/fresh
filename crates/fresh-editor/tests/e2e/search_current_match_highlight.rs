//! E2E for the current-search-match highlight: while a search is active
//! with multiple visible matches, the match containing the primary cursor
//! must be styled with the dedicated current-match colors
//! (`search.current_match_bg` / `current_match_fg`) while every other
//! match keeps the plain `search.match_bg` / `match_fg` — the emacs
//! isearch vs lazy-highlight distinction. Without it the user has no
//! on-screen indication of which match find-next will advance from.

use crate::common::harness::EditorTestHarness;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Color;

#[test]
fn current_search_match_is_styled_apart_from_other_matches() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let file_path = temp_dir.path().join("needle.txt");
    std::fs::write(&file_path, "needle one\nmiddle\nneedle two\n").unwrap();

    let mut harness = EditorTestHarness::new(100, 24).unwrap();
    harness.open_file(&file_path).unwrap();
    harness.render().unwrap();

    // Open the search prompt (default keymap: Ctrl+F) and type the query.
    // Incremental highlighting runs while the prompt is open.
    harness
        .send_key(KeyCode::Char('f'), KeyModifiers::CONTROL)
        .unwrap();
    harness.render().unwrap();
    harness.type_text("needle").unwrap();
    harness.render().unwrap();

    // Both occurrences must be on screen and highlighted before we
    // compare styles.
    harness
        .wait_until(|h| h.count_search_highlights() >= 2)
        .unwrap();

    // The cursor sits on the first match ("needle" at line 1); the second
    // match ("needle" at line 3) is the "other" one. Search for the full
    // line text so the buffer's own matches are found, not the tab-bar
    // filename or the status line.
    let first = harness
        .find_text_on_screen("needle one")
        .expect("first match on screen");
    let second = harness
        .find_text_on_screen("needle two")
        .expect("second match on screen");
    assert_ne!(
        first.1, second.1,
        "the two matches must be on different rows"
    );

    let current_style = harness
        .get_cell_style(first.0, first.1)
        .expect("style at first match");
    let other_style = harness
        .get_cell_style(second.0, second.1)
        .expect("style at second match");

    // The harness's default theme is `high-contrast`: its search colors are
    //   match: fg black on rgb(255,255,0)
    // and it does not override the current-match colors, so those fall back
    // to the code defaults: fg black on gold rgb(255,215,0).
    let expected_current_bg = Color::Rgb(255, 215, 0);
    let expected_other_bg = Color::Rgb(255, 255, 0);

    assert_eq!(
        current_style.bg,
        Some(expected_current_bg),
        "the match at the cursor must use current_match_bg; got {:?}",
        current_style
    );
    assert_eq!(
        other_style.bg,
        Some(expected_other_bg),
        "the other match must use match_bg; got {:?}",
        other_style
    );
    assert_ne!(
        current_style.bg, other_style.bg,
        "current and other matches must be visually distinct"
    );

    // Stepping to the next match (Enter commits; Ctrl+F3 steps) must move
    // the distinct highlight to the second occurrence.
    harness
        .send_key(KeyCode::Enter, KeyModifiers::NONE)
        .unwrap();
    harness.process_async_and_render().unwrap();
    harness.send_key(KeyCode::F(3), KeyModifiers::NONE).unwrap();
    harness.process_async_and_render().unwrap();

    let now_current = harness
        .get_cell_style(second.0, second.1)
        .expect("style at second match after stepping");
    let now_other = harness
        .get_cell_style(first.0, first.1)
        .expect("style at first match after stepping");
    assert_eq!(
        now_current.bg,
        Some(expected_current_bg),
        "after find-next the second match must carry the current style; got {:?}",
        now_current
    );
    assert_eq!(
        now_other.bg,
        Some(expected_other_bg),
        "after find-next the first match must revert to the plain style; got {:?}",
        now_other
    );
}
