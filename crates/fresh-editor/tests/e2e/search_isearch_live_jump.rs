//! E2E for emacs-isearch live jumping: while the search prompt is open,
//! each keystroke of the query must move the cursor to the match the
//! growing query selects (first match at or after where the prompt
//! opened) and report the live `match N of M` count — not merely paint
//! highlights and leave the cursor frozen until the search is confirmed.

use crate::common::harness::{EditorTestHarness, HarnessOptions};
use crossterm::event::{KeyCode, KeyModifiers};
use fresh::config::Config;

#[test]
fn typing_in_search_prompt_jumps_to_match_live() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let file_path = temp_dir.path().join("targets.txt");
    // Cursor starts at 0; the only `two` is on line 3, so the live jump
    // must visibly move the cursor before any Enter.
    std::fs::write(&file_path, "alpha one\nfiller\nalpha two\n").unwrap();

    let mut config = Config::default();
    config.editor.search_jump_while_typing = true;
    let mut harness = EditorTestHarness::create(
        100,
        24,
        HarnessOptions::new()
            .without_empty_plugins_dir()
            .with_config(config),
    )
    .unwrap();
    harness.open_file(&file_path).unwrap();
    harness.render().unwrap();
    assert_eq!(harness.cursor_position(), 0);

    // Open the search prompt (default keymap: Ctrl+F).
    harness
        .send_key(KeyCode::Char('f'), KeyModifiers::CONTROL)
        .unwrap();
    harness.render().unwrap();
    harness.wait_for_prompt().unwrap();

    // Type `alp` — a prefix that already matches. The cursor must leave
    // position 0 and land on the first match at or after the prompt's
    // origin (also 0): `alpha` at byte 0 is the first, so typing the full
    // word and beyond the origin needs a distinct target. Instead type
    // `two`: its only match is on line 3, so the cursor must visibly jump
    // there before any Enter.
    harness.type_text("two").unwrap();
    harness.render().unwrap();

    let two_pos = "alpha one\nfiller\nalpha two\n".find("two").unwrap();
    assert_eq!(
        harness.cursor_position(),
        two_pos,
        "typing in the search prompt must jump the cursor to the match \
         (emacs isearch); screen:\n{}",
        harness.screen_to_string()
    );

    // The live count appears on the status bar.
    harness
        .wait_until(|h| h.screen_to_string().contains("Match 1 of 1"))
        .unwrap();
}
