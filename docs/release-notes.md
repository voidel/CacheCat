CacheCat now shows its version wherever you need to identify a build.

- The desktop window title includes **v0.1.2**.
- Release executables and archive filenames include **0.1.2**, for example `cachecat-0.1.2.exe` inside `cachecat-0.1.2-windows-x86_64.zip`.
- Windows file properties include the file and product version. macOS bundle metadata and platform launchers use the same version.
- Release checks now launch the packaged application, verify its version and read the synthetic cache before creating a draft release.

This retains v0.1.1's faster batched indexing and existing GPU/CPU choices.

Extract the archive before launching. When upgrading, keep your existing portable `data` folder beside the new executable (beside `CacheCat.app` on macOS). Saved embeddings and downloaded models remain usable. SHA-256 checksums are included.

macOS bundles are ad-hoc signed, not Apple notarized. Linux requires the [documented system libraries](https://github.com/voidel/CacheCat/blob/v0.1.2/docs/development.md#desktop); Windows requires the Visual C++ runtime.

Only export or reuse assets where you have the necessary rights or permission, and comply with Linden Lab's terms. See [Responsible use](https://github.com/voidel/CacheCat/blob/v0.1.2/README.md#responsible-use).
