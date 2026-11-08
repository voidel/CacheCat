# CacheCat

A better Second Life Cache Viewer. Lightweight and dependable with semantic image search.

Free and licensed under the GNU General Public License v3.0. Native Rust + GPUI
desktop application, plus a browser version that processes everything on the device.

## Features

- Virtualized texture grid, newest viewer last-used timestamp first.
- Exact or partial UUID filtering (8+ hexadecimal characters).
- Local semantic search for colours, shapes, materials, and objects using quantized
  SigLIP base/16 at 224 pixels. Results are ranked by cosine similarity, up to 200 matches.
- Progressive embeddings: progress bar, indexed/pending/unavailable counts, and immediate
  search over completed embeddings. Partial results are explicitly labelled.
- System-following light/dark themes with an override.
- Native theme and sort dropdowns; sort by recency, dimensions, UUID, or relevance.
- Resizable native sidebars, consolidated progress counts, and one-click UUID copying.
- Original-resolution PNG/TGA export with transparency through a download split button.
- Read-only viewer-cache access. Content-hashed thumbnails and model-versioned embeddings
  are stored separately. No uploads, telemetry, accounts, or server-side inference.
- Native inference is isolated in a helper process so driver crashes fall back to CPU.

Supported cache versions are **1.7** (8-byte header) and **1.71** (44-byte header),
with 28-byte index records and 600-byte JPEG 2000 header slots. Only live records
within the header's entry count are read; trailing allocated slots are ignored.
Incomplete/evicted textures may be unavailable. Rescan when the viewer changes the cache.

## Desktop

Requirements: current stable Rust, C/C++ build tools, CMake. Windows needs the MSVC
toolchain and Windows SDK. macOS needs Xcode command-line tools. Linux dependencies
(Ubuntu/Debian):

```sh
sudo apt install clang cmake pkg-config libasound2-dev libfontconfig1-dev libfreetype6-dev \
  libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libx11-dev libxcb1-dev \
  libxcomposite-dev libxrandr-dev libxi-dev libxcursor-dev libxinerama-dev libvulkan-dev libssl-dev
cargo run --release
```

Select a cache folder in the app, or pass it explicitly:

```powershell
cargo run --release -- --cache R:\FirestormCache
```

CacheCat is portable. By default, all persistent data lives in a `data/` folder
beside `cachecat.exe` (or the Linux executable). On macOS it is beside `CacheCat.app`,
outside the signed bundle. This location follows the executable, not the working directory.

```text
CacheCat/
  cachecat.exe
  data/
    index.sqlite3
    settings.json
    thumbnails/
    models/
```

On first launch with no portable database, CacheCat copies data from its former
user-profile location in the background. Existing embeddings, model files and
settings are retained; the old files remain untouched. SQLite's backup API includes
committed journal entries. Existing portable databases are never overwritten or
merged automatically. Move the whole application folder to keep your index with it;
when updating the executable, keep `data/`. Release archives never include local data.

`--data-dir PATH` overrides portable storage and disables automatic profile import.
Use a writable application folder, or specify a writable data directory.
`--cpu` forces CPU inference. Windows tries DirectML, macOS tries Core ML, and Linux
uses CPU by default. Linux NVIDIA acceleration is an optional `--features cuda` build
and requires matching CUDA/cuDNN libraries. The stock Linux release uses CPU inference.
GPU graph support varies; unsupported or failed sessions recover on CPU.

CPU indexing uses up to four inference threads by default, leaving capacity for
other applications. Existing embeddings remain reusable after performance updates.
For controlled measurements, `tools/benchmark-indexing.py` runs the native pipeline
without opening a window or using saved embeddings. `CACHECAT_CPU_THREADS` (1–16)
can override the default thread count; more threads are not always faster.

Click **Enable semantic search** to fetch the pinned model (about 213 MB).
Once downloaded, the native application works offline. The files are verified by SHA-256.
The UI remains usable during download and indexing. Pause stops indexing between items.

## Browser and feature page

`web/` contains a static feature page at `/cachecat/` and the viewer at `/cachecat/app/`.
The page includes GitHub release links for all platforms. Model downloads come from
the pinned public Hugging Face repository; no local files are sent there.

```sh
cd web
corepack enable
pnpm install --frozen-lockfile
pnpm test
pnpm build
```

Browser requirements: a secure context (HTTPS or localhost), WebAssembly, workers,
and OffscreenCanvas. Chrome/Edge support the read-only directory picker. Other recent
browsers use folder selection; selecting the `texturecache` subfolder avoids enumerating
unrelated viewer files. GPU inference uses WebGPU where the model is supported, with
WebAssembly CPU fallback. Browser storage persists thumbnails/embeddings/model assets
subject to browser quotas and eviction. Keep the tab open while indexing. Clearing site
data removes the local index. Network requests download public application/model assets;
the viewer has no upload API and makes no analytics requests.

Container preview (no host cache mounts):

```sh
docker build -t cachecat .
docker run --rm -p 127.0.0.1:5188:8080 --read-only --tmpfs /tmp --cap-drop ALL cachecat
```

Open `http://127.0.0.1:5188/cachecat/`. Use the file picker to grant access to the local
cache. See `deploy/nginx.conf` for namespace restrictions, CSP, and read-only HTTP methods.

## Verification

```sh
cargo fmt --check
cargo test --lib
cargo clippy --all-targets -- -D warnings
cargo run -- --cache web/tests/fixtures/texturecache --inspect 4
pnpm --dir web test
pnpm --dir web exec playwright test
```

Browser tests use synthetic fixtures and a running preview container. Install the
Playwright Chromium browser on Linux CI; Windows tests use installed Edge. Set
`CACHECAT_MODEL_TEST=1` for the optional real-model browser ranking test (213 MB download).
`tools/make-fixtures.py` regenerates the synthetic JPEG 2000 fixture with Pillow.

Native real-model smoke test:

```sh
cargo run -- --data-dir .local/data --download-model
cargo run -- --cache web/tests/fixtures/texturecache --data-dir .local/data --inspect 4 --query "red stripes"
```

## Releases

GitHub Actions tests and builds Windows x64, macOS Apple silicon/Intel, Linux x64,
and the static browser app. Push a `v*` tag to assemble platform archives and SHA-256
checksums into a **draft** GitHub release. Review artifacts before publishing.
No model files or user caches are included. No production deployment is performed by CI.

App code is licensed under the GNU General Public License v3.0. See [LICENSE](LICENSE)
and [THIRD_PARTY.md](THIRD_PARTY.md) for the license text and dependency/model
licensing and cache-format references. Cached assets remain their creators' property.
