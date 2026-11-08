# Architecture and data handling

## Desktop

Default persistent storage is `data/` beside the executable (beside the `.app`
bundle on macOS). `--data-dir` remains an explicit override. The first default
launch without a portable database imports the former platform user-data location
on the background worker. The UI initially reads the former settings if needed.
Imports copy only models, settings and a consistent SQLite backup,
including WAL contents. Files are installed atomically without overwriting existing
portable files; the database is published last so interrupted imports can resume.
The source is retained. Release packaging always uses a fresh staging directory
to prevent accidental inclusion of portable data from previous local runs.

The GPUI thread renders a virtualized grid and handles input. A background indexer
reads headers, hashes content and maintains the SQLite embedding index. Entries
with saved embeddings need no image decode; missing embeddings are prepared and
indexed incrementally. Search uses all currently available embeddings and refreshes
as new embeddings arrive.

A separate preview worker decodes only the current grid viewport and the selected
detail item, even while indexing is paused or loading the model. New viewport
requests replace queued work; obsolete generations/results are discarded. One
image is decoded at a time, with at most two completed 256px previews waiting for
the UI. The UI retains only visible previews and the selected detail preview;
eviction explicitly releases GPUI image resources as well as CPU memory. No
thumbnail files are read or written. Older thumbnail folders are ignored and are
not imported into portable storage. Rescanning invalidates in-memory previews.

Search mode is explicit: Semantic is the default; UUID filtering never requests
the model. A semantic query without a ready model opens a download dialog showing
the pinned public source, progress and cancellation. Downloads run separately from
the cache worker, and cancellation aborts pending network reads and removes partial
files. Verified complete files are reused on retry. The dialog closes when the
model loads successfully, and indexing and the pending query continue automatically.

After eight initial metadata records, native indexing alternates preparation and
inference. Up to 16 prepared 224×224 tensors (about 9.2 MiB) can wait for inference;
full-size textures are not retained. Content hashes are rechecked before using prepared data.
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

Content hashes invalidate embeddings when bytes change; an independent
model key invalidates embeddings when preprocessing or weights change. A changed
texture between preview and indexing is reported for rescan. Model downloads use
a fixed repository revision, exact file sizes and SHA-256 verification before rename.

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
- Native release archives are unsigned/ad-hoc signed until a maintainer supplies
  platform signing and notarization credentials. CI produces reviewable draft releases.
