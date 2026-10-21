import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import Icons from "unplugin-icons/vite";
import { fileURLToPath, URL } from "url";

export default defineConfig({
  server: {
    // the launcher's dev server takes 3000
    port: 3001,
    strictPort: true,
  },
  plugins: [svelte(), Icons({ compiler: "svelte" }), tailwindcss()],
  resolve: {
    alias: {
      "$level-editor": fileURLToPath(new URL("./ui", import.meta.url)),
    },
  },
});
