//! Live audio spectrum tap for background shaders.
//!
//! Wiring:
//!
//! 1. [`SharedSpectrum`] owns the sample ring and the FFT planner. Cheap
//!    to clone — everything is behind `Arc`.
//! 2. [`SampleTap`] wraps a `rodio::Source` and mirrors each sample it
//!    yields into the ring. The mixer thread reads through the tap, so
//!    every sample the user hears is also pushed. Rodio decodes to
//!    `i16`; we convert to `f32` and downmix to mono (channel 0) for
//!    FFT purposes.
//! 3. [`SharedSpectrum::snapshot`] takes the most-recent `FFT_SIZE`
//!    samples, applies a Hann window, runs a real-FFT, and reduces the
//!    half-spectrum to [`NUM_BINS`] log-spaced magnitudes for shader
//!    consumption.
//!
//! Thread model: `try_lock` on the producer side means the tap drops
//! samples rather than blocking the mixer if the main thread is in the
//! middle of `snapshot`. That's fine — dropping a few samples out of
//! ~48k/s is inaudible and irrelevant for visual FFT.
//!
//! Bin smoothing (attack-fast, release-slow exponential) is applied on
//! the consumer side so spikes read as spikes but don't immediately
//! snap back to zero. The shader gets a clean 0..1 envelope per bin.
//!
//! Shaders read bins via `u.spectrum[i/4][i%4]`.

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};
use rodio::Source;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Ring buffer capacity — ~85ms at 48 kHz. Chosen so a single snapshot
/// (FFT_SIZE samples) always has room plus one frame's worth of
/// producer lag without stalling.
const RING_CAP: usize = 4096;

/// FFT window size. Must be ≤ [`RING_CAP`] and a multiple of 2 for
/// realfft. 1024 at 48 kHz = 21 ms of audio per snapshot, giving ~47 Hz
/// bin width before log-bucketing.
pub const FFT_SIZE: usize = 1024;

/// Number of log-spaced output bands delivered to shaders. Sized to
/// fit exactly in 4×`vec4<f32>` of uniform storage.
pub const NUM_BINS: usize = 16;

/// Band edge fraction of Nyquist: 16 log-spaced bins from ~60 Hz to
/// Nyquist (~22 kHz at 44.1 kHz). Precomputed so the consumer side
/// doesn't `log()` every snapshot.
fn band_edges_hz(sample_rate: f32) -> [f32; NUM_BINS + 1] {
    let lo = 60.0f32;
    let hi = (sample_rate * 0.5).max(lo * 2.0);
    let mut edges = [0.0f32; NUM_BINS + 1];
    let ln_lo = lo.ln();
    let ln_hi = hi.ln();
    for (i, edge) in edges.iter_mut().enumerate() {
        let t = i as f32 / NUM_BINS as f32;
        *edge = (ln_lo + t * (ln_hi - ln_lo)).exp();
    }
    edges
}

/// Shared between the rodio mixer thread (producer) and the main frame
/// loop (consumer). Cheap to clone.
#[derive(Clone)]
pub struct SharedSpectrum {
    inner: Arc<Inner>,
}

struct Inner {
    /// Most-recent samples (mono-downmix, f32 in [-1, 1]). Bounded to
    /// [`RING_CAP`]. Producer uses `try_lock` and drops on contention.
    ring: Mutex<VecDeque<f32>>,
    /// Source sample rate. Written by the tap once it's known; read
    /// each snapshot to compute band edges.
    sample_rate: AtomicU32,
    /// FFT state. Mutex so the consumer can call `process` exclusively;
    /// producer never touches this.
    fft: Mutex<FftState>,
    /// Smoothed output bins. Kept across snapshots so the shader sees
    /// continuous envelopes instead of per-frame noise.
    smoothed: Mutex<[f32; NUM_BINS]>,
    /// Diagnostic: set on first sample batch so the log line fires once
    /// (confirms the tap is wired up end-to-end through rodio).
    first_batch_logged: std::sync::atomic::AtomicBool,
    /// Diagnostic counter for snapshot calls + last-printed timestamp;
    /// prints peak bin every ~2 s so we can confirm reactive data is
    /// flowing.
    snapshot_count: std::sync::atomic::AtomicU64,
}

struct FftState {
    plan: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
    hann: Vec<f32>,
}

impl FftState {
    fn new(size: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let plan = planner.plan_fft_forward(size);
        let scratch_len = plan.get_scratch_len();
        Self {
            input: vec![0.0; size],
            output: vec![Complex32::new(0.0, 0.0); size / 2 + 1],
            scratch: vec![Complex32::new(0.0, 0.0); scratch_len],
            hann: (0..size)
                .map(|i| {
                    let t = i as f32 / (size - 1) as f32;
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * t).cos()
                })
                .collect(),
            plan,
        }
    }
}

