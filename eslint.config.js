import js from '@eslint/js';
import ts from '@typescript-eslint/eslint-plugin';
import tsParser from '@typescript-eslint/parser';
import hooks from 'eslint-plugin-react-hooks';

export default [
  { ignores: ['dist/**', 'src-tauri/**', 'docs/.vitepress/cache/**', 'docs/.vitepress/dist/**'] },
  js.configs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: { ecmaVersion: 'latest', sourceType: 'module', ecmaFeatures: { jsx: true } },
      globals: {
        window: 'readonly', document: 'readonly', console: 'readonly',
        setTimeout: 'readonly', clearTimeout: 'readonly', ResizeObserver: 'readonly',
        HTMLDivElement: 'readonly', HTMLCanvasElement: 'readonly',
      },
    },
    plugins: { '@typescript-eslint': ts, 'react-hooks': hooks },
    rules: {
      ...ts.configs.recommended.rules,
      ...hooks.configs.recommended.rules,
      'no-undef': 'off', // TypeScript handles this better than eslint can
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-explicit-any': 'error',
      'no-console': ['warn', { allow: ['warn', 'error'] }],
    },
  },
];
