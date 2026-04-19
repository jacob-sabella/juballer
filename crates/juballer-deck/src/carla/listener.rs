//! Read-side OSC listener for Carla's reply stream.
//!
//! Phase 2 wires this in alongside the write-side [`super::osc`] client
//! so display cells can show live values pulled from the running Carla.
//! On spawn the listener:
//!
//! 1. Binds an ephemeral UDP socket on the loopback interface.
//! 2. Sends `/register osc.udp://127.0.0.1:<port>/Carla` to the target
//!    (the path Carla 2.x advertises on; the `/Carla` suffix is what
//!    Carla prepends to every callback it pushes).
//! 3. Decodes incoming packets and updates a shared
//!    [`Arc<RwLock<CarlaFeed>>`] that the renderer reads each frame.
//! 4. On [`CarlaListener::shutdown`], sends `/unregister` so the live
//!    Carla stops pushing into a port we no longer own.
//!
//! Observed packet types (Carla 2.6.0-alpha1, JACK engine):
//! - `/Carla/param  <i:plugin>  <i:param>  <f:value>`
//! - `/Carla/peaks  <i:plugin>  <f:in_l> <f:in_r> <f:out_l> <f:out_r>`
//!
//! The schema does not advertise plugin / parameter names — name → index
//! resolution is a separate Phase 2.1 concern (parse Carla project files
//! or send `/Carla/<id>/get_real_plugin_name`-style queries). For now,
//! display cells are bound by numeric index just like input cells.

use crate::Result;
use rosc::{decoder, encoder, OscMessage, OscPacket, OscType};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// Live state pushed by Carla into the registered URL. Cheap to clone
/// (every clone shares the inner `Arc`); read with `read()` for cheap
/// per-frame snapshots in the renderer.
#[derive(Debug, Default)]
pub struct CarlaFeed {
    /// `(plugin_id, param_id) -> value`. Last-write-wins; Carla sends
    /// updates whenever a parameter changes, plus periodic resyncs.
    pub params: HashMap<(u32, u32), f32>,
    /// `plugin_id -> [in_l, in_r, out_l, out_r]` peak meters in 0..=1.
    pub peaks: HashMap<u32, [f32; 4]>,
    /// True after the registration handshake has produced at least one
    /// update — useful to distinguish "Carla is connected but quiet"
    /// from "we never heard back at all".
    pub seen_first_message: bool,
}

impl CarlaFeed {
    pub fn param(&self, plugin: u32, param: u32) -> Option<f32> {
        self.params.get(&(plugin, param)).copied()
    }

    pub fn peaks_for(&self, plugin: u32) -> Option<[f32; 4]> {
        self.peaks.get(&plugin).copied()
    }
}

/// Handle to the listener task. Exposes the shared feed plus a manual
/// shutdown for the rare case where the deck wants to disconnect
/// without dropping the carla mode entirely.
#[derive(Debug, Clone)]
pub struct CarlaListener {
    feed: Arc<RwLock<CarlaFeed>>,
    shutdown_tx: mpsc::Sender<()>,
    bound: SocketAddr,
}

impl CarlaListener {
    pub fn feed(&self) -> Arc<RwLock<CarlaFeed>> {
        self.feed.clone()
    }

    pub fn bound(&self) -> SocketAddr {
        self.bound
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.try_send(());
    }
}

