import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// Tauri 前端：产物出 dist/，由 tauri.conf.json 的 frontendDist 指向。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: 'dist',
    target: 'chrome105',
  },
});
