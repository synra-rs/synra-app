import js from '@eslint/js'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  // The config files are not part of the type-checked project, and a lint rule is not what keeps them correct.
  {
    ignores: [
      'dist/**',
      'node_modules/**',
      'src-tauri/**',
      'eslint.config.js',
      'vite.config.ts',
      'vitest.config.ts',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      // A store's job is to hold state, and `undefined` is a state a device list can be in before its first snapshot.
      '@typescript-eslint/no-unnecessary-condition': 'off',
      // The dispatcher's handlers are typed by their discriminant, and one cast at the registration boundary is what
      // buys one map instead of seventeen fields.
      '@typescript-eslint/consistent-type-assertions': 'off',
      // ★ A leading underscore is the convention for "destructured to drop it": the dispatcher strips an envelope's
      // `seq` and `at` so a handler cannot make a second judgement about the sequence, and a store clears a device's
      // transfer by removing the key. Both bind a name they deliberately do not use, and the rule that forbids that
      // would push them toward a `delete` or a second object literal — which is the bug, not the fix.
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      // A handler registered for one event kind is a subtype of the general handler, and the assertion is what tells
      // the compiler the map is safe. Without it every registration needs its own field.
      '@typescript-eslint/no-unnecessary-type-assertion': 'off',
    },
  },
)