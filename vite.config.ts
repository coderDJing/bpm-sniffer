import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  base: './',
  plugins: [react()],
  build: {
    sourcemap: false,
    outDir: 'src-tauri/dist'
  },
  server: {
    port: 5173,
    strictPort: true
  }
})
