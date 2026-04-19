//! End-to-end smoke: load the fixture config, bootstrap the deck, dispatch a synthetic
//! button-0,0 press, wait for the tokio side-effect, assert bus received the result event.

use juballer_deck::action::ActionCx;
use juballer_deck::config::DeckPaths;
use juballer_deck::tile::{TileHandle, TileState};
use juballer_deck::DeckApp;
use std::path::PathBuf;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_action_fires_and_publishes_result() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal");
    let paths = DeckPaths::from_root(fixture);
    let rt = tokio::runtime::Handle::current();

    let mut deck = DeckApp::bootstrap(paths, rt.clone()).unwrap();
    assert!(deck.bound_actions.contains_key(&(0, 0)));

    let mut rx = deck.bus.subscribe();

    // Simulate a button-down by calling on_down directly.
    let bound = deck.bound_actions.get_mut(&(0, 0)).unwrap();
    let tile_state = &mut TileState::default();
    let env = deck.config.active_profile().unwrap().meta.env.clone();
    let binding_id = bound.binding_id.clone();
    {
        let mut cx = ActionCx {
            cell: (0, 0),
            binding_id: &binding_id,
            tile: TileHandle::new(tile_state),
            env: &env,
            bus: &deck.bus,
            state: &mut deck.state,
            rt: &rt,
        };
        bound.action.on_down(&mut cx);
    }

    // Wait for the spawned task to complete and publish to the bus.
    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("bus recv timed out")
        .expect("bus channel closed");
    assert_eq!(ev.topic, "action.shell.run:home:0,0");
    assert!(ev.data.get("status").is_some() || ev.data.get("error").is_some());
}
