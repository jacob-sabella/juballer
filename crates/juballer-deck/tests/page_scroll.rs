//! Page scrolling + row pinning tests.
//!
//! Covers:
//! - a page with `logical_rows = 8` scrolls correctly through the logical grid;
//! - clamping at top/bottom edges;
//! - pinned rows stay fixed at their physical row regardless of scroll;
//! - scroll offset persists per (profile, page) in the state store;
//! - existing 4x4 pages are unaffected.

use juballer_deck::config::DeckPaths;
use juballer_deck::render::drain_bus;
use juballer_deck::DeckApp;
use std::path::Path;
use tempfile::TempDir;

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

/// Build a fixture profile on disk:
/// - `scrolly` page: 8 logical rows, row 3 pinned; each logical row has a label button at col 0.
/// - `plain` page: classic 4x4 page with one button.
fn fixture() -> (TempDir, DeckApp) {
    let dir = tempfile::tempdir().unwrap();
    let paths = DeckPaths::from_root(dir.path().to_path_buf());

    write(
        &paths.deck_toml,
        r#"
version = 1
active_profile = "p"

[editor]
bind = "127.0.0.1:7373"

[render]

[log]
level = "info"
"#,
    );
    write(
        &paths.profile_meta_toml("p"),
        r#"
name = "p"
default_page = "scrolly"
pages = ["scrolly", "plain"]
"#,
    );
    // Logical rows 0..=7, col 0; logical row 3 pinned.
    // Each button labels itself with its logical row so the mapping is observable.
    let mut scrolly = String::from(
        r#"
[meta]
title = "scrolly"
logical_rows = 8
logical_cols = 4
pinned_rows = [3]

"#,
    );
    for r in 0u8..8 {
        scrolly.push_str(&format!(
            "[[button]]\nrow = {r}\ncol = 0\naction = \"shell.run\"\nargs = {{ cmd = \"echo r{r}\" }}\nlabel = \"r{r}\"\n\n"
        ));
    }
    write(&paths.profile_page_toml("p", "scrolly"), &scrolly);

    write(
        &paths.profile_page_toml("p", "plain"),
        r#"
[meta]
title = "plain"

[[button]]
row = 0
col = 0
action = "shell.run"
args = { cmd = "echo hi" }
label = "hi"
"#,
    );

    let rt = tokio::runtime::Handle::current();
    let deck = DeckApp::bootstrap(paths, rt).unwrap();
    (dir, deck)
}

/// Helper — inspect the label bound at the given physical cell.
fn label_at(deck: &DeckApp, r: u8, c: u8) -> Option<String> {
    deck.bound_actions
        .get(&(r, c))
        .and_then(|b| b.label.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn classic_4x4_page_is_unaffected() {
    let (_dir, mut deck) = fixture();
    // Switch to the 4x4 page.
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "plain"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.logical_rows, 4);
    assert_eq!(deck.logical_cols, 4);
    assert!(deck.pinned_rows.is_empty());
    assert_eq!(deck.scroll_row, 0);
    assert_eq!(deck.scroll_col, 0);
    assert_eq!(label_at(&deck, 0, 0).as_deref(), Some("hi"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_scrolly_shows_first_window_with_pinned_row() {
    let (_dir, deck) = fixture();
    assert_eq!(deck.logical_rows, 8);
    assert_eq!(deck.pinned_rows, vec![3]);
    assert_eq!(deck.scroll_row, 0);

    // pinned_rows=[3] → unpinned physical rows = [0,1,2]; unpinned logical rows = [0,1,2,4,5,6,7].
    // With scroll_row=0, physical 0→log 0, physical 1→log 1, physical 2→log 2,
    //                    physical 3→log 3 (pinned).
    assert_eq!(label_at(&deck, 0, 0).as_deref(), Some("r0"));
    assert_eq!(label_at(&deck, 1, 0).as_deref(), Some("r1"));
    assert_eq!(label_at(&deck, 2, 0).as_deref(), Some("r2"));
    assert_eq!(label_at(&deck, 3, 0).as_deref(), Some("r3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scroll_down_shifts_unpinned_rows_but_preserves_pin() {
    let (_dir, mut deck) = fixture();
    deck.bus
        .publish("deck.scroll_request", serde_json::json!({"dr": 1, "dc": 0}));
    drain_bus(&mut deck);
    assert_eq!(deck.scroll_row, 1);
    // Unpinned logical rows = [0,1,2,4,5,6,7]; scroll=1 → window = indices 1..=3 = [1,2,4].
    assert_eq!(label_at(&deck, 0, 0).as_deref(), Some("r1"));
    assert_eq!(label_at(&deck, 1, 0).as_deref(), Some("r2"));
    assert_eq!(label_at(&deck, 2, 0).as_deref(), Some("r4"));
    // Physical row 3 is pinned to logical row 3 regardless of scroll.
    assert_eq!(label_at(&deck, 3, 0).as_deref(), Some("r3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scroll_clamps_at_bottom() {
    let (_dir, mut deck) = fixture();
    // Try to scroll further than possible. Unpinned logical rows = 7, unpinned physical rows = 3,
    // so max scroll_row = 7 - 3 = 4.
    for _ in 0..10 {
        deck.bus
            .publish("deck.scroll_request", serde_json::json!({"dr": 1, "dc": 0}));
        drain_bus(&mut deck);
    }
    assert_eq!(deck.scroll_row, 4);
    // With scroll_row=4: window = indices 4..=6 of [0,1,2,4,5,6,7] = [5,6,7].
    assert_eq!(label_at(&deck, 0, 0).as_deref(), Some("r5"));
    assert_eq!(label_at(&deck, 1, 0).as_deref(), Some("r6"));
    assert_eq!(label_at(&deck, 2, 0).as_deref(), Some("r7"));
    assert_eq!(label_at(&deck, 3, 0).as_deref(), Some("r3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scroll_up_clamps_at_zero() {
    let (_dir, mut deck) = fixture();
    deck.bus.publish(
        "deck.scroll_request",
        serde_json::json!({"dr": -1, "dc": 0}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.scroll_row, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scroll_offset_persists_across_page_switches() {
    let (_dir, mut deck) = fixture();
    // Scroll twice.
    for _ in 0..2 {
        deck.bus
            .publish("deck.scroll_request", serde_json::json!({"dr": 1, "dc": 0}));
        drain_bus(&mut deck);
    }
    assert_eq!(deck.scroll_row, 2);
    // Switch away and back.
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "plain"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.scroll_row, 0); // plain page has no saved offset.
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "scrolly"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.scroll_row, 2);
    // Physical row 0 should again show logical row 2.
    assert_eq!(label_at(&deck, 0, 0).as_deref(), Some("r2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scroll_emits_action_scroll_bus_event() {
    let (_dir, mut deck) = fixture();
    let mut rx = deck.bus.subscribe();
    deck.bus
        .publish("deck.scroll_request", serde_json::json!({"dr": 1, "dc": 0}));
    drain_bus(&mut deck);
    // We should see the scroll_request AND an action.scroll echo.
    let mut saw_scroll_ack = false;
    while let Ok(ev) = rx.try_recv() {
        if ev.topic == "action.scroll" {
            saw_scroll_ack = true;
            assert_eq!(ev.data["row"].as_u64(), Some(1));
            assert_eq!(ev.data["logical_rows"].as_u64(), Some(8));
        }
    }
    assert!(saw_scroll_ack, "expected action.scroll bus event");
}
