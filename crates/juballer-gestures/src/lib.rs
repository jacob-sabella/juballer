//! Gesture recognizer over juballer-core raw events.

use juballer_core::input::Event;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Gesture {
    Tap { row: u8, col: u8, dur: Duration },
    Hold { row: u8, col: u8, dur: Duration },
    Chord { cells: Vec<(u8, u8)>, ts: Instant },
    Swipe { path: Vec<(u8, u8)>, dur: Duration },
}

#[derive(Debug, Clone)]
pub struct Thresholds {
    pub tap_max: Duration,
    pub hold_min: Duration,
    pub chord_window: Duration,
    pub swipe_window_per_step: Duration,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            tap_max: Duration::from_millis(250),
            hold_min: Duration::from_millis(400),
            chord_window: Duration::from_millis(50),
            swipe_window_per_step: Duration::from_millis(80),
        }
    }
}

pub struct Recognizer {
    th: Thresholds,
    pressed_at: HashMap<(u8, u8), Instant>,
    swipe_path: Vec<((u8, u8), Instant)>,
    chord_buf: Vec<((u8, u8), Instant)>,
}

impl Recognizer {
    pub fn with_defaults() -> Self {
        Self::new(Thresholds::default())
    }

    pub fn new(th: Thresholds) -> Self {
        Self {
            th,
            pressed_at: HashMap::new(),
            swipe_path: Vec::new(),
            chord_buf: Vec::new(),
        }
    }

    pub fn builder() -> RecognizerBuilder {
        RecognizerBuilder {
            th: Thresholds::default(),
        }
    }

    pub fn feed(&mut self, ev: &Event) -> Vec<Gesture> {
        let mut out = Vec::new();
        match ev {
            Event::KeyDown { row, col, ts, .. } => {
                self.pressed_at.insert((*row, *col), *ts);
                self.chord_buf.push(((*row, *col), *ts));
                self.swipe_path.push(((*row, *col), *ts));
                self.try_emit_chord(*ts, &mut out);
            }
            Event::KeyUp { row, col, ts, .. } => {
                if let Some(t0) = self.pressed_at.remove(&(*row, *col)) {
                    let dur = ts.duration_since(t0);
                    if dur <= self.th.tap_max {
                        out.push(Gesture::Tap {
                            row: *row,
                            col: *col,
                            dur,
                        });
                    } else if dur >= self.th.hold_min {
                        out.push(Gesture::Hold {
                            row: *row,
                            col: *col,
                            dur,
                        });
                    }
                }
                self.try_emit_swipe(*ts, &mut out);
            }
            _ => {}
        }
        out
    }

    fn try_emit_chord(&mut self, now: Instant, out: &mut Vec<Gesture>) {
        let cutoff = now - self.th.chord_window;
        self.chord_buf.retain(|(_, t)| *t >= cutoff);
        if self.chord_buf.len() >= 2 {
            let cells: HashSet<(u8, u8)> = self.chord_buf.iter().map(|(c, _)| *c).collect();
            if cells.len() == self.chord_buf.len() {
                let mut v: Vec<_> = cells.into_iter().collect();
                v.sort();
                out.push(Gesture::Chord { cells: v, ts: now });
                self.chord_buf.clear();
            }
        }
    }

    fn try_emit_swipe(&mut self, now: Instant, out: &mut Vec<Gesture>) {
        if self.swipe_path.len() < 2 {
            self.swipe_path.clear();
            return;
        }
        for w in self.swipe_path.windows(2) {
            if w[1].1.duration_since(w[0].1) > self.th.swipe_window_per_step {
                self.swipe_path.clear();
                return;
            }
        }
        let path: Vec<(u8, u8)> = self.swipe_path.iter().map(|(c, _)| *c).collect();
        let dur = now.duration_since(self.swipe_path[0].1);
        out.push(Gesture::Swipe { path, dur });
        self.swipe_path.clear();
    }
}

pub struct RecognizerBuilder {
    th: Thresholds,
}

impl RecognizerBuilder {
    pub fn tap_max(mut self, d: Duration) -> Self {
        self.th.tap_max = d;
        self
    }
    pub fn hold_min(mut self, d: Duration) -> Self {
        self.th.hold_min = d;
        self
    }
    pub fn chord_window(mut self, d: Duration) -> Self {
        self.th.chord_window = d;
        self
    }
    pub fn swipe_window_per_step(mut self, d: Duration) -> Self {
        self.th.swipe_window_per_step = d;
        self
    }
    pub fn build(self) -> Recognizer {
        Recognizer::new(self.th)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use juballer_core::input::KeyCode;

    fn down(row: u8, col: u8, t: Instant) -> Event {
        Event::KeyDown {
            row,
            col,
            key: KeyCode::new("X"),
            ts: t,
        }
    }
    fn up(row: u8, col: u8, t: Instant) -> Event {
        Event::KeyUp {
            row,
            col,
            key: KeyCode::new("X"),
            ts: t,
        }
    }

    #[test]
    fn short_press_is_tap() {
        let mut r = Recognizer::with_defaults();
        let t0 = Instant::now();
        let _ = r.feed(&down(1, 1, t0));
        let g = r.feed(&up(1, 1, t0 + Duration::from_millis(100)));
        assert!(matches!(g[0], Gesture::Tap { .. }));
    }

    #[test]
    fn long_press_is_hold() {
        let mut r = Recognizer::with_defaults();
        let t0 = Instant::now();
        let _ = r.feed(&down(0, 0, t0));
        let g = r.feed(&up(0, 0, t0 + Duration::from_millis(800)));
        assert!(matches!(g[0], Gesture::Hold { .. }));
    }
}
