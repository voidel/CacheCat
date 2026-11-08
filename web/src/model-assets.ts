export type SearchMode = 'semantic' | 'uuid';
export const REPO = 'Xenova/siglip-base-patch16-224';
export const REVISION = '4649052661e53c7000355844105f8a1792088239';
export const MODEL_SOURCE = `https://huggingface.co/${REPO}/tree/${REVISION}`;
export const MODEL_FILES = {
  'onnx/vision_model_quantized.onnx': 99_499_129,
  'onnx/text_model_quantized.onnx': 111_475_220,
  'tokenizer.json': 2_398_744,
};

export async function hasCachedModel(): Promise<boolean> {
  try {
    const cache = await caches.open('transformers-cache');
    const files = [...Object.keys(MODEL_FILES), 'config.json', 'tokenizer_config.json', 'preprocessor_config.json'];
    return (await Promise.all(files.map(file => cache.match(`https://huggingface.co/${REPO}/resolve/${REVISION}/${file}`)))).every(Boolean);
  } catch { return false; }
}
