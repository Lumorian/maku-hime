import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";

// 前端与构建脚本的静态检查。不做格式化（交给编辑器），只抓真问题。
export default tseslint.config(
  {
    ignores: [
      "dist/**",
      "dist-packages/**",
      "node_modules/**",
      "src-tauri/**",
      "target/**",
      ".cache/**",
      ".research/**",
      ".tmp/**",
      "coverage/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: { "react-hooks": reactHooks },
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-hooks/exhaustive-deps": "warn",
      // 这条规则针对 React Compiler 的「纯」组件；本项目的滚动同步、配置镜像、
      // 防抖计时器都属于合理的副作用写法，降级为提示而不是错误。
      "react-hooks/set-state-in-effect": "warn",
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_" }],
    },
  },
  {
    files: ["scripts/**/*.mjs", "vite.config.ts", "eslint.config.js"],
    languageOptions: { globals: { ...globals.node } },
  },
);
