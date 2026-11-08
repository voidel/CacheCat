// Explicit opt-in, read-only sample of a real cache. Never publishes or commits samples.
// Run from repository root: node tools/browser-real-cache-smoke.mjs R:/FirestormCache
import { createRequire } from 'node:module';
import { readFile, writeFile, mkdir, copyFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
const require = createRequire(new URL('../web/package.json', import.meta.url));
const { chromium } = require('@playwright/test');
const root = process.argv[2];
if (!root) throw new Error('Pass the viewer cache path explicitly.');
const cache = join(root, 'texturecache');
const index = await readFile(join(cache, 'texture.entries'));
const header = index.readFloatLE(0) === Math.fround(1.71) ? 44 : 8;
const count = index.readUInt32LE(header - 4);
const slots = [];
for (let i = 0; i < count; i++) {
  const record = index.subarray(header + 28 * i, header + 28 * (i + 1));
  if (record.readInt32LE(16) > 0) slots.push({ i, record, time: record.readUInt32LE(24) });
}
slots.sort((a, b) => b.time - a.time);
const selected = slots.slice(0, 16);
const sample = resolve('.local/real-cache-sample/texturecache');
await mkdir(sample, { recursive: true });
const out = Buffer.from(index.subarray(0, header)); out.writeUInt32LE(selected.length, header - 4);
const headers = await readFile(join(cache, 'texture.cache'));
await writeFile(join(sample, 'texture.entries'), Buffer.concat([out, ...selected.map(s => s.record)]));
await writeFile(join(sample, 'texture.cache'), Buffer.concat(selected.map(s => headers.subarray(s.i * 600, s.i * 600 + 600))));
for (const { record } of selected) {
  const hex = record.subarray(0, 16).toString('hex');
  const id = `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
  await mkdir(join(sample, id[0]), { recursive: true });
  try { await copyFile(join(cache, id[0], `${id}.texture`), join(sample, id[0], `${id}.texture`)); } catch { /* Partial cache entries are expected. */ }
}
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  const page = await browser.newPage();
  const outbound = [];
  page.on('request', r => { if (!r.url().startsWith('http://127.0.0.1:5188/') && !r.url().startsWith('blob:')) outbound.push(r.url()); });
  await page.goto('http://127.0.0.1:5188/cachecat/app/');
  await page.locator('#folder-input').setInputFiles(sample);
  await page.waitForFunction(() => [...document.querySelectorAll('.texture-label span')].every(e => e.textContent !== 'Loading preview…') && document.querySelectorAll('.texture').length > 0, { timeout: 60_000 });
  const result = await page.evaluate(() => ({ total: document.querySelector('#library-count').textContent, visiblePreviews: document.querySelectorAll('.texture-preview img').length, error: document.querySelector('#error').textContent }));
  if (Number(result.total) !== 16 || result.visiblePreviews < 1 || result.error || outbound.length) throw new Error(JSON.stringify({ result, outbound }));
  console.log(JSON.stringify({ ...result, externalRequests: outbound.length }));
} finally { await browser.close(); }
