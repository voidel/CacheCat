# Architecture and data handling

## Desktop

Default persistent storage is `data/` beside the executable (beside the `.app`
bundle on macOS). `--data-dir` remains an explicit override. The first default
launch without a portable database imports the former platform user-data location
on the background worker. The UI initially reads the former settings if needed.
Imports copy only models, thumbnails, settings and a consistent SQLite backup,
including WAL contents. Files are installed atomically without overwriting existing
portable files; the database is published last so interrupted imports can resume.
The source is retained. Release packaging always uses a fresh staging directory
to prevent accidental inclusion of portable data from previous local runs.

The GPUI thread renders a virtualized grid and handles input. A single background
worker reads the cache, hashes content, generates thumbnails and maintains a SQLite
embedding index. It processes a small preview batch first, then interleaves previews
with embeddings. Commands are handled between items. Search uses all currently
available embeddings and refreshes as new embeddings arrive.

Search mode is explicit: Semantic is the default; UUID filtering never requests
the model. A semantic query without a ready model opens a download dialog showing
the pinned public source, progress and cancellation. Downloads run separately from
the cache worker, and cancellation aborts pending network reads and removes partial
files. Verified complete files are reused on retry. The dialog closes when the
model loads successfully, and indexing and the pending query continue automatically.

After eight initial previews, native indexing alternates preparation and inference.
Up to 16 prepared 224×224 tensors (about 9.2 MiB) reuse the preview decode; full-size
textures are not retained. Content hashes are rechecked before using prepared data.
Identical content queued under multiple UUIDs reuses the first saved embedding.
Search computes each similarity once and selects the best 200 before sorting.

Native display ordering is separate from search ranking: users can order the current
matches by recency, pixel area or UUID, or retain relevance (newest first without a
text query). Unknown dimensions sort last and update as cache metadata is loaded.
Theme, sort and both panel widths are saved in the existing settings file; older
settings gain defaults without losing the selected folder. The selected folder is
shown in the window title. Progress counts come from one UI snapshot rather than
mixing worker status strings with UI counters.

Native PNG/TGA exports use the current cache record for the selected UUID, decode
the original JPEG 2000 bytes, and preserve RGBA pixels. Saving runs off the UI thread
after the user chooses a destination. Export never uses the thumbnail or embedding
input, and it reports entries that have been evicted from the viewer cache.

ONNX Runtime and GPU drivers run in a child process over private stdin/stdout pipes.
The parent retries on CPU after a failed accelerator startup or inference process
exit. Windows crash-dialog suppression is confined to the helper. The helper never
reads the viewer cache directly; it receives preprocessed pixels or query text.
Neither process uploads them. `--cpu` bypasses accelerator probing.

Image tensors use bounded binary pipe frames rather than individual JSON-number
writes. This preserves float values exactly and keeps driver crash isolation.
CPU sessions reuse memory allocations and use up to four inference threads by
default (at most half the available logical processors); idle threads do not spin.
`CACHECAT_CPU_THREADS=1..16` overrides the CPU thread count for local benchmarking.

CacheCat opens viewer files read-only. It reconstructs each JPEG 2000 stream from
the slot's first 600 bytes and its UUID-named body. Version 1.71's 44-byte header
contains address size and encoder information; version 1.7's header is 8 bytes.
Allocated trailing records are ignored. Nil/deleted/invalid records are skipped,
and duplicate UUIDs retain the newest live record.

Content hashes invalidate thumbnails/embeddings when bytes change; an independent
model key invalidates embeddings when preprocessing or weights change. A changed
texture between preview and indexing is reported for rescan. Model downloads use
a fixed repository revision, exact file sizes and SHA-256 verification before rename.

## Browser

The main thread owns accessible DOM controls and a virtualized texture grid.
A worker receives read-only directory handles or selected File objects. It uses
`Blob.slice()` for header slots and the required body files, decodes with OpenJPEG
WebAssembly, and runs SigLIP through Transformers.js/ONNX Runtime Web. WebGPU is
probed using both models; unsupported execution falls back to WASM CPU.

Thumbnails and embeddings live in IndexedDB, keyed by texture-content hash and
model/preprocessing identity. The UI keeps a bounded LRU of object URLs for visible
thumbnails. Browser Cache Storage holds model assets. Storage can be evicted by the
browser; quota errors are surfaced, with a bounded in-memory preview fallback.

The browser uses the same search/download flow. Fully cached model assets are
loaded without remote downloads. Cancelling setup terminates the worker to abort
fetches, then restores the selected cache and saved embeddings in a fresh worker.

Search commands never become URLs. UUID searches operate on the catalogue directly;
text queries produce local normalized embeddings. Cosine similarity ranks the
currently indexed subset. The UI shows indexed/total, pending, unavailable and
partial-result state. A latest-query check rejects stale results.

Only application/runtime/public-model downloads use the network. There is no
upload API or telemetry. The static host denies write methods; CSP limits worker,
script and connection origins. No source cache directory is mounted into a server.
The browser only sees a selected folder after the user chooses it.

## Scope and current limitations

- Recency means viewer last-use time, not texture creation or download time.
- The viewer may change cache files while running. Rescan refreshes the snapshot.
- Missing or incomplete JPEG 2000 streams can be undecodable; they are counted
  separately. No texture is invented to replace missing data.
- Embeddings are similarity representations, not generated tags or guaranteed labels.
- The first index is incremental and may take time on large caches. Pause takes
  effect between items. Native model downloads currently finish before queued
  indexing commands resume.
- GPU operator coverage depends on the runtime and driver. Native Linux CUDA is
  an optional build; the portable Linux build uses CPU inference.
- Browser persistence is subject to quotas/eviction. Background tabs can be suspended.
- Native release archives are unsigned/ad-hoc signed until a maintainer supplies
  platform signing and notarization credentials. CI produces reviewable draft releases.
