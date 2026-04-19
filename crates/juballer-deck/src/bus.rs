//! Intra-process event bus. Actions + widgets + plugins publish/subscribe by topic.
//!
//! Wraps a tokio broadcast channel. Messages are typed as (topic, JSON value) pairs.
//! Bounded capacity (default 1024) with lagging senders overwriting old messages —
//! late subscribers may miss pre-subscription events.

use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct Event {
    pub topic: String,
    pub data: serde_json::Value,
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, topic: impl Into<String>, data: serde_json::Value) {
        let _ = self.tx.send(Event {
            topic: topic.into(),
            data,
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_and_receive() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        bus.publish("test.topic", serde_json::json!({ "n": 1 }));
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.topic, "test.topic");
        assert_eq!(ev.data["n"], 1);
    }

    #[tokio::test]
    async fn no_subscribers_no_error() {
        let bus = EventBus::default();
        bus.publish("nobody.listening", serde_json::json!({}));
    }
}
