# Development

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

`--cache PATH` selects a viewer cache. `--data-dir PATH` overrides portable storage; `--cpu` forces CPU inference. Linux CUDA builds use `--features cuda` and require matching CUDA/cuDNN libraries.

Use the cache location shown in the viewer's settings. It varies by platform and
can be changed by the user; examples should not assume a particular drive or folder.

## Browser

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

## Checks

```sh
cargo fmt --check
cargo test --all-targets
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
