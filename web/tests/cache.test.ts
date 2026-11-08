import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseEntries, uuidQuery, normalize, similarity, validateJ2k } from '../src/cache.ts';
function fixture(version: number) {
  const header = version === 1.71 ? 44 : 8;
  const buffer = new ArrayBuffer(header + 3 * 28), view = new DataView(buffer);
  view.setFloat32(0, version, true);
  if (header === 44) view.setUint32(4, 64, true);
  view.setUint32(header - 4, 3, true);
  for (let i = 0; i < 3; i++) { const offset = header + i * 28; view.setUint8(offset + 15, i); view.setInt32(offset + 16, 700, true); view.setInt32(offset + 20, 100, true); view.setUint32(offset + 24, i * 5, true); }
  return buffer;
}
test('both cache headers preserve slots, skip nil entries, and order newest first', () => {
  for (const version of [1.7, 1.71]) { const items = parseEntries(fixture(version)); assert.equal(items.length, 2); assert.equal(items[0].slot, 2); assert.equal(items[1].slot, 1); }
});
test('truncated, unsupported and invalid headers fail explicitly', () => {
  assert.throws(() => parseEntries(new ArrayBuffer(7)));
  assert.throws(() => parseEntries(fixture(9)));
  assert.throws(() => parseEntries(fixture(1.71).slice(0, 45)));
});
test('UUID fragments do not swallow ordinary colour and object queries', () => {
  assert.equal(uuidQuery('DEADBEEF-1234'), 'deadbeef1234'); assert.equal(uuidQuery('bed'), null); assert.equal(uuidQuery('red silk'), null);
});
test('embedding normalization produces cosine ranking', () => {
  const a = normalize(Array(768).fill(1)); assert.ok(similarity(a, a) > .999); assert.throws(() => normalize(Array(768).fill(NaN)));
});
test('malformed textures cannot request unbounded decoder allocations', () => { assert.throws(() => validateJ2k(new Uint8Array(50))); });
