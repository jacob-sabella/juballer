//! Bus dispatch tests for the deck nav events + widget.action_request.

use juballer_deck::config::DeckPaths;
use juballer_deck::render::drain_bus;
use juballer_deck::DeckApp;
use std::path::PathBuf;

fn fixture() -> DeckApp {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multipage");
    let paths = DeckPaths::from_root(path);
    let rt = tokio::runtime::Handle::current();
    DeckApp::bootstrap(paths, rt).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_switch_request_changes_active_page() {
    let mut deck = fixture();
    assert_eq!(deck.active_page, "home");
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "media"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "media");
    assert_eq!(deck.page_history, vec!["home".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_switch_reapplies_top_layout() {
    let mut deck = fixture();
    assert_eq!(
        deck.last_applied_top_pane_names,
        vec!["home_top_a".to_string(), "home_top_b".to_string()]
    );
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "media"}),
    );
    drain_bus(&mut deck);
    assert_eq!(
        deck.last_applied_top_pane_names,
        vec!["media_top_x".to_string()]
    );
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "settings"}),
    );
    drain_bus(&mut deck);
    assert!(deck.last_applied_top_pane_names.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cycle_request_reapplies_top_layout() {
    let mut deck = fixture();
    deck.bus.publish(
        "deck.cycle_request",
        serde_json::json!({"pages": ["home", "media"], "direction": 1}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "media");
    assert_eq!(
        deck.last_applied_top_pane_names,
        vec!["media_top_x".to_string()]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_switch_reapplies_top_layout() {
    let mut deck = fixture();
    deck.bus.publish(
        "deck.profile_switch_request",
        serde_json::json!({"profile": "alt"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.config.deck.active_profile, "alt");
    assert!(deck.last_applied_top_pane_names.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_back_pops_history() {
    let mut deck = fixture();
    deck.bus.publish(
        "deck.page_switch_request",
        serde_json::json!({"page": "media"}),
    );
    drain_bus(&mut deck);
    deck.bus
        .publish("deck.page_back_request", serde_json::json!({}));
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "home");
    assert!(deck.page_history.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn page_back_with_empty_history_is_noop() {
    let mut deck = fixture();
    deck.bus
        .publish("deck.page_back_request", serde_json::json!({}));
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "home");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cycle_request_steps_forward_and_back() {
    let mut deck = fixture();
    let pages = serde_json::json!(["home", "media", "settings"]);
    deck.bus.publish(
        "deck.cycle_request",
        serde_json::json!({"pages": pages, "direction": 1}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "media");
    deck.bus.publish(
        "deck.cycle_request",
        serde_json::json!({"pages": ["home", "media", "settings"], "direction": 1}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "settings");
    deck.bus.publish(
        "deck.cycle_request",
        serde_json::json!({"pages": ["home", "media", "settings"], "direction": -1}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "media");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_switch_request_changes_active_profile_and_rebinds() {
    let mut deck = fixture();
    deck.bus.publish(
        "deck.profile_switch_request",
        serde_json::json!({"profile": "alt"}),
    );
    drain_bus(&mut deck);
    assert_eq!(deck.config.deck.active_profile, "alt");
    assert_eq!(deck.active_page, "home");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn widget_action_request_unknown_action_does_not_panic() {
    let mut deck = fixture();
    deck.bus.publish(
        "widget.action_request",
        serde_json::json!({"action": "no.such.action"}),
    );
    drain_bus(&mut deck);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn widget_action_request_invokes_known_action() {
    let mut deck = fixture();
    let mut rx = deck.bus.subscribe();
    deck.bus.publish(
        "widget.action_request",
        serde_json::json!({
            "action": "deck.page_goto",
            "args": {"page": "settings"},
        }),
    );
    drain_bus(&mut deck);
    // The page_goto action publishes deck.page_switch_request — drain again to apply.
    drain_bus(&mut deck);
    assert_eq!(deck.active_page, "settings");
    // Drain the residual events from rx so the channel doesn't lag in this test.
    while rx.try_recv().is_ok() {}
}
