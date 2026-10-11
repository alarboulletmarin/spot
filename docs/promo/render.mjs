// Renders promo.html frame by frame and pipes the frames to ffmpeg.
//   node render.mjs [out.mp4] [fps]          full video
//   node render.mjs --stills dir t1 t2 ...   PNG stills at given times
import { createRequire } from 'node:module';
import { execSync } from 'node:child_process';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// playwright is resolved from the local project, else from the global install.
const require = createRequire(import.meta.url);
let chromium;
try { ({ chromium } = require('playwright')); } catch { ({ chromium } = require(path.join(execSync('npm root -g').toString().trim(), 'playwright'))); }

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
await page.goto('file://' + path.join(here, 'promo.html'));
await page.evaluate(() => document.fonts.ready);

if (args[0] === '--stills') {
  const dir = args[1];
  for (const t of args.slice(2)) {
    await page.evaluate(t => render(t), Number(t));
    await page.screenshot({ path: path.join(dir, `t${t}.png`) });
  }
} else {
  const out = args[0] || path.join(here, 'spot-promo.mp4');
  const fps = Number(args[1] || 60);
  const frames = Math.round(15 * fps);
  const ff = spawn('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(fps), '-i', '-',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '18', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', out], { stdio: ['pipe', 'inherit', 'inherit'] });
  for (let f = 0; f < frames; f++) {
    await page.evaluate(t => render(t), f / fps);
    const buf = await page.screenshot({ type: 'png' });
    if (!ff.stdin.write(buf)) await new Promise(r => ff.stdin.once('drain', r));
    if (f % 60 === 0) process.stderr.write(`frame ${f}/${frames}\n`);
  }
  ff.stdin.end();
  await new Promise(r => ff.on('close', r));
}
await browser.close();
