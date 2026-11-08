export const MODEL_KEY = 'siglip-base-224-q8-4649052-web-v1';
export interface RecordData { hash: string; thumbnail: Blob; width: number; height: number; vector?: Float32Array; model?: string }
let database: Promise<IDBDatabase> | undefined;
function db(): Promise<IDBDatabase> {
  return database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open('cachecat-local', 1);
    request.onupgradeneeded = () => request.result.createObjectStore('textures', { keyPath: 'hash' });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
export async function getRecord(hash: string): Promise<RecordData | undefined> {
  const database = await db();
  return new Promise((resolve, reject) => {
    const request = database.transaction('textures').objectStore('textures').get(hash);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
export async function putRecord(record: RecordData): Promise<void> {
  const database = await db();
  return new Promise((resolve, reject) => {
    const transaction = database.transaction('textures', 'readwrite');
    transaction.objectStore('textures').put(record);
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error ?? new Error('Local storage write failed.'));
  });
}
