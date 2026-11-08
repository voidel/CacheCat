import './style.css';
import { uuidQuery, type Item } from './cache';
import { getRecord } from './store';

const app = document.querySelector<HTMLDivElement>('#app')!;
const cat = '<svg viewBox="0 0 48 48" fill="none" aria-hidden="true"><path d="M9 21 7 7l13 8a25 25 0 0 1 8 0L41 7l-2 14c5 17-3 22-15 22S4 38 9 21Z" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"/><path d="M15 27h2m14 0h2m-12 7 3 2 3-2M4 29l7 2m26 0 7-2M5 37h7m24 0h7" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"/></svg>';
app.innerHTML = `<aside class="sidebar">
  <a class="brand" href="../">${cat}<span>CacheCat<small>by pom.zone</small></span></a>
  <div class="nav-label">YOUR COLLECTION</div><div class="nav-active"><span>▦</span> Texture library <span id="library-count">0</span></div>
  <div class="source-box"><div class="eyebrow">CACHE FOLDER</div><div id="source">Nothing open yet</div><button id="open" class="button primary">Open cache folder <span>↗</span></button><button id="rescan" class="button subtle" disabled>Rescan folder</button></div>
  <div class="local-note"><span class="status-dot"></span><div>Local processing<small>Your textures never leave<br>this device.</small></div></div>
  <div class="sidebar-bottom"><a href="https://github.com/voidel/CacheCat" target="_blank" rel="noopener">Open-source on GitHub ↗</a><label for="theme">Appearance</label><select id="theme"><option value="system">System setting</option><option value="light">Light</option><option value="dark">Dark</option></select><a href="../">About CacheCat ↗</a></div>
</aside>
<main class="workspace"><header><div><p class="eyebrow">SECOND LIFE CACHE EXPLORER</p><h1>Texture library</h1><p class="subtitle">Browse and search your Second Life cache.</p></div><span class="local-badge">● &nbsp; LOCAL ONLY</span></header>
<div class="search-box"><span aria-hidden="true">⌕</span><input id="search" type="search" aria-label="Search textures or UUID" placeholder="Red silk, weathered stone, floral patterns… or a UUID" autocomplete="off" spellcheck="false"><kbd>/</kbd></div>
<div class="library-toolbar"><span id="result-count">Your texture library</span><span id="sort-label">↓ &nbsp; Most recent first</span></div>
<div id="notice" class="notice" role="status" hidden></div><div id="error" class="error" role="alert" hidden></div>
<section id="viewport" aria-label="Cached textures" tabindex="0"><div id="spacer"></div><div id="grid"></div><div id="empty"><div class="empty-cat">${cat}</div><p class="eyebrow">LOCAL CACHE VIEWER</p><h2>Open a texture cache</h2><p>Select your viewer’s texture cache folder.<br>Everything is read and processed on your device.</p><button id="empty-open" class="button primary">Choose a cache folder <span>↗</span></button><p class="small">Firestorm & Second Life · Read-only access</p></div></section>
<footer class="index-panel"><div class="index-heading"><div><span class="status-dot"></span><strong>Semantic index</strong><span id="backend">On-device SigLIP</span></div><button id="pause" class="text-button" disabled>Pause indexing</button></div><div class="progress-row"><progress id="progress" max="1" value="0" aria-label="Texture embedding progress"></progress><span id="percentage">0%</span></div><div class="index-meta"><span id="progress-label">0 / 0 embeddings indexed</span><span id="pending-label">Open a cache to begin</span></div><div class="model-row"><p id="status">Enable semantic search to find colours, shapes, and objects.<br><span>One-time model download: about 213 MB. Processing stays local.</span></p><button id="enable" class="button compact">Enable semantic search</button></div></footer></main>
<dialog id="details"><button id="close-details" class="close-button" aria-label="Close texture details">×</button><div id="detail-image"></div><p class="eyebrow">TEXTURE DETAILS</p><h2 id="detail-title"></h2><code id="detail-uuid"></code><p id="detail-status"></p><button id="copy" class="button primary">Copy UUID</button></dialog>
<input id="folder-input" type="file" webkitdirectory multiple hidden>`;

