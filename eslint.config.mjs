import js from "@eslint/js";
import ts from "typescript-eslint";
import prettier from "eslint-config-prettier";

export default ts.config(
  { ignores: ["**/dist/**", "**/dist-tauri/**", "**/node_modules/**", "assets/**", "output/**", "benchmarks/**"] },
  js.configs.recommended,
  ...ts.configs.recommended,
  prettier,
  { files: ["packages/ui/src/**/*.{ts,tsx}", "apps/ui-workshop/src/**/*.{ts,tsx}"], languageOptions: { globals: { document: "readonly", window: "readonly", fetch: "readonly", HTMLElement: "readonly", HTMLDivElement: "readonly", HTMLInputElement: "readonly", React: "readonly", console: "readonly" } } },
);
