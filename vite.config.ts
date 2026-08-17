import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// CodexDesk serves two webview entry points: the main dashboard and the tray popup.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: "index.html",
        tray: "tray.html",
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
