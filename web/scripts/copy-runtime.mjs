import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { mkdir, readdir, copyFile } from 'node:fs/promises';
const require = createRequire(import.meta.url);
const fromTransformers = createRequire(require.resolve('@huggingface/transformers'));
const runtime = dirname(fromTransformers.resolve('onnxruntime-web'));
for (const output of ['dist/runtime', 'public/runtime']) {
  await mkdir(output, { recursive: true });
  for (const name of await readdir(runtime)) if (/\.wasm$|ort-wasm.*\.mjs$/.test(name)) await copyFile(join(runtime, name), join(output, name));
}
