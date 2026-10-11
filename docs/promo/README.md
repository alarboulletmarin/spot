# Promo video

`spot-promo.mp4`: 15 seconds, 1920×1080, 60 fps, with sound.

Everything is generated from code, no assets:

- `promo.html` draws every frame as a pure function of time, `render(t)`. Open it in a browser to watch it play live.
- `sound.py` synthesises the soundtrack on the same cues (needs `numpy`).
- `render.mjs` steps through the frames in headless Chromium (Playwright) and pipes them to `ffmpeg`.

```bash
python3 sound.py sound.wav
node render.mjs silent.mp4 60
ffmpeg -i silent.mp4 -i sound.wav -c:v copy -c:a aac -shortest spot-promo.mp4
```

| Time | Beat |
|---|---|
| 0–2.5 s | `Super` + `Space`; the icon pops and unfolds into the search bar |
| 2.5–4.4 s | `calc`: apps and files, ~10 ms to open |
| 4.4–5.6 s | `!htop` in a terminal, `search:` in the browser |
| 5.6–7.4 s | `>palette`: six palettes previewed as the selection moves |
| 7.4–8.8 s | `>style`: default, glass, compact, flat, rounded, mono |
| 8.8–9.6 s | `Esc` puts things back, `Enter` launches, the window is gone |
| 9.6–11.4 s | Press. Type. Enter. Gone. |
| 11.4–13.2 s | No account. No tracking. No server. |
| 13.2–15 s | End card, tagline flips from English to French |
