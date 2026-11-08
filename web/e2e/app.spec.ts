import { test, expect } from '@playwright/test';
import { resolve } from 'node:path';
const fixture = resolve('tests/fixtures/texturecache');
test('denied directory handles offer the alternative folder picker', async ({ page }) => {
  await page.addInitScript(() => Object.defineProperty(window, 'showDirectoryPicker', {
    value: async () => { throw new DOMException('Access denied', 'SecurityError'); },
  }));
  await page.goto('/cachecat/app/');
  await page.getByRole('button', { name: 'Open cache folder' }).click();
  await expect(page.locator('#error')).toContainText('alternative folder picker');
  const chooser = page.waitForEvent('filechooser');
  await page.getByRole('button', { name: 'Open cache folder' }).click();
  await (await chooser).setFiles(fixture);
  await expect(page.locator('.texture-preview img')).toHaveCount(4);
});
test('landing has feature details, browser entry point and all platform links', async ({ page }) => {
  await page.goto('/cachecat/');
  await expect(page.getByRole('heading', { name: 'Second Life cache viewer.' })).toBeVisible();
  await expect(page.getByRole('link', { name: /Windows releases/ })).toHaveAttribute('href', 'https://github.com/voidel/CacheCat/releases');
  await expect(page.getByRole('link', { name: /macOS releases/ })).toBeVisible();
  await expect(page.getByRole('link', { name: /Linux releases/ })).toBeVisible();
  await page.getByRole('link', { name: /Try it in your browser/ }).click();
  await expect(page.getByRole('searchbox')).toBeVisible();
});
test('local folder decodes JPEG 2000, filters UUIDs, and reports partial embeddings', async ({ page }) => {
  const errors: string[] = [], outbound: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (!request.url().startsWith('http://127.0.0.1:') && !request.url().startsWith('blob:')) outbound.push(request.url()); });
  await page.goto('/cachecat/app/');
  await page.locator('#folder-input').setInputFiles(fixture);
  await expect(page.locator('.texture')).toHaveCount(4);
  await expect(page.locator('.texture-preview img')).toHaveCount(4, { timeout: 30_000 });
  await expect(page.locator('#progress-label')).toHaveText('0 / 4 embeddings indexed');
  await expect(page.locator('#notice')).toContainText('UUID filtering is available now');
  await page.getByRole('searchbox').fill('00000000-0000-0000-0000-000000000002');
  await expect(page.locator('.texture')).toHaveCount(1);
  await page.locator('.texture').click();
  await expect(page.locator('#detail-title')).toHaveText('128 × 128');
  await page.getByRole('button', { name: 'Close texture details' }).click();
  await page.locator('#theme').selectOption('dark');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Resume indexing');
  expect(errors).toEqual([]); expect(outbound).toEqual([]);
});
test('layouts fit a narrow screen', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  for (const path of ['/cachecat/', '/cachecat/app/']) {
    await page.goto(path);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  }
});
test('real browser SigLIP indexes progressively and ranks a matching texture', async ({ page }) => {
  test.skip(!process.env.CACHECAT_MODEL_TEST, 'Opt-in 213 MB model smoke test');
  test.setTimeout(300_000);
  const writes: string[] = [];
  page.on('request', request => { if (!['GET', 'HEAD'].includes(request.method())) writes.push(request.url()); });
  await page.goto('/cachecat/app/');
  await page.locator('#folder-input').setInputFiles(fixture);
  await expect(page.locator('.texture-preview img')).toHaveCount(4);
  await page.getByRole('searchbox').fill('red stripes');
  await page.evaluate(() => {
    const updates: string[] = [];
    Object.assign(window, { embeddingUpdates: updates });
    new MutationObserver(() => updates.push(`${document.querySelector('#progress-label')!.textContent}|${document.querySelector('#result-count')!.textContent}|${document.querySelector('#notice')!.textContent}`))
      .observe(document.querySelector('.workspace')!, { childList: true, subtree: true, characterData: true });
  });
  await page.getByRole('button', { name: 'Enable semantic search', exact: true }).click();
  await Promise.race([
    expect(page.locator('#progress-label')).toHaveText('4 / 4 embeddings indexed', { timeout: 240_000 }),
    page.locator('#error').waitFor({ state: 'visible', timeout: 240_000 }).then(async () => { throw new Error(await page.locator('#error').innerText()); }),
  ]);
  await expect(page.locator('.texture').first()).toHaveAttribute('aria-label', 'Texture 00000000-0000-0000-0000-000000000001');
  await expect(page.locator('#percentage')).toHaveText('100%');
  const updates = await page.evaluate(() => (window as any).embeddingUpdates as string[]);
  expect(updates.some(text => /^[1-3] \/ 4 embeddings indexed\|[1-3] textures/.test(text) && text.includes('results may be partial'))).toBe(true);
  console.info('Browser inference backend:', await page.locator('#backend').innerText());
  await page.reload();
  await page.locator('#folder-input').setInputFiles(fixture);
  await expect(page.locator('#progress-label')).toHaveText('4 / 4 embeddings indexed');
  expect(writes).toEqual([]);
});
