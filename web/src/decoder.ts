import OpenJPEG from '@cornerstonejs/codec-openjpeg/decodewasmjs';
import wasmUrl from '@cornerstonejs/codec-openjpeg/decodewasm?url';
import { validateJ2k } from './cache';
let module: Promise<any> | undefined;
export async function decode(bytes: Uint8Array): Promise<{ rgba: Uint8ClampedArray; width: number; height: number }> {
  validateJ2k(bytes);
  const codec = await (module ??= OpenJPEG({ locateFile: () => wasmUrl }));
  const decoder = new codec.J2KDecoder();
  try {
    decoder.getEncodedBuffer(bytes.length).set(bytes);
    decoder.decode();
    const frame = decoder.getFrameInfo();
    const { width, height, componentCount, bitsPerSample } = frame;
    if (!width || !height || width * height > 16_777_216 || ![1, 2, 3, 4].includes(componentCount) || bitsPerSample > 8) throw new Error('Unsupported texture pixel format.');
    const pixels = decoder.getDecodedBuffer();
    if (pixels.length < width * height * componentCount) throw new Error('Incomplete texture pixels.');
    const rgba = new Uint8ClampedArray(width * height * 4);
    for (let i = 0; i < width * height; i++) {
      const p = i * componentCount, o = i * 4;
      rgba[o] = pixels[p]; rgba[o + 1] = pixels[p + (componentCount > 2 ? 1 : 0)]; rgba[o + 2] = pixels[p + (componentCount > 2 ? 2 : 0)];
      rgba[o + 3] = componentCount === 2 ? pixels[p + 1] : componentCount === 4 ? pixels[p + 3] : 255;
    }
    return { rgba, width, height };
  } finally { decoder.delete(); }
}
export function canvasFor(image: { rgba: Uint8ClampedArray; width: number; height: number }): OffscreenCanvas {
  const canvas = new OffscreenCanvas(image.width, image.height);
  canvas.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(image.rgba), image.width, image.height), 0, 0);
  return canvas;
}
export async function thumbnail(image: { rgba: Uint8ClampedArray; width: number; height: number }): Promise<Blob> {
  const scale = Math.min(1, 256 / Math.max(image.width, image.height));
  const canvas = new OffscreenCanvas(Math.max(1, Math.round(image.width * scale)), Math.max(1, Math.round(image.height * scale)));
  canvas.getContext('2d')!.drawImage(canvasFor(image), 0, 0, canvas.width, canvas.height);
  return canvas.convertToBlob({ type: 'image/png' });
}
