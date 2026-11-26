import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite options tailored for Tauri development.
export default defineConfig({
  plugins: [react()],
  // Keep Rust compiler errors visible in the terminal.
  clearScreen: false,
  server: {
    // Tauri expects a fixed port; fail instead of picking another one.
    port: 1420,
    strictPort: true,
    // Only listen on the local machine.
    host: "localhost",
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
