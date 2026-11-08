CacheCat is a local-first Second Life / Firestorm texture-cache explorer.

- Native Rust/GPUI viewer for Windows, macOS, and Linux.
- Newest-first texture grid, UUID filtering, description search, and system-aware themes.
- Embedding progress with immediate search over completed items and partial-result notices.
- Isolated native inference process with CPU recovery when an accelerator fails.

Choose the matching platform archive, extract it, and run CacheCat. macOS archives
contain an app bundle (ad-hoc signed, not Apple notarized). Linux requires X11 or
Wayland and the system graphics/font libraries listed in the
[development guide](https://github.com/voidel/CacheCat/blob/main/docs/development.md#desktop). Windows needs
a current supported Windows installation with the Visual C++ runtime.

This is a portable application. Its `data` folder lives beside the executable
(beside the `.app` bundle on macOS). Keep that folder when upgrading or moving the
application. First launch copies an existing profile-based index into portable
storage without deleting the original. Explicit `--data-dir` overrides skip import.

Model downloads are approximately 213 MB. Cache contents stay local. Viewer caches
can contain incomplete files, which are reported separately. GPU availability and
operator support vary; CPU fallback is always provided. Search is available while indexing continues, with a notice when results are partial.

Maintainer: publish this draft only after reviewing platform smoke tests and artifacts.
