import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Tauri-friendly config:
// - fixed port 1420 (Tauri expects it by default)
// - strictPort so dev server fails fast instead of bumping port
// - clearScreen off so Vite output is preserved next to tauri logs
// - bind to IPv4 locally; Tauri's dev server probe can miss an IPv6-only localhost
const tauriDevHost = process.env.TAURI_DEV_HOST;
const host = tauriDevHost || '127.0.0.1';

export default defineConfig({
  plugins: [react()],
  // Cross-platform alias for shadcn's "@/lib/utils" etc. Build the URL
  // relative to this config, convert to a filesystem path, normalise
  // Windows backslashes to forward slashes (Vite's resolver compares
  // strings; mixed separators can stop a match), then use the array
  // regex form so the prefix anchor is explicit.
  resolve: {
    alias: [
      {
        find: /^@\//,
        replacement:
          fileURLToPath(new URL('./src/', import.meta.url)).replace(/\\/g, '/'),
      },
    ],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host,
    hmr: tauriDevHost ? { protocol: 'ws', host: tauriDevHost, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
});
