//! Send-side OSC client for Carla's built-in server.
//!
//! Phase 1 is **write-only**: juballer pushes parameter / program /
//! custom-data updates to Carla and never reads back. Name → index
//! resolution for plugin and parameter references requires Carla's
//! `/Carla/register` reply stream; Phase 2 wires that in. Until then,
//! [`PluginRef::Name`] and [`ParamRef::Name`] log a warning and skip
//! dispatch — only numeric indices actually fire OSC messages.
//!
//! Architecture: a small tokio task owns the UDP socket and pulls
//! [`OscCommand`]s off a bounded mpsc channel. The public
//! [`CarlaClient`] handle is cheap to clone and the producer side
//! never blocks the deck event loop; if the channel is saturated the
//! command is dropped with a warning rather than queued indefinitely.

use crate::carla::config::{ParamRef, PluginRef};
use crate::Result;
use rosc::{encoder, OscMessage, OscPacket, OscType};
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// Outbound channel depth. Each OSC message is small (well under one
/// MTU) so a few dozen in flight is plenty for the burst that follows
/// a fast finger; if you saturate it you have a real problem upstream.
const COMMAND_QUEUE_DEPTH: usize = 64;

/// Handle to the Carla OSC sender task. Clone freely — every clone
/// shares the underlying mpsc sender. Producers never block the deck
/// loop: a full queue drops the command and logs a warning.
#[derive(Debug, Clone)]
pub struct CarlaClient {
    tx: mpsc::Sender<OscCommand>,
}

/// Internal command enum. Public callers use the typed methods on
/// [`CarlaClient`]; this enum is `pub(crate)` only so tests can build
/// expected packets directly.
#[derive(Debug, PartialEq)]
pub(crate) enum OscCommand {
    SetParameterValue {
        plugin_id: u32,
        param_id: u32,
        value: f32,
    },
    SetProgram {
        plugin_id: u32,
        program: u32,
    },
    SetCustomData {
        plugin_id: u32,
        kind: String,
        key: String,
        value: String,
    },
    /// `/Carla/<plugin>/set_chunk <s:base64>`. Used by VST2 / VST3
    /// preset application — the plugin restores its own state from
    /// the chunk blob.
    SetChunk {
        plugin_id: u32,
        chunk: String,
    },
    Shutdown,
}

impl CarlaClient {
    /// Spawn the sender task on `rt`. Returns a handle that publishes
    /// commands over the channel; the task exits when the handle is
    /// dropped (channel close) or when [`CarlaClient::shutdown`] is
    /// called.
    pub fn spawn(rt: &tokio::runtime::Handle, target: SocketAddr) -> Result<Self> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;
        // tokio::net::UdpSocket::from_std needs the runtime context so
        // its I/O reactor can register the fd; enter() guard handles
        // the case where the caller is on a non-tokio thread (e.g. the
        // CLI main thread, the deck event loop).
        let _guard = rt.enter();
        let socket = UdpSocket::from_std(socket)?;

        let (tx, mut rx) = mpsc::channel::<OscCommand>(COMMAND_QUEUE_DEPTH);
        rt.spawn(async move {
            tracing::info!(
                target: "juballer::carla::osc",
                "carla osc sender task started, target={target}"
            );
            while let Some(cmd) = rx.recv().await {
                if matches!(cmd, OscCommand::Shutdown) {
                    break;
                }
                if let Err(e) = dispatch(&socket, target, &cmd).await {
                    tracing::warn!(
                        target: "juballer::carla::osc",
                        "dispatch {cmd:?} failed: {e}"
                    );
                }
            }
            tracing::info!(target: "juballer::carla::osc", "carla osc sender task exited");
        });

