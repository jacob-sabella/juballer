"""
Juballer rhythm tick: soft click + FM bell on E5.

Design:
- 80ms total, mono 44.1 kHz
- Transient: HP-filtered noise burst, 4ms exp decay — soft "tock" attack.
  Deliberately reduced from an earlier sharp version so chord taps don't
  startle. Small amplitude keeps it subordinate to the bell body.
- Bell: FM synthesis, carrier=659.26 Hz (E5, one octave down from an
  earlier too-piercing E6 take), mod ratio 1.5, mod depth decays faster
  than amplitude → warm attack settling to sine tail. 2ms linear attack
  ramp prevents startle snap. Pitched so chord taps (4 at once) stack on
  one harmonic fundamental instead of clashing.
- Sub-body: 110 Hz sine for weight, 14ms exp decay.
- Soft tanh saturation + 1ms/3ms fades. -6 dBFS peak gives voice-cap
  stacking and master-volume=1.0 generous headroom.
"""
import numpy as np
import soundfile as sf

SR = 44_100
DUR = 0.080
N = int(SR * DUR)
t = np.arange(N) / SR

# 1. Transient click — HP-ish via first-diff of white noise, softened.
rng = np.random.default_rng(0xBEEFD00D)
noise = rng.standard_normal(N)
hp = np.diff(noise, prepend=0.0)
click_env = np.exp(-t / 0.004)
click = hp * click_env * 0.22

# 2. FM bell at E5. 2ms attack ramp avoids sharp step on bell onset.
fc = 659.26  # E5
fm = 1.5 * fc
mod_env = np.exp(-t / 0.025)
amp_env = np.exp(-t / 0.050)
mod = np.sin(2 * np.pi * fm * t)
atk = np.minimum(t / 0.002, 1.0)
bell = np.sin(2 * np.pi * fc * t + 1.8 * mod_env * mod) * amp_env * atk * 0.55

# 3. Sub body — short low sine for weight so the tick doesn't feel tinny.
body_env = np.exp(-t / 0.014)
body = np.sin(2 * np.pi * 110.0 * t) * body_env * 0.18

y = click + bell + body

# Soft saturation — keeps peaks clip-free at any master_volume.
y = np.tanh(y * 1.15) * 0.85

# Anti-click fades.
fi = int(0.001 * SR)
fo = int(0.003 * SR)
y[:fi] *= np.linspace(0.0, 1.0, fi)
y[-fo:] *= np.linspace(1.0, 0.0, fo)

# Normalize to -6 dBFS peak — generous headroom under voice stacking.
peak = np.max(np.abs(y))
y = y * (10 ** (-6 / 20) / peak)

sf.write("/tmp/tick.wav", y.astype(np.float32), SR, subtype="PCM_16")
print(f"wrote /tmp/tick.wav  n={N}  dur={DUR*1000:.0f}ms  peak={20*np.log10(np.max(np.abs(y))):.1f} dBFS")
