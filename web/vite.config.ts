import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// `npm run dev` proxies the API to a running `modman ui --no-open --port 7427`.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: './',
  build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 900 },
  server: { proxy: { '/api': { target: 'http://127.0.0.1:7427', headers: { Host: '127.0.0.1:7427' } } } },
})
