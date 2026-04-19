//! Quick smoke test for `carla::listener` against a live Carla.
//!
//! Run a Carla server (default `127.0.0.1:22752`) — e.g.
//! `carla --no-gui /path/to/project.carxp` — and then:
//!
//! ```sh
//! cargo run -p juballer-deck --example carla_listener_smoke
//! ```
//!
//! Prints every feed change (params + peaks) for 6 seconds.

use juballer_deck::carla::config::{DEFAULT_CARLA_HOST, DEFAULT_CARLA_PORT};
use juballer_deck::carla::listener;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::time::{Duration, Instant};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "juballer::carla=info".into()),
        )
        .init();

    let key = format!("{DEFAULT_CARLA_HOST}:{DEFAULT_CARLA_PORT}");
    let target = key.to_socket_addrs().unwrap().next().unwrap();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = listener::spawn(rt.handle(), target).expect("listener spawn");
    let feed = listener.feed();

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut last_params: HashMap<(u32, u32), f32> = HashMap::new();
    let mut last_peaks: HashMap<u32, [f32; 4]> = HashMap::new();
    let mut peak_print_counter: u32 = 0;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(150));
        let snapshot = match feed.read() {
            Ok(g) => g,
            Err(_) => continue,
        };
        for ((plugin, param), value) in &snapshot.params {
            if last_params.get(&(*plugin, *param)) != Some(value) {
                println!("[param] plugin={plugin:>2} param={param:>3} value={value:.6}");
                last_params.insert((*plugin, *param), *value);
            }
        }
        // Peaks update at audio rate — print every ~5th poll to keep
        // the log readable while still surfacing motion.
        peak_print_counter += 1;
        if peak_print_counter % 5 == 0 {
            for (plugin, peaks) in &snapshot.peaks {
                if last_peaks.get(plugin) != Some(peaks) {
                    println!("[peaks] plugin={plugin:>2} {peaks:?}");
                    last_peaks.insert(*plugin, *peaks);
                }
            }
        }
    }

    println!("--- final snapshot ---");
    let g = feed.read().unwrap();
    println!("params: {} entries", g.params.len());
    for ((plugin, param), value) in &g.params {
        println!("  ({plugin}, {param}) = {value}");
    }
    println!("peaks: {} entries", g.peaks.len());
    for (plugin, peaks) in &g.peaks {
        println!("  {plugin}: {peaks:?}");
    }

    listener.shutdown();
    std::thread::sleep(Duration::from_millis(200));
    println!("done. seen_first_message={}", g.seen_first_message);
}
