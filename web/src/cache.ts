export interface Texture { id: string; slot: number; imageSize: number; bodySize: number; timestamp: number }
export interface Item extends Texture { hash?: string; width?: number; height?: number; indexed?: boolean; error?: string }
export function parseEntries(buffer: ArrayBuffer): Texture[] {
  const view = new DataView(buffer);
  if (view.byteLength < 8) throw new Error('Truncated texture index.');
  const version = view.getFloat32(0, true);
  const header = version === Math.fround(1.7) ? 8 : version === Math.fround(1.71) ? 44 : 0;
  if (!header) throw new Error(`Unsupported cache version ${version.toFixed(3)}.`);
  if (view.byteLength < header) throw new Error('Truncated cache header.');
  if (header === 44 && ![32, 64].includes(view.getUint32(4, true))) throw new Error('Invalid cache address size.');
  const count = view.getUint32(header - 4, true);
  if (count > 4_000_000 || view.byteLength < header + count * 28) throw new Error('The cache index is incomplete. Try rescanning after the viewer finishes writing.');
  const items: Texture[] = [];
  for (let slot = 0; slot < count; slot++) {
    const offset = header + slot * 28;
    const hex = Array.from(new Uint8Array(buffer, offset, 16), x => x.toString(16).padStart(2, '0')).join('');
    const id = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
    const imageSize = view.getInt32(offset + 16, true), bodySize = view.getInt32(offset + 20, true);
    if (/^0+$/.test(hex) || imageSize <= 0 || bodySize < 0 || imageSize > 64 * 1024 * 1024 || bodySize > Math.max(0, imageSize - 600)) continue;
    items.push({ id, slot, imageSize, bodySize, timestamp: view.getUint32(offset + 24, true) });
  }
  items.sort((a, b) => b.timestamp - a.timestamp || a.id.localeCompare(b.id));
  const seen = new Set<string>();
  return items.filter(item => !seen.has(item.id) && !!seen.add(item.id));
}
export function uuidQuery(query: string): string | null {
  const compact = query.trim().toLowerCase().replaceAll('-', '');
  return /^[0-9a-f]{8,32}$/.test(compact) ? compact : null;
}
export function normalize(values: ArrayLike<number>): Float32Array {
  if (values.length !== 768) throw new Error('Unexpected embedding dimension.');
  const vector = Float32Array.from(values);
  const norm = Math.sqrt(vector.reduce((sum, v) => sum + v * v, 0));
  if (!Number.isFinite(norm) || norm < 1e-12) throw new Error('Invalid embedding.');
  return vector.map(x => x / norm);
}
export function similarity(a: Float32Array, b: Float32Array): number {
  if (a.length !== b.length) return -Infinity;
  return a.reduce((sum, x, i) => sum + x * b[i], 0);
}
export function validateJ2k(bytes: Uint8Array): void {
  if (bytes.length < 42 || bytes[0] !== 255 || bytes[1] !== 79 || bytes[2] !== 255 || bytes[3] !== 81) throw new Error('Not a complete JPEG 2000 header.');
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const x = v.getUint32(8), y = v.getUint32(12), ox = v.getUint32(16), oy = v.getUint32(20);
  if (x <= ox || y <= oy || x > 8192 || y > 8192 || (x - ox) * (y - oy) > 16_777_216) throw new Error('Oversized or invalid texture dimensions.');
}
