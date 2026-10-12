"""Synthesises the promo's soundtrack, synced to the cues in promo.html.

    python3 sound.py out.wav
"""
import sys
import wave

import numpy as np

SR = 48000
DUR = 15.0
N = int(SR * DUR)
rng = np.random.default_rng(3)
L = np.zeros(N)
R = np.zeros(N)


def add(sig, t, gain=1.0, pan=0.0):
    i = int(t * SR)
    if i >= N:
        return
    sig = sig[: N - i] * gain
    L[i:i + len(sig)] += sig * (1 - max(0, pan))
    R[i:i + len(sig)] += sig * (1 + min(0, pan))


def env(n, a=0.002, d=0.1):
    t = np.arange(n) / SR
    return np.minimum(1, t / a) * np.exp(-t / d)


def lowpass(x, k):
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc += k * (v - acc)
        y[i] = acc
    return y


def kick(gain=1.0):
    n = int(0.45 * SR)
    t = np.arange(n) / SR
    f = 45 + 110 * np.exp(-t * 28)
    ph = 2 * np.pi * np.cumsum(f) / SR
    return np.sin(ph) * env(n, 0.001, 0.16) * gain


def hat(n_s=0.05):
    n = int(n_s * SR)
    x = rng.standard_normal(n)
    x = x - lowpass(x, 0.25)
    return x * env(n, 0.0005, 0.012)


def click():
    n = int(0.03 * SR)
    x = rng.standard_normal(n)
    x = x - lowpass(x, 0.35)
    t = np.arange(n) / SR
    body = np.sin(2 * np.pi * 1800 * t) * 0.4
    return (x * 0.6 + body) * env(n, 0.0003, 0.006)


def tick(freq=2400):
    n = int(0.06 * SR)
    t = np.arange(n) / SR
    return np.sin(2 * np.pi * freq * t) * env(n, 0.0005, 0.018)


def whoosh(dur=0.45, rise=True):
    n = int(dur * SR)
    x = rng.standard_normal(n)
    t = np.linspace(0, 1, n)
    shape = np.sin(np.pi * t) ** 2 if rise else np.exp(-t * 4)
    # sweep a one-pole low-pass by running it in chunks
    y = np.empty(n)
    acc = 0.0
    for i in range(n):
        k = 0.02 + 0.25 * t[i] if rise else 0.3 - 0.25 * t[i]
        acc += k * (x[i] - acc)
        y[i] = acc
    return y * shape * 2.2


def thump(freq=70, dur=0.6):
    n = int(dur * SR)
    t = np.arange(n) / SR
    f = freq + 160 * np.exp(-t * 20)
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * env(n, 0.001, 0.22)


def bell(freqs, dur=2.2, gain=0.25):
    n = int(dur * SR)
    t = np.arange(n) / SR
    s = np.zeros(n)
    for f in freqs:
        for h, a in ((1, 1), (2.01, .35), (3.02, .12)):
            s += a * np.sin(2 * np.pi * f * h * t) * np.exp(-t * (1.2 + h))
    return s * np.minimum(1, t / 0.004) * gain / len(freqs)


def pad(freqs, dur, gain=0.05):
    n = int(dur * SR)
    t = np.arange(n) / SR
    s = np.zeros(n)
    for f in freqs:
        for det in (-0.6, 0.6):
            s += np.sin(2 * np.pi * (f + det) * t + rng.uniform(0, 6))
    fade = np.minimum(1, t / 0.6) * np.minimum(1, (dur - t) / 0.8)
    return s * fade * gain / len(freqs)


def riser(dur):
    n = int(dur * SR)
    t = np.arange(n) / SR
    f = 200 + 900 * (t / dur) ** 2
    s = np.sin(2 * np.pi * np.cumsum(f) / SR)
    return s * (t / dur) ** 2 * 0.12


