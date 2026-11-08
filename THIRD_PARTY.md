# Third-party software and models

CacheCat's own code is licensed under the GNU General Public License v3.0.
Dependencies retain their respective licenses.

- GPUI and GPUI Component: Apache-2.0.
- ONNX Runtime: MIT; `ort`: MIT OR Apache-2.0.
- JPEG 2000: `jpeg2k` (MIT OR Apache-2.0), OpenJPEG (BSD-2-Clause).
- Hugging Face Tokenizers: Apache-2.0.
- SQLite: public domain; `rusqlite`: MIT.
- Windows acceleration uses Microsoft's separately licensed DirectML runtime,
  redistributed by the ONNX Runtime dependency. The portable CPU path does not
  require GPU acceleration. See the runtime's bundled notices/license terms.

## Model

Model: [Google SigLIP base, patch16, 224](https://huggingface.co/google/siglip-base-patch16-224).
ONNX conversion: [Xenova/siglip-base-patch16-224](https://huggingface.co/Xenova/siglip-base-patch16-224),
revision `4649052661e53c7000355844105f8a1792088239`.
The model repository declares Apache-2.0. CacheCat downloads the quantized image
and text encoders plus tokenizer, with fixed sizes and SHA-256 hashes.
Model weights are downloaded separately and are not committed to this repository.

## Cache format

The reader is an independent Rust implementation of the documented binary layout,
informed by Firestorm's `indra/newview/lltexturecache.h` and `.cpp`:
https://github.com/FirestormViewer/phoenix-firestorm/tree/master/indra/newview

No viewer source code is included. Second Life and Firestorm are trademarks of
their respective owners. CacheCat is an independent project. Cached textures
remain the intellectual property of their creators.
