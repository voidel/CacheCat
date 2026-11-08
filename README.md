# CacheCat

A better Second Life Cache Viewer. Lightweight and dependable with semantic image search.

A portable Rust + GPUI desktop app for Windows, macOS and Linux.
Textures and searches stay on your device.

## Features

- Browse cached textures and filter by UUID.
- Search colours, shapes and materials with local SigLIP indexing.
- Search while indexing continues, with progress and partial results shown.
- Sort textures, copy UUIDs, and export PNG or TGA.
- Light and dark themes, with system preference support.

## Getting started

1. Download an available build from [GitHub Releases](https://github.com/voidel/CacheCat/releases), or build from source below.
2. Open the cache folder shown in your viewer’s settings, or its `texturecache` subfolder.
3. Search in **Semantic** mode and download the model when prompted (about 213 MB), or choose **UUID** to filter without it.

CacheCat reads your cache without changing it. Incomplete cache entries may be unavailable.

The desktop app is portable: its index, models and settings live in `data/`
beside the executable (beside `CacheCat.app` on macOS). Keep this folder when moving or
updating the app.

Thumbnails are decoded on demand for visible items and kept only in memory.

## Responsible use

Do not use CacheCat in violation of [Linden Lab’s Terms of Service](https://lindenlab.com/tos),
the [Second Life Terms and Conditions](https://lindenlab.com/legal/second-life-terms-and-conditions),
or applicable law.

Only download, export, copy or reuse assets where you hold the necessary rights or
have the rights holder’s permission for that use. An asset appearing in your
viewer’s cache does not give you ownership or permission to use it. Purchasing an
in-world item does not automatically grant permission to export its assets.

You are solely responsible for verifying permissions and for the assets you
download, export or use with CacheCat. CacheCat’s authors and contributors do not
authorise or endorse infringement and, to the extent permitted by applicable law,
disclaim liability for misuse of the software.

## Build

With Rust and the [platform prerequisites](docs/development.md#desktop) installed:

```sh
cargo run --release
```

See the [development guide](docs/development.md) for tests and releases.

## License

[GNU General Public License v3.0](LICENSE). Dependencies retain their
[respective licenses](THIRD_PARTY.md). Cached textures belong to their creators.