# ---- music bed: 120 bpm ----
D, Fs, A, B, Cs, E = 146.83, 185.0, 220.0, 246.94, 277.18, 329.63
add(pad([D / 2, A / 2, Fs, Cs * 2], 6.0, 0.08), 1.3)
add(pad([B / 2, Fs / 2, D, A * 2], 3.0, 0.08), 7.0)
add(pad([D / 2, A / 2, E, Fs * 2], 5.0, 0.07), 10.0)
add(riser(0.72), 0.02, 1.0)

for b in np.arange(2.5, 9.2, 0.5):
    add(kick(), b, 0.55)
for b in np.arange(2.75, 9.2, 0.5):
    add(hat(), b, 0.18, 0.3)
for b in np.arange(11.5, 13.2, 0.5):
    add(kick(), b, 0.5)
for b in np.arange(11.25, 13.2, 0.25):
    add(hat(), b, 0.12, -0.3)

# bass line, one note per beat
bass_notes = [D, D, A / 2 * 2, A, B, B, Fs, A] * 2
for i, b in enumerate(np.arange(2.5, 9.0, 0.5)):
    n = int(0.42 * SR)
    t = np.arange(n) / SR
    f = bass_notes[i % len(bass_notes)] / 2
    add((np.sin(2 * np.pi * f * t) + 0.3 * np.sin(4 * np.pi * f * t)) * env(n, 0.005, 0.25), b, 0.22)

# ---- sfx ----
add(thump(60, 0.9), 0.74, 0.9)                     # Super + Space
add(hat(0.2), 0.74, 0.25)
add(whoosh(0.7), 1.25, 0.25)                         # icon → search bar
add(bell([D * 2, A * 2], 1.0, 0.18), 0.95)           # icon pops

scenes = [(2.55, 'calc', 0.085), (4.4, '!htop', 0.045), (5.0, 'search: rust gtk4', 0.02),
          (5.6, '>palette', 0.02), (7.4, '>style', 0.02)]
for t0, q, cps in scenes:
    for i in range(len(q)):
        add(click(), t0 + i * cps, 0.35, rng.uniform(-0.3, 0.3))
for i in range(1, 6):
    add(tick(2000 + i * 180), 5.75 + i * 0.27, 0.12, 0.4)
    add(tick(1600 + i * 200), 7.5 + i * 0.22, 0.12, -0.4)
add(whoosh(0.3, False), 4.4, 0.12)
add(whoosh(0.3, False), 5.0, 0.12)

add(click(), 8.85, 0.7)                               # Esc
add(whoosh(0.35, False), 8.85, 0.15)
add(click(), 9.15, 0.8)                               # Enter
add(thump(55, 0.8), 9.16, 0.7)
add(riser(0.3) * 3, 9.2, 0.6)                         # implode
add(thump(40, 1.2), 9.48, 1.0)
add(hat(0.4), 9.48, 0.35)

for t in (9.58, 10.0, 10.42):                         # panel wipes
    add(whoosh(0.4), t - 0.12, 0.3, 0.5 if t == 10.0 else -0.5)
    add(kick(), t + 0.12, 0.6)
for i in range(4):
    add(click(), 10.1 + i * 0.05, 0.5)                # "Type."
add(whoosh(0.45), 10.78, 0.3)
add(whoosh(0.6, False), 11.12, 0.25)                  # "Gone." scatters

add(bell([D * 2, Fs * 2, A * 2, Cs * 4], 2.2, 0.35), 13.25)   # end card
add(thump(50, 1.0), 13.25, 0.6)
add(bell([A * 4, E * 4], 1.0, 0.12), 14.12)           # repository pill

# ---- master ----
mix = np.stack([L, R], axis=1)
fade = np.ones(N)
fade[-int(0.25 * SR):] = np.linspace(1, 0, int(0.25 * SR))
mix *= fade[:, None]
mix = np.tanh(mix * 1.4) / np.tanh(1.4)
mix *= 0.89 / np.max(np.abs(mix))
pcm = (mix * 32767).astype('<i2')
with wave.open(sys.argv[1] if len(sys.argv) > 1 else 'sound.wav', 'wb') as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(pcm.tobytes())
