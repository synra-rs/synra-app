import { antfu } from '@antfu/eslint-config'

export default antfu({
  // ★ Two build outputs, both ignored, both for the same reason.
  //
  // `dist` is the Tauri shell's `frontendDist` and `src-tauri/gen` is what `tauri-build` generates on every compile of
  // the shell — so `src-tauri/gen` appears the moment anyone builds the shell, and `dist` the moment anyone builds the
  // interface. Without these, `pnpm run build` followed by `pnpm run lint` fails with thousands of errors in generated
  // files, which reads as "the build produced bad code" and is only "lint is reading a build artefact". The ordering
  // hides it: a fresh checkout lints clean and the failure lands on whoever happened to build first.
  ignores: ['dist', 'src-tauri/target', 'src-tauri/gen'],

  rules: {
    'style/comma-dangle': ['warn', 'never']
  },

  // ★ `strict: true` is what turns on `unocss/blocklist`, and the blocklist it reads is the one `uno.config.ts`
  // spreads in from `@synra/theme` — so "which utilities are forbidden" has one answer, shared with the build
  // ([28 §13.1](./28-app-ui.md) guard 1).
  //
  // `attributify: false` because this interface does not use attributify mode (R5): the rule would only be
  // available for something that does not exist.
  unocss: {
    attributify: false,
    strict: true
  },

  // ★ **TOML is off, and it is off for a reason that is not "the rule was noisy".**
  //
  // The only TOML in this repository is `src-tauri/Cargo.toml` — the Rust shell's manifest. ESLint found a real
  // formatting complaint in it (`toml/array-element-newline`), and that is the problem: the Rust side cannot see this
  // linter, so the next `cargo`-side edit re-breaks it and the failure lands on whoever runs `pnpm run lint` next.
  // One file, owned by another toolchain, linted by this one, is a rule that can only ever be right by accident.
  toml: false,

  vue: {
    overrides: {
      'vue/block-order': ['error', {
        order: ['style', 'template', 'script']
      }],
      'vue/comma-dangle': ['warn', 'never']
    }
  },

  jsonc: {
    overrides: {
      'jsonc/comma-dangle': ['warn', 'never']
    }
  }
})
