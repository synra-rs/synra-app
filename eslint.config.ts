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
