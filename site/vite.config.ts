import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

// servido em https://milkway.github.io/helm/
export default defineConfig({
  base: "/helm/",
  plugins: [tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
