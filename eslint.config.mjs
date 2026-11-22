import js from "@eslint/js";
import ts from "typescript-eslint";
import prettier from "eslint-config-prettier";

export default ts.config(
  { ignores: ["**/dist/**", "**/dist-tauri/**", "**/node_modules/**", "assets/**", "output/**", "benchmarks/**"] },
  js.configs.recommended,
  ...ts.configs.recommended,
  prettier,
);
