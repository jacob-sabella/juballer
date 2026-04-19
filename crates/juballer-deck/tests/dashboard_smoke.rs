//! Dashboard smoke: loads the multi-widget fixture, asserts all 4 widgets + 1 button
//! were instantiated by the registries.

use juballer_deck::config::DeckPaths;
use juballer_deck::DeckApp;
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dashboard_fixture_instantiates_widgets_and_buttons() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dashboard");
    let paths = DeckPaths::from_root(fixture);
    let rt = tokio::runtime::Handle::current();

    let deck = DeckApp::bootstrap(paths, rt).unwrap();

    // 1 button bound.
    assert!(deck.bound_actions.contains_key(&(0, 0)));

    // 4 widgets active.
    assert_eq!(deck.active_widgets.len(), 4);
    assert!(deck.active_widgets.contains_key("clock_pane"));
    assert!(deck.active_widgets.contains_key("sys_pane"));
    assert!(deck.active_widgets.contains_key("log_pane"));
    assert!(deck.active_widgets.contains_key("probe_pane"));
}