impl Default for SharedSpectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSpectrum {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
                sample_rate: AtomicU32::new(44100),
                fft: Mutex::new(FftState::new(FFT_SIZE)),
                smoothed: Mutex::new([0.0; NUM_BINS]),
                first_batch_logged: std::sync::atomic::AtomicBool::new(false),
                snapshot_count: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// Non-blocking push from the mixer thread. Drops on contention
    /// rather than stalling audio output.
    fn push_batch(&self, batch: &[f32]) {
        let Ok(mut ring) = self.inner.ring.try_lock() else {
            return;
        };
        for &s in batch {
            if ring.len() == RING_CAP {
                ring.pop_front();
            }
            ring.push_back(s);
        }
        // First-batch breadcrumb so we can see in the log whether the
        // sample tap is actually being driven by the mixer thread.
        // Cheap one-shot: AcqRel swap on a dedicated bool.
        if !self
            .inner
            .first_batch_logged
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            tracing::info!(
                target: "juballer::rhythm::spectrum",
                "audio sample tap active: first batch of {} samples received (peak {:.3})",
                batch.len(),
                batch.iter().fold(0.0f32, |a, &b| a.max(b.abs()))
            );
        }
    }

    fn set_sample_rate(&self, hz: u32) {
        self.inner.sample_rate.store(hz, Ordering::Relaxed);
    }

    /// Run an FFT on the newest window and return smoothed, log-spaced
    /// bins in [0, 1]. Call once per frame from the render loop.
    pub fn snapshot(&self) -> [f32; NUM_BINS] {
        let rate = self.inner.sample_rate.load(Ordering::Relaxed) as f32;
        let edges = band_edges_hz(rate.max(8_000.0));

        // Copy the newest FFT_SIZE samples out of the ring under lock.
        // If the ring hasn't filled yet, left-pad with zeros.
        let window = {
            let ring = match self.inner.ring.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if ring.len() < FFT_SIZE {
                let mut w = vec![0.0f32; FFT_SIZE];
                let start = FFT_SIZE - ring.len();
                for (i, &s) in ring.iter().enumerate() {
                    w[start + i] = s;
                }
                w
            } else {
                let offset = ring.len() - FFT_SIZE;
                ring.iter().skip(offset).copied().collect::<Vec<_>>()
            }
        };

        let raw = {
            let mut fft = match self.inner.fft.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            // Destructure to borrow fields disjointly — otherwise the
            // fft.plan.process_with_scratch call conflicts with the
            // mutable borrow of fft.input / fft.output.
            let FftState {
                input,
                output,
                scratch,
                hann,
                plan,
            } = &mut *fft;
            for (dst, (src, w)) in input.iter_mut().zip(window.iter().zip(hann.iter())) {
                *dst = *src * *w;
            }
            let _ = plan.process_with_scratch(input, output, scratch);
            // Magnitudes per FFT bin, scaled to roughly 0..1.
            let norm = 2.0 / (FFT_SIZE as f32);
            output
                .iter()
                .map(|c| (c.re * c.re + c.im * c.im).sqrt() * norm)
                .collect::<Vec<_>>()
        };

        // Collapse linear bins into log-spaced bands. Freq per FFT bin
        // = rate / FFT_SIZE.
        let bin_hz = rate / FFT_SIZE as f32;
        let mut bands = [0.0f32; NUM_BINS];
        for (b, band) in bands.iter_mut().enumerate() {
            let lo = edges[b] / bin_hz;
            let hi = edges[b + 1] / bin_hz;
            let lo_i = lo.floor() as usize;
            let hi_i = (hi.ceil() as usize).min(raw.len().saturating_sub(1));
            if hi_i <= lo_i {
                continue;
            }
            // Average across FFT bins in this log-band rather than max:
            // max lets one loud frequency saturate the whole band; mean
            // gives a steadier amplitude that tracks energy density.
            let mut acc: f32 = 0.0;
            let mut n: f32 = 0.0;
            for r in raw.iter().take(hi_i + 1).skip(lo_i) {
                acc += *r;
                n += 1.0;
            }
            let mean = if n > 0.0 { acc / n } else { 0.0 };
            // Perceptual curve. sqrt(mean) compresses the dynamic range;
            // 0.9× keeps peaks around 0.5-0.7 — gentle pulse rather
            // than a saturating throb.
            *band = (mean.sqrt() * 0.9).clamp(0.0, 1.0);
        }

        // Attack/release smoothing. Both attack and release lean on the
        // previous value so peaks ramp in (no instant strobe) and decay
        // gracefully — without this the shader visualisations strobe.
        let mut out = match self.inner.smoothed.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for (prev, &new) in out.iter_mut().zip(bands.iter()) {
            if new > *prev {
                // Attack: 50/50 blend so spikes don't snap the visual.
                *prev = *prev * 0.5 + new * 0.5;
            } else {
                // Release: longer tail.
                *prev = *prev * 0.88 + new * 0.12;
            }
        }
        // Diagnostic: dump bins every ~120 snapshots (~2 s at 60 fps)
        // so we can see whether the values move with the music.
        let n = self
            .inner
            .snapshot_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n % 120 == 0 {
            let peak = out.iter().fold(0.0f32, |a, &b| a.max(b));
            let avg: f32 = out.iter().sum::<f32>() / NUM_BINS as f32;
            tracing::info!(
                target: "juballer::rhythm::spectrum",
                "bins peak={peak:.3} avg={avg:.3} → {:?}",
                out.map(|v| (v * 100.0).round() / 100.0)
            );
        }
        *out
    }
}

