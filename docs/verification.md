# Verification record

Local checks, 2026-11-08:

- Windows: `cargo check`, five Rust unit tests, and strict Clippy passed.
- Optimized Windows release archive built successfully. The packaged executable
  decoded all four synthetic textures and correctly ranked red stripes, recovering
  from GPU startup failure on CPU with exit code 0 and no interactive window.
- Linux: isolated Debian container `cargo check --locked` and all five Rust unit tests passed.
- Read-only scan of the supplied Firestorm 1.71 cache found 21,401 live entries.
  The first 100 entries yielded 88 decoded textures and 12 unavailable entries.
- Native real-model smoke test: all four synthetic textures decoded, and a query
  for red stripes ranked the generated red-striped texture first. DirectML startup
  failed on the test machine; isolated-process recovery completed inference on CPU.
- Browser: five parser/embedding tests and five headless end-to-end tests passed.
  End-to-end tests include real WebGPU SigLIP inference, correct synthetic ranking,
  partial results before indexing completion, index reuse after reload, themes,
  UUID filtering, details, directory-picker recovery, and narrow-screen layouts.
- Browser read-only sample from the real cache: 16 entries loaded, 13 visible
  previews decoded, no application error, **zero external requests** while browsing.
- Static site/container build passed. All model runtime modules are served with the
  correct MIME types; write methods and paths outside `/cachecat/` are denied.

Synthetic fixtures are committed; real cache samples, models and screenshots are
kept under ignored `.local/`. No real cache data is included in source or release
archives. macOS builds and native Linux graphical operation still require validation
through the build matrix and platform testing; successful compile/unit checks alone
do not establish all-platform readiness.

No production deployment or public release is implied by these local checks.

## Portable storage — 2026-11-08

All 14 tests passed on Windows and Linux, including executable-relative paths,
macOS bundle placement, import of committed WAL entries while the source database
remains open, and preservation of both the original database and existing portable
settings/data. Strict Clippy and the optimized Windows build passed. The packaged
executable found its adjacent model files and completed synthetic semantic search
when launched from another working directory without `--data-dir`. Archive inspection
confirmed no database, thumbnails, models or portable data were bundled. The user's
actual AppData index was not moved during testing; the first normal launch of the
updated build performs its non-destructive import in the background.

## Native library controls — 2026-11-08

The native UI now has theme/sort dropdowns, resizable panels with persisted widths,
one snapshot-derived count/progress area, title-bar cache location, readable UUID
copy controls and PNG/TGA export through a split button. Redundant sidebar navigation,
local-device labels and duplicated worker counts were removed.

All 12 tests passed on Windows and in the isolated Linux container. New coverage
checks export from a real synthetic cache, full-resolution RGBA preservation for
both formats, missing-asset errors, each sort order, unknown dimension placement,
legacy settings defaults and minimum window/panel sizing. Strict Clippy and the
Windows optimized release build passed. CI now runs all targets, including UI-state
tests. The virtual Linux display still produced blank captures, so dropdown/drag
interaction and final visual appearance have not been verified interactively. No
application was opened on the user's active desktop.

## Native indexing performance update — 2026-11-08

Read-only benchmark of a fixed private 100-entry snapshot from the supplied cache:
87 textures decoded and embedded, 13 incomplete entries skipped in every run.
Three fresh-process runs per build, optimized Windows executable, same pinned q8
SigLIP model and full-resolution JPEG 2000 decode. No saved embeddings were used.
Application/model files and OS disk caches were warm. These timings cover model
startup/query, reading, decoding, preprocessing, inference and ranking; they exclude
GUI drawing, thumbnail generation and SQLite writes.

| Build | Runs (seconds) | Median |
| --- | --- | --- |
| Original, two CPU threads | 68.684 / 73.057 / 65.621 | 68.684 s |
| Binary transport only, two CPU threads | 13.316 / 13.896 / 14.045 | 13.896 s |
| Final, four CPU threads and allocation reuse | 11.404 / 11.373 / 11.331 | 11.373 s |

The final sample is **6.0× faster**. Its middle run spent 0.460 s on model/query
startup, 7.433 s reading/decoding, and 3.433 s on preprocessing/inference. Whole-cache
throughput depends on texture dimensions, incomplete entries, disk speed and active
searches; this is not a promise of a fixed speedup for every cache.

The UI indexing worker also reuses up to 16 prepared tensors, avoids duplicate-content
inference, removes its per-item sleep, and scores each search candidate only once.
The same model ID and preprocessing are retained, so saved embeddings remain valid.
Seven Rust tests pass, including binary-frame fidelity/bounds and top-200 ranking
equivalence (including ties); strict Clippy and release build pass.

Reproduce with `python tools/benchmark-indexing.py --exe target/release/cachecat.exe
--cache PATH --data-dir PATH --count 100 --runs 3 --cpu`. Use an unchanged cache
snapshot and predownloaded model. The tool never writes to the cache or embedding DB.
