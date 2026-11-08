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
  await expect(page.locator('#notice')).toContainText('Choose UUID');
  await page.locator('#search-mode').selectOption('uuid');
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
    const downloadUpdates: number[] = [];
    Object.assign(window, { embeddingUpdates: updates, downloadUpdates });
    new MutationObserver(() => downloadUpdates.push((document.querySelector('#model-progress') as HTMLProgressElement).value))
      .observe(document.querySelector('#model-progress')!, { attributes: true });
    new MutationObserver(() => updates.push(`${document.querySelector('#progress-label')!.textContent}|${document.querySelector('#result-count')!.textContent}|${document.querySelector('#notice')!.textContent}`))
      .observe(document.querySelector('.workspace')!, { childList: true, subtree: true, characterData: true });
  });
  await expect(page.locator('#model-dialog')).toBeVisible();
  await page.getByRole('button', { name: 'Download model', exact: true }).click();
  await Promise.race([
    expect(page.locator('#progress-label')).toHaveText('4 / 4 embeddings indexed', { timeout: 240_000 }),
    page.locator('#error').waitFor({ state: 'visible', timeout: 240_000 }).then(async () => { throw new Error(await page.locator('#error').innerText()); }),
  ]);
  await expect(page.locator('.texture').first()).toHaveAttribute('aria-label', 'Texture 00000000-0000-0000-0000-000000000001');
  await expect(page.locator('#percentage')).toHaveText('100%');
  await expect(page.locator('#model-dialog')).not.toBeVisible();
  expect((await page.evaluate(() => (window as any).downloadUpdates as number[])).some(value => value > 0)).toBe(true);
  const updates = await page.evaluate(() => (window as any).embeddingUpdates as string[]);
  expect(updates.some(text => /^[1-3] \/ 4 embeddings indexed\|[1-3] textures/.test(text) && text.includes('results may be partial'))).toBe(true);
  console.info('Browser inference backend:', await page.locator('#backend').innerText());
  await page.context().route('https://huggingface.co/**', route => route.abort('internetdisconnected'));
  const remoteAfterReload: string[] = [];
  page.on('request', request => { if (request.url().startsWith('https://huggingface.co/')) remoteAfterReload.push(request.url()); });
  await page.reload();
  await page.locator('#folder-input').setInputFiles(fixture);
  await expect(page.locator('#progress-label')).toHaveText('4 / 4 embeddings indexed');
  await page.getByRole('searchbox').fill('red stripes');
  await expect(page.locator('.texture').first()).toHaveAttribute('aria-label', 'Texture 00000000-0000-0000-0000-000000000001');
  await expect(page.locator('#model-dialog')).not.toBeVisible();
  expect(writes).toEqual([]);
  expect(remoteAfterReload).toEqual([]);
});


test('search modes prompt only for semantic searches and allow dismissal', async ({ page }) => {
  const outbound: string[] = [];
  page.on('request', request => { if (request.url().includes('huggingface.co')) outbound.push(request.url()); });
  await page.goto('/cachecat/app/');
  await expect(page.locator('#search-mode')).toHaveValue('semantic');
  await page.locator('#folder-input').setInputFiles(fixture);
  await expect(page.locator('.texture-preview img')).toHaveCount(4);
  await page.getByRole('searchbox').fill('00000000');
  await expect(page.locator('#model-dialog')).toBeVisible();
  await expect(page.locator('#model-source')).toHaveAttribute('href', /huggingface.co\/Xenova\/siglip-base-patch16-224\/tree\/4649052/);
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await expect(page.locator('#model-dialog')).not.toBeVisible();
  await page.locator('#search-mode').selectOption('uuid');
  await expect(page.locator('.texture')).toHaveCount(4);
  await page.getByRole('searchbox').fill('not-a-uuid');
  await expect(page.locator('.texture')).toHaveCount(0);
  await expect(page.locator('#model-dialog')).not.toBeVisible();
  await page.locator('#search-mode').selectOption('semantic');
  await expect(page.locator('#model-dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#model-dialog')).not.toBeVisible();
  expect(outbound).toEqual([]);
});

test('download failure stays in the modal and cancellation restores UUID search', async ({ page, context }) => {
  await context.route('https://huggingface.co/**', route => route.abort('internetdisconnected'));
  await page.goto('/cachecat/app/');
  await page.locator('#folder-input').setInputFiles(fixture);
  await page.getByRole('searchbox').fill('red stripes');
  await page.getByRole('button', { name: 'Download model', exact: true }).click();
  await expect(page.locator('#model-error')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Retry download' })).toBeEnabled();
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.locator('#search-mode').selectOption('uuid');
  await page.getByRole('searchbox').fill('00000000-0000-0000-0000-000000000002');
  await expect(page.locator('.texture')).toHaveCount(1);
});

test('active download can be cancelled without losing the cache selection', async ({ page, context }) => {
  let requested = false;
  let release!: () => void;
  const blocked = new Promise<void>(resolve => { release = resolve; });
  await context.route('https://huggingface.co/**', async route => { requested = true; await blocked; await route.abort().catch(() => {}); });
  try {
    await page.goto('/cachecat/app/');
    await page.locator('#folder-input').setInputFiles(fixture);
    await page.getByRole('searchbox').fill('red stripes');
    await page.getByRole('button', { name: 'Download model', exact: true }).click();
    await expect.poll(() => requested).toBe(true);
    await expect(page.locator('#model-progress-area')).toBeVisible();
    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(page.locator('#model-dialog')).not.toBeVisible();
    await page.locator('#search-mode').selectOption('uuid');
    await page.getByRole('searchbox').fill('00000000-0000-0000-0000-000000000002');
    await expect(page.locator('.texture')).toHaveCount(1);
    await page.locator('#search-mode').selectOption('semantic');
    await expect(page.locator('#model-dialog')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Download model', exact: true })).toBeEnabled();
  } finally { release(); }
});
