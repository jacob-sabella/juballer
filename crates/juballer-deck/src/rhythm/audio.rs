//! Audio playback wrapper over rodio 0.19.
//!
//! Ownership model: [`Audio`] owns the `OutputStream` (must live as long as the
//! sink) and an `Arc<Sink>` so the game loop can query the playing offset without
//! cloning the stream. `position_ms` is the master clock for the rhythm game;
//! every note-scheduling decision and every input-judgment calculation is
//! expressed relative to it.
//!
//! Delayed-start model: [`Audio::load_delayed`] creates a paused sink and
//! schedules playback to begin `start_delay_ms` into the future. During the
//! countdown interval, [`Audio::position_ms`] returns a **negative** value
//! (`-start_delay_ms` at creation, ramping up to 0 at kickoff). Callers use
//! this to gate scheduling ("don't tick until music_ms >= 0") and to render
//! a pre-song countdown HUD. On the first positive crossing, [`position_ms`]
//! unpauses the sink so actual decoder output aligns with the master clock.

use crate::rhythm::spectrum::{SampleTap, SharedSpectrum};
use crate::{Error, Result};
use rodio::{Decoder, OutputStream, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Audio {
    // OutputStream must live as long as the sink but isn't otherwise accessed.
    _stream: OutputStream,
    sink: Arc<Sink>,
    /// Monotonic reference instant from which `position_ms` is derived. Shifted
    /// **forward** by `start_delay_ms` for a delayed start, so
    /// `Instant::now() < start_instant` yields a negative music-time.
    start_instant: Instant,
    user_offset_ms: i32,
    duration_ms: Option<f64>,
    /// True until the first time `position_ms` observes `now >= start_instant`
    /// and unpauses the sink. Tracks the "cold-start" gate so we don't un-pause
    /// every frame (rodio is cheap, but this keeps the intent explicit).
    started: AtomicBool,
}

impl Audio {
    /// Decode `path`, kick off playback, return a live handle. `user_offset_ms` shifts
    /// the perceived music-time: positive value means "audio lags input" → subtract
    /// from music_time so input feels in sync.
    pub fn load_and_play(path: &Path, user_offset_ms: i32) -> Result<Self> {
        Self::load_delayed(path, user_offset_ms, 0, None)
    }

    /// Like [`load_and_play`] but delays actual audio output by `start_delay_ms`.
    /// Immediately after this returns, [`position_ms`] reports roughly
    /// `-start_delay_ms` and counts up; the sink is paused until the first
    /// positive-side observation of `position_ms`.
    ///
    /// `start_delay_ms = 0` is equivalent to [`load_and_play`] — the sink
    /// starts immediately.
    ///
    /// If `spectrum` is supplied, samples passing through the sink are
    /// mirrored into it for FFT-driven shader backgrounds. The tap is
    /// non-blocking: on contention samples are dropped (inaudible since
    /// they still play through to the speaker; only the visualiser
    /// misses them).
    pub fn load_delayed(
        path: &Path,
        user_offset_ms: i32,
        start_delay_ms: u32,
        spectrum: Option<SharedSpectrum>,
    ) -> Result<Self> {
        let (stream, handle) = OutputStream::try_default()
            .map_err(|e| Error::Config(format!("audio: no output device: {e}")))?;
        let file = File::open(path)
            .map_err(|e| Error::Config(format!("audio: cannot open {}: {e}", path.display())))?;
        let decoder = Decoder::new(BufReader::new(file))
            .map_err(|e| Error::Config(format!("audio: decode {}: {e}", path.display())))?;

        let duration_ms = decoder.total_duration().map(|d| d.as_secs_f64() * 1000.0);

        let sink = Sink::try_new(&handle)
            .map_err(|e| Error::Config(format!("audio: sink create: {e}")))?;
        match spectrum {
            Some(shared) => sink.append(SampleTap::new(decoder, shared)),
            None => sink.append(decoder),
        }
        let now = Instant::now();
        let (start_instant, started) = if start_delay_ms == 0 {
            sink.play();
            (now, true)
        } else {
            sink.pause();
            (now + Duration::from_millis(start_delay_ms as u64), false)
        };
        Ok(Self {
            _stream: stream,
            sink: Arc::new(sink),
            start_instant,
            user_offset_ms,
            duration_ms,
            started: AtomicBool::new(started),
        })
    }

    /// Current song position in ms. Before the scheduled start this returns
    /// a **negative** value (e.g. `-3000.0` at the top of a 3-second countdown).
    /// Once `now >= start_instant`, unpauses the sink if we haven't already
    /// and falls through to the rodio-backed position.
    ///
    /// Uses `Sink::get_pos` (rodio 0.19+) with `user_offset_ms` applied.
    /// If the sink hasn't started producing yet (cold buffers or we're still
    /// in the delay window), falls back to wall-clock math relative to
    /// `start_instant`.
    pub fn position_ms(&self) -> f64 {
        let now = Instant::now();
        let wall_ms = wall_position_ms(self.start_instant, now);

        if wall_ms < 0.0 {
            // Pre-start countdown phase. Sink stays paused; report the wall
            // offset straight.
            return wall_ms - (self.user_offset_ms as f64);
        }

        // At/past the scheduled start. Un-pause once (cheap no-op if already
        // playing) and then fall through to the sink's own position if it has
        // started producing.
        if !self.started.swap(true, Ordering::AcqRel) {
            self.sink.play();
        }

        let sink_pos = self.sink.get_pos().as_secs_f64() * 1000.0;
        let pos = if sink_pos <= 0.0 { wall_ms } else { sink_pos };
        pos - (self.user_offset_ms as f64)
    }

    /// Song duration if the decoder could determine it. Vorbis streams typically
    /// can; bare-streamed formats may return None.
    pub fn duration_ms(&self) -> Option<f64> {
        self.duration_ms
    }

    /// True when the sink has drained all appended sources. Caller should also
    /// consult `position_ms` / `duration_ms` because empty sinks can be momentary.
    ///
    /// **Always false during the pre-start countdown**, even though the paused
    /// sink would otherwise report empty during ramp-up on some backends —
    /// otherwise the game loop would fire the "finished" banner before the
    /// song ever plays.
    pub fn is_finished(&self) -> bool {
        if !self.started.load(Ordering::Acquire) {
            return false;
        }
        self.sink.empty()
    }

    /// Immediately stop playback; subsequent `is_finished()` calls return true.
    pub fn stop(&self) {
        // Flip the gate so `is_finished()` starts reporting honestly even if
        // we're stopped during the pre-start phase (e.g. user ESCs out mid-countdown).
        self.started.store(true, Ordering::Release);
        self.sink.stop();
    }
}

/// Pure arithmetic helper: milliseconds between `start_instant` and `now`,
/// expressed as a signed f64. Negative when `now < start_instant`. Extracted
/// so unit tests can exercise the clock math without needing a real rodio
/// OutputStream (rodio fails on headless CI).
pub(crate) fn wall_position_ms(start_instant: Instant, now: Instant) -> f64 {
    if now >= start_instant {
        now.duration_since(start_instant).as_secs_f64() * 1000.0
    } else {
        -(start_instant.duration_since(now).as_secs_f64() * 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_position_is_negative_before_start() {
        // A start_instant 3s in the future from `now` should produce ~-3000ms.
        let now = Instant::now();
        let start_instant = now + Duration::from_millis(3000);
        let pos = wall_position_ms(start_instant, now);
        assert!(
            (pos - (-3000.0)).abs() < 5.0,
            "expected ~-3000ms pre-start, got {pos}"
        );
    }

    #[test]
    fn wall_position_is_zero_at_start() {
        // Passing the same instant for start and `now` yields exactly 0.
        let now = Instant::now();
        let pos = wall_position_ms(now, now);
        assert!(pos.abs() < 1e-6, "expected 0ms at start, got {pos}");
    }

    #[test]
    fn wall_position_is_positive_after_start() {
        // start 500ms before `now` → ~+500ms.
        let start_instant = Instant::now();
        let now = start_instant + Duration::from_millis(500);
        let pos = wall_position_ms(start_instant, now);
        assert!(
            (pos - 500.0).abs() < 1.0,
            "expected ~500ms after start, got {pos}"
        );
    }

    #[test]
    fn wall_position_monotonic_across_boundary() {
        // A series of `now` values spanning the start_instant should produce
        // monotonically non-decreasing position readings, with the crossover
        // at zero.
        let start_instant = Instant::now() + Duration::from_millis(1_000);
        let samples = [0u64, 250, 500, 750, 1_000, 1_250, 1_500];
        let mut prev = f64::NEG_INFINITY;
        let mut saw_negative = false;
        let mut saw_nonnegative = false;
        for off in samples {
            let now = (start_instant - Duration::from_millis(1_000)) + Duration::from_millis(off);
            let pos = wall_position_ms(start_instant, now);
            assert!(pos >= prev, "non-monotonic: {prev} → {pos}");
            prev = pos;
            if pos < 0.0 {
                saw_negative = true;
            } else {
                saw_nonnegative = true;
            }
        }
        assert!(saw_negative, "expected some negative samples");
        assert!(saw_nonnegative, "expected some non-negative samples");
    }

    #[test]
    fn wall_position_matches_delay_at_creation() {
        // Mirrors how `load_delayed` sets start_instant: `now + delay`. The
        // initial `position_ms` measurement with `now == creation_instant`
        // must be ~-delay_ms.
        for delay_ms in [1_000u64, 2_500, 3_000, 5_000] {
            let creation = Instant::now();
            let start_instant = creation + Duration::from_millis(delay_ms);
            let pos = wall_position_ms(start_instant, creation);
            let expected = -(delay_ms as f64);
            assert!(
                (pos - expected).abs() < 2.0,
                "delay={delay_ms}ms: expected ~{expected}, got {pos}"
            );
        }
    }
}
