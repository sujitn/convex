import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// convex-server serves the build at /desk from desk_ui_dir. In development the Vite
// server proxies the API and the stream, so the page still calls its own origin.
export default defineConfig({
  base: '/desk/',
  plugins: [react()],
  server: {
    port: 3200,
    proxy: { '/api': { target: process.env.CONVEX_API_URL ?? 'http://127.0.0.1:8080', ws: true } },
  },
  build: { outDir: 'dist', sourcemap: false },
  test: { include: ['src/**/*.test.ts'] },
})
