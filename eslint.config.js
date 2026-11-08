import babel from "@babel/eslint-parser";
import reactHooks from "eslint-plugin-react-hooks";

// Objectif : la justesse des hooks, pas la police de style. Analyse TS via Babel :
// typescript-eslint refuse TS 7 (voir https://github.com/typescript-eslint/typescript-eslint/issues/10940).
export default [
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: babel,
      parserOptions: {
        requireConfigFile: false,
        babelOptions: { presets: ["@babel/preset-typescript", "@babel/preset-react"] },
      },
    },
    plugins: { "react-hooks": reactHooks },
    linterOptions: { reportUnusedDisableDirectives: "error" },
    rules: { "react-hooks/rules-of-hooks": "error", "react-hooks/exhaustive-deps": "error" },
  },
];
