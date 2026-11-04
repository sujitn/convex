import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// convex-server serves the build at /admin from admin_ui_dir. In development the
// Vite server proxies the API, so the page still calls its own origin.
export default defineConfig({
  base: '/admin/',
  plugins: [react()],
  server: {
    port: 3100,
    proxy: { '/api': process.env.CONVEX_API_URL ?? 'http://127.0.0.1:8080' },
  },
  build: { outDir: 'dist', sourcemap: false },
  test: { include: ['src/**/*.test.ts'] },
})