        Ok(Self { tx })
    }

    /// `/Carla/<plugin>/set_parameter_value <int param> <float value>`.
    pub fn set_parameter_value(&self, plugin: &PluginRef, param: &ParamRef, value: f32) {
        let Some(plugin_id) = resolve_index(plugin, "plugin") else {
            return;
        };
        let Some(param_id) = resolve_index(param, "param") else {
            return;
        };
        self.send(OscCommand::SetParameterValue {
            plugin_id,
            param_id,
            value,
        });
    }

    /// `/Carla/<plugin>/set_program <int program>`. Used by Phase 3
    /// preset triggers to switch a plugin's built-in program slot.
    pub fn set_program(&self, plugin: &PluginRef, program: u32) {
        let Some(plugin_id) = resolve_index(plugin, "plugin") else {
            return;
        };
        self.send(OscCommand::SetProgram { plugin_id, program });
    }

    /// `/Carla/<plugin>/set_chunk <string base64>`. Used by Phase 5.2
    /// preset application — restores a VST2 / VST3 plugin's full
    /// internal state from the base64 blob the .carxs file ships.
    pub fn set_chunk(&self, plugin: &PluginRef, chunk: &str) {
        let Some(plugin_id) = resolve_index(plugin, "plugin") else {
            return;
        };
        self.send(OscCommand::SetChunk {
            plugin_id,
            chunk: chunk.to_string(),
        });
    }

    /// `/Carla/<plugin>/set_custom_data <string type> <string key>
    /// <string value>`. Used by Phase 3 preset triggers to load
    /// plugin-specific state (e.g. an IR file path for CabXr).
    pub fn set_custom_data(&self, plugin: &PluginRef, kind: &str, key: &str, value: &str) {
        let Some(plugin_id) = resolve_index(plugin, "plugin") else {
            return;
        };
        self.send(OscCommand::SetCustomData {
            plugin_id,
            kind: kind.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    /// Tell the sender task to drain and exit. After this call the
    /// channel is closed; further publishes silently drop.
    pub fn shutdown(&self) {
        let _ = self.tx.try_send(OscCommand::Shutdown);
    }

    fn send(&self, cmd: OscCommand) {
        if let Err(e) = self.tx.try_send(cmd) {
            tracing::warn!(
                target: "juballer::carla::osc",
                "command channel full or closed: {e}"
            );
        }
    }
}

/// Encode `cmd` into an OSC packet and send it over `socket`.
async fn dispatch(socket: &UdpSocket, target: SocketAddr, cmd: &OscCommand) -> Result<()> {
    let pkt = encode_command(cmd).ok_or_else(|| {
        crate::Error::Config("encode_command called with Shutdown variant".into())
    })?;
    let bytes =
        encoder::encode(&pkt).map_err(|e| crate::Error::Config(format!("OSC encode: {e}")))?;
    socket.send_to(&bytes, target).await?;
    Ok(())
}

/// Build the OSC packet for a non-shutdown command. `Shutdown` is a
/// task-internal sentinel and intentionally returns `None`.
fn encode_command(cmd: &OscCommand) -> Option<OscPacket> {
    let msg = match cmd {
        OscCommand::SetParameterValue {
            plugin_id,
            param_id,
            value,
        } => OscMessage {
            addr: format!("/Carla/{plugin_id}/set_parameter_value"),
            args: vec![OscType::Int(*param_id as i32), OscType::Float(*value)],
        },
        OscCommand::SetProgram { plugin_id, program } => OscMessage {
            addr: format!("/Carla/{plugin_id}/set_program"),
            args: vec![OscType::Int(*program as i32)],
        },
        OscCommand::SetCustomData {
            plugin_id,
            kind,
            key,
            value,
        } => OscMessage {
            addr: format!("/Carla/{plugin_id}/set_custom_data"),
            args: vec![
                OscType::String(kind.clone()),
                OscType::String(key.clone()),
                OscType::String(value.clone()),
            ],
        },
        OscCommand::SetChunk { plugin_id, chunk } => OscMessage {
            addr: format!("/Carla/{plugin_id}/set_chunk"),
            args: vec![OscType::String(chunk.clone())],
        },
        OscCommand::Shutdown => return None,
    };
    Some(OscPacket::Message(msg))
}

/// Convert a [`PluginRef`] / [`ParamRef`] to a numeric index. Names
/// log a warning and return `None` until Phase 2 adds the
/// `/Carla/register` listener that builds the name → index map.
fn resolve_index(reference: &PluginRef, kind: &'static str) -> Option<u32> {
    match reference {
        PluginRef::Index(i) => Some(*i),
        PluginRef::Name(name) => {
            tracing::warn!(
                target: "juballer::carla::osc",
                "{kind} name resolution not implemented in Phase 1; \
                 use a numeric index instead of {name:?}"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::OscPacket;
    use std::net::Ipv4Addr;

    #[test]
    fn encode_set_parameter_value_uses_carla_address_pattern_and_int_float_args() {
        let cmd = OscCommand::SetParameterValue {
            plugin_id: 3,
            param_id: 7,
            value: 0.42,
        };
        let pkt = encode_command(&cmd).unwrap();
        let OscPacket::Message(msg) = pkt else {
            panic!("expected message")
        };
        assert_eq!(msg.addr, "/Carla/3/set_parameter_value");
        assert!(matches!(msg.args[0], OscType::Int(7)));
        assert!(matches!(msg.args[1], OscType::Float(v) if (v - 0.42).abs() < 1e-6));
    }

    #[test]
    fn encode_set_program_uses_int_arg() {
        let cmd = OscCommand::SetProgram {
            plugin_id: 1,
            program: 12,
        };
        let pkt = encode_command(&cmd).unwrap();
        let OscPacket::Message(msg) = pkt else {
            panic!("expected message")
        };
        assert_eq!(msg.addr, "/Carla/1/set_program");
        assert!(matches!(msg.args[0], OscType::Int(12)));
    }

    #[test]
    fn encode_set_custom_data_uses_three_string_args() {
        let cmd = OscCommand::SetCustomData {
            plugin_id: 2,
            kind: "lv2".into(),
            key: "ir_file".into(),
            value: "/srv/ir/marshall.wav".into(),
        };
        let pkt = encode_command(&cmd).unwrap();
        let OscPacket::Message(msg) = pkt else {
            panic!("expected message")
        };
        assert_eq!(msg.addr, "/Carla/2/set_custom_data");
        for arg in &msg.args {
            assert!(matches!(arg, OscType::String(_)));
        }
    }

    #[test]
    fn shutdown_command_does_not_encode() {
        assert!(encode_command(&OscCommand::Shutdown).is_none());
    }

    #[test]
    fn resolve_index_returns_index_for_index_variant() {
        assert_eq!(
            resolve_index(&PluginRef::Index(11), "plugin"),
            Some(11),
            "numeric index should pass through unchanged"
        );
    }

    #[test]
    fn resolve_index_returns_none_for_name_variant_in_phase_1() {
        assert_eq!(
            resolve_index(&PluginRef::Name("Roomy".into()), "plugin"),
            None,
            "name resolution lands in Phase 2; until then, drop with warning"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_round_trip_delivers_decoded_set_parameter_value() {
        // Bind a receiver on a kernel-chosen port, point a CarlaClient
        // at it, push one set_parameter_value through the public API,
        // and assert the bytes that arrive decode to the expected OSC
        // message. End-to-end: producer → channel → tokio task →
        // socket → decoder.
        let std_recv = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        std_recv.set_nonblocking(true).unwrap();
        let target = std_recv.local_addr().unwrap();
        let receiver = UdpSocket::from_std(std_recv).unwrap();

        let rt = tokio::runtime::Handle::current();
        let client = CarlaClient::spawn(&rt, target).unwrap();
        client.set_parameter_value(&PluginRef::Index(5), &PluginRef::Index(2), 0.75);

        let mut buf = [0u8; 1024];
        let (n, _) = tokio::time::timeout(
            std::time::Duration::from_millis(1500),
            receiver.recv_from(&mut buf),
        )
        .await
        .expect("packet should arrive within 1.5s")
        .expect("recv_from should succeed");

        let (_rest, pkt) = rosc::decoder::decode_udp(&buf[..n]).expect("decode OSC packet");
        let OscPacket::Message(msg) = pkt else {
            panic!("expected OscPacket::Message");
        };
        assert_eq!(msg.addr, "/Carla/5/set_parameter_value");
        assert!(matches!(msg.args[0], OscType::Int(2)));
        assert!(matches!(msg.args[1], OscType::Float(v) if (v - 0.75).abs() < 1e-6));

        client.shutdown();
    }
}
