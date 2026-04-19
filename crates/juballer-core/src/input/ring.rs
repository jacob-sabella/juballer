use super::Event;
use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};

/// Drop-on-overflow bounded channel for sub-ms input ingestion.
pub struct EventRing {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    pub dropped: std::sync::atomic::AtomicU64,
}

impl EventRing {
    pub fn new(cap: usize) -> Self {
        let (tx, rx) = bounded(cap);
        Self {
            tx,
            rx,
            dropped: 0.into(),
        }
    }

    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    pub fn drain_into(&self, out: &mut Vec<Event>) {
        while let Ok(ev) = self.rx.try_recv() {
            out.push(ev);
        }
    }

    pub fn try_send(&self, ev: Event) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(ev) {
            self.dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn ev() -> Event {
        Event::Unmapped {
            key: super::super::KeyCode::new("X"),
            ts: Instant::now(),
        }
    }

    #[test]
    fn round_trip() {
        let r = EventRing::new(8);
        r.try_send(ev());
        r.try_send(ev());
        let mut out = Vec::new();
        r.drain_into(&mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn overflow_drops_with_metric() {
        let r = EventRing::new(2);
        for _ in 0..5 {
            r.try_send(ev());
        }
        assert!(r.dropped.load(std::sync::atomic::Ordering::Relaxed) >= 3);
    }
}