/// Spawn the listener task on `rt`. Binds to `0.0.0.0:0`, registers
/// with `target` (Carla's OSC server), and drives the receive loop.
/// Returns the handle plus the shared feed.
pub fn spawn(rt: &tokio::runtime::Handle, target: SocketAddr) -> Result<CarlaListener> {
    let std_sock = std::net::UdpSocket::bind(("0.0.0.0", 0))?;
    std_sock.set_nonblocking(true)?;
    let bound = std_sock.local_addr()?;
    // tokio::net::UdpSocket::from_std needs to be called from inside
    // the runtime so the I/O reactor can register the fd. Enter the
    // runtime briefly even when the caller is on a non-tokio thread.
    let _guard = rt.enter();
    let socket = UdpSocket::from_std(std_sock)?;

    // Register URL must be reachable from Carla's perspective. We bind
    // 0.0.0.0 on our side (so localhost packets land) but advertise
    // 127.0.0.1 in the registration so Carla doesn't try to reach us
    // over a routed path.
    let url = format!("osc.udp://127.0.0.1:{}/Carla", bound.port());
    let register = encoder::encode(&OscPacket::Message(OscMessage {
        addr: "/register".into(),
        args: vec![OscType::String(url.clone())],
    }))
    .map_err(|e| crate::Error::Config(format!("encode /register: {e}")))?;

    let feed = Arc::new(RwLock::new(CarlaFeed::default()));
    let feed_for_task = feed.clone();
    let unregister_url = url.clone();
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    rt.spawn(async move {
        // Send /register from inside the runtime so the tokio socket is
        // fully ready. Sending via try_send_to from spawn() raced with
        // the runtime not yet polling the socket and produced flaky
        // WouldBlock errors when the rest of the test suite ran in
        // parallel.
        if let Err(e) = socket.send_to(&register, target).await {
            tracing::warn!(
                target: "juballer::carla::listener",
                "send /register to {target}: {e}"
            );
            return;
        }
        tracing::info!(
            target: "juballer::carla::listener",
            "registered as {url} (carla target {target})"
        );
        let mut buf = vec![0u8; 8192];
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    let unregister = encoder::encode(&OscPacket::Message(OscMessage {
                        addr: "/unregister".into(),
                        args: vec![OscType::String(unregister_url.clone())],
                    })).unwrap_or_default();
                    let _ = socket.send_to(&unregister, target).await;
                    tracing::info!(target: "juballer::carla::listener", "unregistered + exiting");
                    break;
                }
                recv = socket.recv_from(&mut buf) => match recv {
                    Ok((n, _src)) => {
                        if let Ok((_, pkt)) = decoder::decode_udp(&buf[..n]) {
                            apply_packet(&feed_for_task, &pkt);
                        } else {
                            tracing::trace!(
                                target: "juballer::carla::listener",
                                "decode failed: {n}-byte packet dropped"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(target: "juballer::carla::listener", "recv: {e}");
                        break;
                    }
                }
            }
        }
    });

    Ok(CarlaListener {
        feed,
        shutdown_tx,
        bound,
    })
}

/// Apply one decoded OSC packet to the shared feed. Pulled out so unit
/// tests can drive it directly without spinning a tokio task.
fn apply_packet(feed: &Arc<RwLock<CarlaFeed>>, packet: &OscPacket) {
    match packet {
        OscPacket::Message(msg) => apply_message(feed, msg),
        OscPacket::Bundle(bundle) => {
            for child in &bundle.content {
                apply_packet(feed, child);
            }
        }
    }
}

fn apply_message(feed: &Arc<RwLock<CarlaFeed>>, msg: &OscMessage) {
    let mut guard = match feed.write() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    guard.seen_first_message = true;
    match msg.addr.as_str() {
        "/Carla/param" => {
            if let (Some(plugin), Some(param), Some(value)) = (
                msg.args.first().and_then(arg_as_u32),
                msg.args.get(1).and_then(arg_as_u32),
                msg.args.get(2).and_then(arg_as_f32),
            ) {
                guard.params.insert((plugin, param), value);
            }
        }
        "/Carla/peaks" => {
            if let Some(plugin) = msg.args.first().and_then(arg_as_u32) {
                let mut peaks = [0.0f32; 4];
                for (i, slot) in peaks.iter_mut().enumerate() {
                    if let Some(v) = msg.args.get(1 + i).and_then(arg_as_f32) {
                        *slot = v;
                    }
                }
                guard.peaks.insert(plugin, peaks);
            }
        }
        _ => {
            // Unknown messages (e.g. /Carla/runtime, future additions)
            // just bump the seen-first flag and drop. Avoid logging here
            // — Carla pushes these every ~50 ms and the noise drowns
            // out everything else.
        }
    }
}

fn arg_as_u32(arg: &OscType) -> Option<u32> {
    match arg {
        OscType::Int(i) => u32::try_from(*i).ok(),
        OscType::Long(i) => u32::try_from(*i).ok(),
        _ => None,
    }
}

