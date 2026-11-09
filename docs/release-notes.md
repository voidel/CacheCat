Faster local indexing with batched SigLIP inference.

- Process up to eight images per GPU batch or four per CPU batch. The choices remain **GPU** and **CPU**; there is no combined mode.
- Prepare embeddings from small decoded images instead of fully decoding large textures. Exports and sculpt previews still use the original resolution.
- Retry smaller batches when a device runs out of resources, and preserve successful items if an individual image fails.
- Keep search, pause and device changes responsive between image-preparation steps.

On a local 96-texture comparison, indexing improved from approximately 19 to 60–64 images/sec. Results depend on hardware and cache contents. Smaller decoded inputs and quantized batches can produce slightly different search rankings.

Download the archive for your platform and extract it before launching. Keep your existing portable `data` folder when upgrading; saved embeddings remain usable and the model does not need to be downloaded again. SHA-256 checksums are included.

macOS bundles are ad-hoc signed, not Apple notarized. Linux requires the [documented system libraries](https://github.com/voidel/CacheCat/blob/v0.1.1/docs/development.md#desktop); Windows requires the Visual C++ runtime.

Only export or reuse assets where you have the necessary rights or permission, and comply with Linden Lab's terms. See [Responsible use](https://github.com/voidel/CacheCat/blob/v0.1.1/README.md#responsible-use).
