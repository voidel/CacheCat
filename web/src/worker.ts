/// <reference lib="webworker" />
import { parseEntries, uuidQuery, similarity, type Item } from './cache';
import { decode, thumbnail } from './decoder';
import { getRecord, putRecord, MODEL_KEY, type RecordData } from './store';
import type { Inference } from './inference';

type Source = { directory: FileSystemDirectoryHandle } | { files: File[] };
let readFile: (path: string) => Promise<File>;
let headers: File;
let items: Item[] = [], vectors: (Float32Array | undefined)[] = [];
let cursor = 0, embedQueue: number[] = [], model: Inference | undefined;
let paused = false, running = false, loaded = false, modelLoading = false;
let query = '', queryVector: Float32Array | undefined, searchPending = false;
let sinceEmbedding = 0, storageWarning = false;
const volatile = new Map<string, RecordData>();
const send = (type: string, data: object = {}) => self.postMessage({ type, ...data });
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

async function sourceReader(source: Source) {
  if ('directory' in source) {
    let root: FileSystemDirectoryHandle | undefined;
    for (const parts of [[], ['texturecache'], ['FirestormCache', 'texturecache']]) {
      try {
        let dir = source.directory;
        for (const part of parts) dir = await dir.getDirectoryHandle(part);
        await dir.getFileHandle('texture.entries'); root = dir; break;
      } catch { /* Try the next known viewer layout. */ }
    }
    if (!root) throw new Error('Choose your Firestorm cache folder or its texturecache subfolder.');
    const directory = root;
    return async (path: string) => {
      const parts = path.split('/'); let dir = directory;
      for (const part of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(part);
      return (await dir.getFileHandle(parts.at(-1)!)).getFile();
    };
  }
  const files = new Map(source.files.map(file => [file.webkitRelativePath || file.name, file]));
  const index = [...files.keys()].find(path => path === 'texture.entries' || path.endsWith('/texture.entries'));
  if (!index) throw new Error('No texture.entries found. Select the viewer’s texturecache folder.');
  const prefix = index.slice(0, -'texture.entries'.length);
  return async (path: string) => { const file = files.get(prefix + path); if (!file) throw new Error('Texture body is not cached.'); return file; };
}
async function readTexture(item: Item): Promise<Uint8Array> {
  const header = new Uint8Array(await headers.slice(item.slot * 600, item.slot * 600 + Math.min(600, item.imageSize)).arrayBuffer());
  if (header.length !== Math.min(600, item.imageSize)) throw new Error('Texture header is missing; rescan.');
  let body = new Uint8Array(0);
  if (item.bodySize) {
    const path = `${item.id[0]}/${item.id}.texture`;
    try {
      const file = await readFile(path).catch(() => readFile(`textures/${path}`));
      body = new Uint8Array(await file.slice(0, item.bodySize).arrayBuffer());
    } catch { /* Decode the cached low-resolution header if possible. */ }
  }
  const bytes = new Uint8Array(header.length + body.length); bytes.set(header); bytes.set(body, header.length); return bytes;
}
async function hashBytes(bytes: Uint8Array): Promise<string> {
  return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(bytes))), x => x.toString(16).padStart(2, '0')).join('');
}
async function recordFor(hash: string): Promise<RecordData | undefined> {
  return volatile.get(hash) ?? getRecord(hash).catch(() => undefined);
}
async function persist(record: RecordData) {
  try { await putRecord(record); }
  catch {
    // Keep browsing usable when private mode or quota blocks IndexedDB.
    volatile.set(record.hash, record);
    if (volatile.size > 100) volatile.delete(volatile.keys().next().value!);
    if (!storageWarning) { storageWarning = true; send('warning', { message: 'Browser storage is unavailable or full. Some previews and embeddings cannot be saved between visits.' }); }
    send('preview', { hash: record.hash, blob: record.thumbnail });
  }
}
function progress() {
  const indexed = vectors.filter(Boolean).length;
  const unavailable = items.filter((item, i) => item.error && !vectors[i]).length;
  send('progress', { total: items.length, indexed, unavailable, pending: items.length - indexed - unavailable, previewsComplete: cursor >= items.length, backend: model?.backend ?? 'Model not loaded', paused });
}
function results() {
  const uuid = uuidQuery(query);
  let matches: number[] = [];
  if (!query.trim()) matches = items.map((_, i) => i);
  else if (uuid) matches = items.flatMap((item, i) => item.id.replaceAll('-', '').includes(uuid) ? [i] : []);
  else if (queryVector) matches = vectors.flatMap((v, i) => v ? [{ i, score: similarity(v, queryVector!) }] : []).sort((a, b) => b.score - a.score || a.i - b.i).slice(0, 200).map(v => v.i);
  send('results', { query, matches });
}
async function processPreview(i: number) {
  const item = items[i];
  try {
    const bytes = await readTexture(item), hash = await hashBytes(bytes);
    let record = await recordFor(hash);
    if (!record) {
      const image = await decode(bytes);
      record = { hash, thumbnail: await thumbnail(image), width: image.width, height: image.height };
      await persist(record);
    }
    item.hash = hash; item.width = record.width; item.height = record.height;
    if (record.model === MODEL_KEY && record.vector?.length === 768 && record.vector.every(Number.isFinite)) { vectors[i] = record.vector; item.indexed = true; }
    else embedQueue.push(i);
  } catch (error) { item.error = errorText(error); }
  send('item', { i, item });
}
async function processEmbedding(i: number) {
  const item = items[i];
  try {
    const bytes = await readTexture(item);
    if (await hashBytes(bytes) !== item.hash) throw new Error('Texture changed; rescan to refresh.');
    const image = await decode(bytes);
    const vector = await model!.embed(image.rgba, image.width, image.height);
    const record = await recordFor(item.hash!) ?? { hash: item.hash!, thumbnail: await thumbnail(image), width: image.width, height: image.height };
    record.vector = vector; record.model = MODEL_KEY; await persist(record);
    vectors[i] = vector; item.indexed = true;
  } catch (error) { item.error = `Indexing: ${errorText(error)}`; }
  send('item', { i, item });
}
async function pump() {
  if (running || !loaded) return;
  running = true;
  try {
    while (loaded) {
      if (searchPending) {
        searchPending = false;
        const current = query;
        queryVector = undefined;
        if (current.trim() && !uuidQuery(current) && model) {
          try { const vector = await model.query(current); if (current === query) queryVector = vector; }
          catch (error) { send('warning', { message: `Search failed: ${errorText(error)}` }); }
        }
        if (current === query) results();
      }
      if (paused) break;
      if (cursor < items.length && (!model || sinceEmbedding < 4 || !embedQueue.length)) {
        await processPreview(cursor++); sinceEmbedding++;
      } else if (model && embedQueue.length) {
        await processEmbedding(embedQueue.shift()!); sinceEmbedding = 0;
        if (query.trim()) results(); // Search includes each completed embedding immediately.
      } else break;
      if (cursor % 8 === 0 || sinceEmbedding === 0) { progress(); if (queryVector) results(); }
      await new Promise(resolve => setTimeout(resolve, 4));
    }
  } catch (error) { send('error', { message: errorText(error) }); }
  finally { running = false; progress(); results(); }
}
self.onmessage = async ({ data }) => {
  try {
    if (data.type === 'open') {
      readFile = await sourceReader(data.source);
      const entries = await readFile('texture.entries');
      if (entries.size > 128 * 1024 * 1024) throw new Error('Texture index exceeds the supported size.');
      items = parseEntries(await entries.arrayBuffer()); vectors = new Array(items.length);
      headers = await readFile('texture.cache'); loaded = true;
      send('loaded', { items }); progress(); results(); void pump();
    } else if (data.type === 'pause') { paused = data.paused; progress(); void pump(); }
    else if (data.type === 'search') { query = data.query; queryVector = undefined; searchPending = true; results(); void pump(); }
    else if (data.type === 'enable' && !modelLoading && !model) {
      modelLoading = true; send('model', { state: 'loading' });
      try {
        const { Inference } = await import('./inference');
        const candidate = new Inference();
        await candidate.load(message => send('status', { message }));
        model = candidate; searchPending = true;
        send('model', { state: 'ready', backend: model.backend }); void pump();
      } catch (error) { send('model', { state: 'error' }); throw error; }
      finally { modelLoading = false; }
    }
  } catch (error) { send('error', { message: errorText(error) }); }
};
