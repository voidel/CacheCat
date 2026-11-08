import { AutoTokenizer, AutoProcessor, SiglipTextModel, SiglipVisionModel, RawImage, env } from '@huggingface/transformers';
import { normalize } from './cache';
const REPO = 'Xenova/siglip-base-patch16-224';
const REVISION = '4649052661e53c7000355844105f8a1792088239';
env.allowLocalModels = false;
env.useBrowserCache = true;
env.backends.onnx.wasm!.numThreads = 1;
env.backends.onnx.wasm!.wasmPaths = `${self.location.origin}${import.meta.env.BASE_URL}runtime/`;
export class Inference {
  private tokenizer: any;
  private processor: any;
  private vision: any;
  private textModel: any;
  backend = 'CPU';
  private async sessions(device: 'webgpu' | 'wasm', progress: (message: string) => void) {
    const options = { revision: REVISION, dtype: 'q8' as const, device, progress_callback: (p: any) => { if (p.status === 'progress') progress(`Downloading ${p.file}: ${Math.round(p.progress)}%`); } };
    this.vision = await SiglipVisionModel.from_pretrained(REPO, options);
    try { this.textModel = await SiglipTextModel.from_pretrained(REPO, options); }
    catch (error) { await this.vision.dispose(); this.vision = undefined; throw error; }
    this.backend = device === 'webgpu' ? 'WebGPU' : 'CPU · WebAssembly';
  }
  async load(progress: (message: string) => void): Promise<void> {
    progress('Loading local SigLIP…');
    this.tokenizer = await AutoTokenizer.from_pretrained(REPO, { revision: REVISION });
    this.processor = await AutoProcessor.from_pretrained(REPO, { revision: REVISION });
    try {
      if (!('gpu' in navigator)) throw new Error('WebGPU unavailable');
      await this.sessions('webgpu', progress);
      // Exercise both graphs before advertising acceleration.
      await this.query('texture');
      await this.embed(new Uint8ClampedArray([128, 128, 128, 255]), 1, 1);
    } catch {
      await this.vision?.dispose(); await this.textModel?.dispose();
      this.vision = undefined; this.textModel = undefined;
      progress('Using CPU fallback…');
      await this.sessions('wasm', progress);
    }
  }
  private async fallback() {
    await this.vision?.dispose(); await this.textModel?.dispose();
    await this.sessions('wasm', () => {});
  }
  async query(query: string): Promise<Float32Array> {
    const inputs = this.tokenizer(`This is a photo of ${query.trim().replace(/\s+/g, ' ').toLowerCase()}.`, { padding: 'max_length', truncation: true, max_length: 64 });
    try { return normalize((await this.textModel(inputs)).pooler_output.data); }
    catch (error) { if (this.backend !== 'WebGPU') throw error; await this.fallback(); return normalize((await this.textModel(inputs)).pooler_output.data); }
  }
  async embed(rgba: Uint8ClampedArray, width: number, height: number): Promise<Float32Array> {
    const rgb = new Uint8Array(width * height * 3);
    for (let i = 0; i < width * height; i++) for (let c = 0; c < 3; c++) rgb[i * 3 + c] = Math.round(rgba[i * 4 + c] * rgba[i * 4 + 3] / 255 + 127.5 * (1 - rgba[i * 4 + 3] / 255));
    const inputs = await this.processor(new RawImage(rgb, width, height, 3));
    try { return normalize((await this.vision(inputs)).pooler_output.data); }
    catch (error) { if (this.backend !== 'WebGPU') throw error; await this.fallback(); return normalize((await this.vision(inputs)).pooler_output.data); }
  }
}
