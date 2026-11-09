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

Use the cache location shown in your viewer's settings.

## Checks

```sh
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo run -- --cache tests/fixtures/texturecache --inspect 4
```

`tools/make-fixtures.py` regenerates the synthetic JPEG 2000 fixture with Pillow.

Test semantic search with the synthetic textures:

```sh
cargo run -- --data-dir .local/data --download-model
cargo run -- --cache tests/fixtures/texturecache --data-dir .local/data --inspect 4 --query "red stripes"
```

## Releases

GitHub Actions tests and builds Windows x64, macOS Apple silicon/Intel and Linux x64.
Push a `v*` tag to assemble platform archives and SHA-256
checksums into a **draft** GitHub release. Review artifacts before publishing.
No model files or user caches are included.

`Cargo.toml` is the version source for the window title, Windows file properties,
packaged executables and archive names. Bump the matching `cachecat` entry in
`Cargo.lock` with it. For example, version 0.1.2 produces
`cachecat-0.1.2-windows-x86_64.zip` containing `cachecat-0.1.2.exe`.
Native Cargo build outputs keep Cargo's standard `cachecat` name.
CI runs `tools/verify-package.py` against the extracted archive and rejects a
release tag that differs from the Cargo version. Launchers and macOS bundle
metadata point to the versioned executable.

Archives contain the application, required runtime libraries, platform launchers
and licence notices. The README and developer documentation remain in this repository.