function el<T extends HTMLElement>(id: string) { return document.getElementById(id) as T; }
let worker: Worker | undefined, items: Item[] = [], visible: number[] = [];
let directory: FileSystemDirectoryHandle | undefined, selectedFiles: File[] | undefined;
let paused = false, query = '', modelState = 'off';
let total = 0, indexed = 0, unavailable = 0, pending = 0;
let renderQueued = false, queryTimer: ReturnType<typeof setTimeout>;
const viewport = el<HTMLDivElement>('viewport'), grid = el<HTMLDivElement>('grid');
const urls = new Map<string, string>(), pendingUrls = new Map<string, Promise<string | undefined>>();
const volatile = new Map<string, Blob>();
function warning(message: string) { el('error').textContent = message; el('error').hidden = false; }
function status(message: string) { el('status').textContent = message; }
function requestRender() { if (!renderQueued) { renderQueued = true; requestAnimationFrame(() => { renderQueued = false; render(); }); } }
async function previewUrl(hash: string) {
  if (urls.has(hash)) { const url = urls.get(hash)!; urls.delete(hash); urls.set(hash, url); return url; }
  if (pendingUrls.has(hash)) return pendingUrls.get(hash);
  const promise = (async () => {
    const blob = volatile.get(hash) ?? (await getRecord(hash).catch(() => undefined))?.thumbnail;
    if (!blob) return;
    const url = URL.createObjectURL(blob); urls.set(hash, url);
    while (urls.size > 150) { const old = urls.keys().next().value!; URL.revokeObjectURL(urls.get(old)!); urls.delete(old); }
    return url;
  })();
  pendingUrls.set(hash, promise);
  try { return await promise; } finally { pendingUrls.delete(hash); }
}
function updateProgress() {
  el<HTMLProgressElement>('progress').max = Math.max(1, total); el<HTMLProgressElement>('progress').value = indexed;
  el('percentage').textContent = `${total ? Math.floor(indexed / total * 100) : 0}%`;
  el('progress-label').textContent = `${indexed.toLocaleString()} / ${total.toLocaleString()} embeddings indexed`;
  el('pending-label').textContent = `${pending.toLocaleString()} pending${unavailable ? ` · ${unavailable.toLocaleString()} unavailable` : ''}`;
  const partial = total > 0 && pending > 0;
  el('notice').hidden = !partial;
  el('notice').textContent = modelState !== 'ready' ? 'UUID filtering is available now. Enable semantic search to index and search texture descriptions.' : `${paused ? 'Indexing is paused' : 'Indexing is not complete yet'} — results may be partial. Search uses the ${indexed.toLocaleString()} embeddings ready so far.`;
  if (!pending && unavailable) { el('notice').hidden = false; el('notice').textContent = `Indexing finished. ${unavailable.toLocaleString()} incomplete or unsupported textures could not be indexed; search covers the remaining ${indexed.toLocaleString()}.`; }
  el('library-count').textContent = total.toLocaleString();
}
function render() {
  const semantic = query.trim() && !uuidQuery(query);
  el('result-count').textContent = `${visible.length.toLocaleString()} textures${semantic ? ' · best matches' : ''}`;
  el('sort-label').textContent = semantic ? '↗  Ranked by similarity' : '↓  Most recent first';
  const empty = el('empty'); empty.hidden = visible.length > 0;
  if (!visible.length) {
    grid.replaceChildren(); el('spacer').style.height = '0px';
    if (items.length) {
      empty.querySelector('h2')!.textContent = semantic && !indexed ? 'Indexing textures' : 'No matching textures';
      empty.querySelector('p:not(.eyebrow):not(.small)')!.textContent = semantic && !indexed ? 'Enable semantic search. Matches will appear as embeddings are indexed.' : 'Try a different UUID or description.';
      el('empty-open').hidden = true;
    } else el('empty-open').hidden = false;
    return;
  }
  const columns = Math.max(1, Math.floor((viewport.clientWidth + 16) / 190));
  const rowHeight = 224, rows = Math.ceil(visible.length / columns);
  const first = Math.max(0, Math.floor(viewport.scrollTop / rowHeight) - 1);
  const last = Math.min(rows, Math.ceil((viewport.scrollTop + viewport.clientHeight) / rowHeight) + 2);
  el('spacer').style.height = `${rows * rowHeight}px`;
  grid.style.gridTemplateColumns = `repeat(${columns}, minmax(0,1fr))`;
  grid.style.transform = `translateY(${first * rowHeight}px)`;
  const fragment = document.createDocumentFragment();
  for (let n = first * columns; n < Math.min(visible.length, last * columns); n++) {
    const i = visible[n], item = items[i]; if (!item) continue;
    const card = document.createElement('button'); card.className = 'texture'; card.type = 'button'; card.setAttribute('aria-label', `Texture ${item.id}`);
    const preview = document.createElement('div'); preview.className = 'texture-preview';
    const label = document.createElement('div'); label.className = 'texture-label';
    const id = document.createElement('code'); id.textContent = `${item.id.slice(0, 8)}…${item.id.slice(-4)}`;
    const sub = document.createElement('span'); sub.textContent = item.indexed ? '● Search ready' : item.error ? 'Preview unavailable' : item.width ? `${item.width} × ${item.height}` : 'Loading preview…';
    if (item.indexed) sub.className = 'ready';
    label.append(id, sub); card.append(preview, label); card.onclick = () => void showDetail(i); fragment.append(card);
    if (item.hash) void previewUrl(item.hash).then(url => { if (!url || !card.isConnected) return; const img = document.createElement('img'); img.src = url; img.alt = ''; img.decoding = 'async'; preview.append(img); });
    else preview.textContent = item.error ? 'Incomplete texture' : '· · ·';
  }
  grid.replaceChildren(fragment);
}
async function showDetail(i: number) {
  const item = items[i], dialog = el<HTMLDialogElement>('details');
  el('detail-title').textContent = item.width ? `${item.width} × ${item.height}` : 'Cached texture';
  el('detail-uuid').textContent = item.id;
  el('detail-status').textContent = item.error ?? `Last used by viewer: ${new Date(item.timestamp * 1000).toLocaleString()}${item.indexed ? ' · Search ready' : ' · Not yet indexed'}`;
  el('detail-image').replaceChildren();
  if (!dialog.open) dialog.showModal();
  if (item.hash) { const url = await previewUrl(item.hash); if (url && el('detail-uuid').textContent === item.id) { const img = document.createElement('img'); img.src = url; img.alt = 'Selected cached texture'; el('detail-image').replaceChildren(img); } }
  el('copy').textContent = 'Copy UUID';
  el('copy').onclick = async () => { try { await navigator.clipboard.writeText(item.id); el('copy').textContent = 'Copied'; } catch { el('copy').textContent = 'Select and copy the UUID above'; } };
}
function openSource(source: { directory: FileSystemDirectoryHandle } | { files: File[] }) {
  worker?.terminate(); worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' });
  items = []; visible = []; total = indexed = unavailable = pending = 0; paused = false; modelState = 'off';
  el('error').hidden = true; el('source').textContent = 'directory' in source ? source.directory.name : source.files[0]?.webkitRelativePath.split('/')[0] ?? 'Selected folder';
  el<HTMLButtonElement>('rescan').disabled = false; el<HTMLButtonElement>('pause').disabled = false;
  el('pause').textContent = 'Pause indexing'; el<HTMLButtonElement>('enable').disabled = false; el('enable').hidden = false; el('enable').textContent = 'Enable semantic search';
  el('backend').textContent = 'On-device SigLIP'; viewport.scrollTop = 0; status('Reading your cache locally…'); updateProgress(); requestRender();
  worker.onerror = event => { warning(`The background worker stopped: ${event.message}. Rescan to restart.`); };
  worker.onmessage = ({ data }) => {
    if (data.type === 'loaded') { items = data.items; total = items.length; status('Loading texture previews…'); worker!.postMessage({ type: 'search', query }); }
    else if (data.type === 'item') { items[data.i] = data.item; requestRender(); }
    else if (data.type === 'results' && data.query === query) { visible = data.matches; requestRender(); }
    else if (data.type === 'progress') {
      ({ total, indexed, unavailable, pending } = data); el('backend').textContent = data.backend; updateProgress();
      if (!pending && total) status(unavailable ? 'Indexing finished. Unavailable textures are excluded from semantic results.' : 'Indexing finished. Search is ready.');
      else if (data.previewsComplete && modelState === 'off') status('Previews loaded. Enable semantic search to index texture descriptions.');
    }
    else if (data.type === 'status') status(data.message);
    else if (data.type === 'warning' || data.type === 'error') warning(data.message);
    else if (data.type === 'preview') { volatile.set(data.hash, data.blob); if (volatile.size > 150) volatile.delete(volatile.keys().next().value!); }
    else if (data.type === 'model') {
      modelState = data.state;
      el<HTMLButtonElement>('enable').disabled = data.state === 'loading'; el('enable').textContent = data.state === 'loading' ? 'Loading model…' : 'Retry semantic search';
      el('enable').hidden = data.state === 'ready';
      if (data.state === 'ready') status('Indexing on your device. You can search straight away.'); updateProgress();
    }
  };
  worker.postMessage({ type: 'open', source });
}
async function pickFolder() {
  el('error').hidden = true;
  if (window.showDirectoryPicker) {
    try { directory = await window.showDirectoryPicker({ mode: 'read' }); selectedFiles = undefined; openSource({ directory }); }
    catch (error) {
      if ((error as DOMException).name !== 'AbortError') {
        warning(`Folder access failed: ${(error as Error).message}. Select Open cache folder again to use the alternative folder picker.`);
        el('open').onclick = () => el<HTMLInputElement>('folder-input').click();
        el('empty-open').onclick = () => el<HTMLInputElement>('folder-input').click();
      }
    }
  } else el<HTMLInputElement>('folder-input').click();
}
el('open').onclick = () => void pickFolder(); el('empty-open').onclick = () => void pickFolder();
el<HTMLInputElement>('folder-input').onchange = event => { selectedFiles = Array.from((event.target as HTMLInputElement).files ?? []); directory = undefined; if (selectedFiles.length) openSource({ files: selectedFiles }); };
el('rescan').onclick = () => { if (directory) openSource({ directory }); else el<HTMLInputElement>('folder-input').click(); };
el('pause').onclick = () => { paused = !paused; worker?.postMessage({ type: 'pause', paused }); el('pause').textContent = paused ? 'Resume indexing' : 'Pause indexing'; updateProgress(); };
el('enable').onclick = () => { if (!worker) { warning('Open a cache folder first, then enable semantic search.'); return; } worker.postMessage({ type: 'enable' }); };
el<HTMLInputElement>('search').oninput = event => {
  query = (event.target as HTMLInputElement).value; clearTimeout(queryTimer); viewport.scrollTop = 0; visible = []; requestRender();
  queryTimer = setTimeout(() => worker?.postMessage({ type: 'search', query }), 180);
};
viewport.addEventListener('scroll', requestRender, { passive: true }); new ResizeObserver(requestRender).observe(viewport);
el('close-details').onclick = () => el<HTMLDialogElement>('details').close();
const theme = el<HTMLSelectElement>('theme');
try { theme.value = localStorage.getItem('cachecat-theme') ?? 'system'; } catch { /* Private browsing. */ }
function applyTheme() { document.documentElement.dataset.theme = theme.value; try { localStorage.setItem('cachecat-theme', theme.value); } catch { /* Theme still applies to this visit. */ } }
theme.onchange = applyTheme; applyTheme();
document.addEventListener('keydown', event => { if (event.key === '/' && !(event.target instanceof HTMLInputElement) && !el<HTMLDialogElement>('details').open) { event.preventDefault(); el('search').focus(); } });
requestRender();
