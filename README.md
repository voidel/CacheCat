# CacheCat

A better Second Life Cache Viewer. Lightweight and dependable with semantic image search.

A Rust + GPUI desktop app for Windows, macOS and Linux, with a browser version.
Textures and searches stay on your device.

## Features

- Browse cached textures and filter by UUID.
- Search colours, shapes and materials with local SigLIP indexing.
- Search while indexing continues, with progress and partial results shown.
- Sort textures, copy UUIDs, and export PNG or TGA on desktop.
- Light and dark themes, with system preference support.

## Getting started

1. Download an available build from [GitHub Releases](https://github.com/voidel/CacheCat/releases), or build from source below.
2. Open the cache folder shown in your viewer’s settings, or its `texturecache` subfolder.
3. Search in **Semantic** mode and download the model when prompted (about 213 MB), or choose **UUID** to filter without it.

CacheCat reads your cache without changing it. Incomplete cache entries may be unavailable.

The desktop app is portable: its index, models, thumbnails and settings live in `data/`
beside the executable (beside `CacheCat.app` on macOS). Keep this folder when moving or
updating the app. The browser version stores its data in browser storage.

## Build

With Rust and the [platform prerequisites](docs/development.md#desktop) installed:

```sh
cargo run --release
```

See the [development guide](docs/development.md) for browser setup, tests and releases,
and [architecture notes](docs/architecture.md) for technical details.

## License

[GNU General Public License v3.0](LICENSE). Dependencies retain their
[respective licenses](THIRD_PARTY.md). Cached textures belong to their creators.