/// rodio Source wrapper that mirrors i16 samples into [`SharedSpectrum`]
/// as they pass through to the mixer.
pub struct SampleTap<S> {
    inner: S,
    shared: SharedSpectrum,
    channels: u16,
    /// Which channel we're about to see next (0..channels). Used to
    /// pick out channel 0 for mono downmix without buffering the rest.
    ch_ix: u16,
    /// Small local buffer so the producer does one try_lock per N
    /// samples instead of per sample. 128 samples ≈ 2.7ms at 48k.
    scratch: Vec<f32>,
}

const SCRATCH_FLUSH: usize = 128;

impl<S: Source<Item = i16>> SampleTap<S> {
    pub fn new(inner: S, shared: SharedSpectrum) -> Self {
        let channels = inner.channels().max(1);
        let rate = inner.sample_rate();
        shared.set_sample_rate(rate);
        Self {
            inner,
            shared,
            channels,
            ch_ix: 0,
            scratch: Vec::with_capacity(SCRATCH_FLUSH),
        }
    }
}

impl<S: Source<Item = i16>> Iterator for SampleTap<S> {
    type Item = i16;

    fn next(&mut self) -> Option<i16> {
        let s = self.inner.next()?;
        // Grab channel 0 for the FFT; skip the rest.
        if self.ch_ix == 0 {
            self.scratch.push((s as f32) / 32_768.0);
            if self.scratch.len() >= SCRATCH_FLUSH {
                self.shared.push_batch(&self.scratch);
                self.scratch.clear();
            }
        }
        self.ch_ix = (self.ch_ix + 1) % self.channels;
        Some(s)
    }
}

impl<S: Source<Item = i16>> Source for SampleTap<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_edges_are_monotonic_and_log_spaced() {
        let e = band_edges_hz(48_000.0);
        for i in 1..e.len() {
            assert!(e[i] > e[i - 1], "non-monotonic at {i}: {:?}", e);
        }
        // Ratio between successive edges should be ~constant (log spacing).
        let r0 = e[1] / e[0];
        let r_last = e[NUM_BINS] / e[NUM_BINS - 1];
        assert!(
            (r0 - r_last).abs() < 1e-3,
            "not log-spaced: {r0} vs {r_last}"
        );
    }

    #[test]
    fn snapshot_on_empty_ring_returns_zeros() {
        let s = SharedSpectrum::new();
        let out = s.snapshot();
        for v in out {
            assert!(v < 1e-3);
        }
    }

    #[test]
    fn snapshot_responds_to_pure_tone() {
        // Feed a 1 kHz sine into the ring and confirm the bin covering
        // 1 kHz ends up hotter than the DC-adjacent bin.
        let s = SharedSpectrum::new();
        s.set_sample_rate(48_000);
        let f = 1_000.0f32;
        let sr = 48_000.0f32;
        let samples: Vec<f32> = (0..RING_CAP)
            .map(|i| (2.0 * std::f32::consts::PI * f * (i as f32) / sr).sin() * 0.7)
            .collect();
        s.push_batch(&samples);
        let bins = s.snapshot();
        // Find the bin that should own 1 kHz.
        let edges = band_edges_hz(48_000.0);
        let mut target = 0usize;
        for i in 0..NUM_BINS {
            if f >= edges[i] && f < edges[i + 1] {
                target = i;
                break;
            }
        }
        let hot = bins[target];
        let cold = bins[0]; // low-freq edge — pure tone at 1 kHz should be quiet here.
        assert!(
            hot > cold * 2.0,
            "target bin {target} not dominant: {bins:?}"
        );
    }
}
