import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import security from "eslint-plugin-security";

export default tseslint.config(
  { ignores: ["dist", "node_modules"] },
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  security.configs.recommended,
  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "no-eval": "error",
      "no-implied-eval": "error",
      "no-new-func": "error",
      "no-restricted-properties": [
        "error",
        { property: "innerHTML", message: "Render text with React. No raw HTML." },
        { property: "outerHTML", message: "Render text with React. No raw HTML." },
        { property: "insertAdjacentHTML", message: "Render text with React. No raw HTML." },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector: "JSXAttribute[name.name='dangerouslySetInnerHTML']",
          message: "No raw HTML. Imported cards and model output are untrusted.",
        },
      ],
    },
  },
  { files: ["eslint.config.js"], ...tseslint.configs.disableTypeChecked },
);