fn arg_as_f32(arg: &OscType) -> Option<f32> {
    match arg {
        OscType::Float(f) => Some(*f),
        OscType::Double(d) => Some(*d as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::OscMessage;

    fn make_feed() -> Arc<RwLock<CarlaFeed>> {
        Arc::new(RwLock::new(CarlaFeed::default()))
    }

    #[test]
    fn apply_param_message_records_value_and_marks_seen() {
        let feed = make_feed();
        apply_message(
            &feed,
            &OscMessage {
                addr: "/Carla/param".into(),
                args: vec![OscType::Int(2), OscType::Int(5), OscType::Float(0.42)],
            },
        );
        let g = feed.read().unwrap();
        assert!(g.seen_first_message);
        assert_eq!(g.param(2, 5), Some(0.42));
    }

    #[test]
    fn apply_peaks_message_records_four_floats() {
        let feed = make_feed();
        apply_message(
            &feed,
            &OscMessage {
                addr: "/Carla/peaks".into(),
                args: vec![
                    OscType::Int(1),
                    OscType::Float(0.1),
                    OscType::Float(0.2),
                    OscType::Float(0.3),
                    OscType::Float(0.4),
                ],
            },
        );
        let g = feed.read().unwrap();
        assert_eq!(g.peaks_for(1), Some([0.1, 0.2, 0.3, 0.4]));
    }

    #[test]
    fn apply_param_with_long_plugin_id_still_decodes() {
        let feed = make_feed();
        apply_message(
            &feed,
            &OscMessage {
                addr: "/Carla/param".into(),
                args: vec![OscType::Long(7), OscType::Long(3), OscType::Double(0.9)],
            },
        );
        let g = feed.read().unwrap();
        assert_eq!(g.param(7, 3), Some(0.9));
    }

    #[test]
    fn unknown_messages_only_bump_the_seen_flag() {
        let feed = make_feed();
        apply_message(
            &feed,
            &OscMessage {
                addr: "/Carla/runtime".into(),
                args: vec![OscType::Float(0.5), OscType::Int(0)],
            },
        );
        let g = feed.read().unwrap();
        assert!(g.seen_first_message);
        assert!(g.params.is_empty());
        assert!(g.peaks.is_empty());
    }

    #[test]
    fn malformed_param_message_is_ignored_without_panicking() {
        let feed = make_feed();
        apply_message(
            &feed,
            &OscMessage {
                addr: "/Carla/param".into(),
                args: vec![OscType::String("nope".into())],
            },
        );
        let g = feed.read().unwrap();
        assert!(g.params.is_empty());
        assert!(g.seen_first_message);
    }

    #[test]
    fn apply_packet_walks_bundle_contents_recursively() {
        let feed = make_feed();
        let bundle = OscPacket::Bundle(rosc::OscBundle {
            timetag: rosc::OscTime {
                seconds: 0,
                fractional: 0,
            },
            content: vec![
                OscPacket::Message(OscMessage {
                    addr: "/Carla/param".into(),
                    args: vec![OscType::Int(0), OscType::Int(0), OscType::Float(1.0)],
                }),
                OscPacket::Message(OscMessage {
                    addr: "/Carla/param".into(),
                    args: vec![OscType::Int(0), OscType::Int(1), OscType::Float(2.0)],
                }),
            ],
        });
        apply_packet(&feed, &bundle);
        let g = feed.read().unwrap();
        assert_eq!(g.param(0, 0), Some(1.0));
        assert_eq!(g.param(0, 1), Some(2.0));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_round_trip_with_fake_carla_server() {
        // Stand up a "fake Carla": bind a UDP socket on a kernel port,
        // wait for /register, then push one /Carla/param packet back
        // to the registered URL. Verify the listener's feed picks it up.
        // Using an async UdpSocket on the receiver side too so the
        // whole test stays inside the tokio runtime — spawn_blocking
        // raced badly when run in parallel with the rest of the suite.
        let std_carla = std::net::UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        std_carla.set_nonblocking(true).unwrap();
        let carla_addr = std_carla.local_addr().unwrap();
        let carla_async = UdpSocket::from_std(std_carla).unwrap();

        let rt = tokio::runtime::Handle::current();
        let listener = spawn(&rt, carla_addr).expect("listener should spawn");
        let feed = listener.feed();

        let mut buf = [0u8; 1024];
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_millis(2000),
            carla_async.recv_from(&mut buf),
        )
        .await
        .expect("/register should arrive within 2s")
        .expect("recv_from should succeed");
        let (_, pkt) = decoder::decode_udp(&buf[..n]).unwrap();
        let OscPacket::Message(msg) = pkt else {
            panic!("expected /register message");
        };
        assert_eq!(msg.addr, "/register");
        let url = match &msg.args[0] {
            OscType::String(s) => s.clone(),
            _ => panic!("expected /register URL string"),
        };
        // Parse host:port out of osc.udp://host:port/Carla
        let stripped = url.trim_start_matches("osc.udp://");
        let host_port = stripped.split('/').next().unwrap();
        let listener_addr: SocketAddr = host_port.parse().unwrap();

        let push_pkt = OscPacket::Message(OscMessage {
            addr: "/Carla/param".into(),
            args: vec![OscType::Int(3), OscType::Int(7), OscType::Float(0.55)],
        });
        let push_bytes = encoder::encode(&push_pkt).unwrap();
        carla_async
            .send_to(&push_bytes, listener_addr)
            .await
            .unwrap();

        let value = tokio::time::timeout(std::time::Duration::from_millis(2000), async {
            loop {
                if let Some(v) = feed.read().unwrap().param(3, 7) {
                    return v;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("value should land within 2s");
        assert!((value - 0.55).abs() < 1e-6);

        listener.shutdown();
    }
}
