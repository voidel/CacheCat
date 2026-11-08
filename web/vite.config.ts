import { defineConfig } from 'vite';
export default defineConfig({
  base: '/cachecat/',
  worker: { format: 'es' },
  build: { target: 'es2022', rollupOptions: { input: { landing: 'index.html', app: 'app/index.html' } } },
});
