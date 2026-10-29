# Working in this repository

The shell and the interface around `synra-core`. Rust in `src-tauri/`, TypeScript and Vue in `src/`.

Its sibling is [`synra-core`](https://github.com/synra-rs/synra-core); the boundary is in the README ("if it has to work
without a UI, it belongs to core"). This file is what a person needs *before* their first edit, and it exists because those
facts were learned the expensive way.

The root `rust-toolchain.toml` also selects Rust when commands run inside `src-tauri/`. CI installs this development toolchain;
the separate MSRV job reads `src-tauri/Cargo.toml` and uses `cargo +<msrv>`, without changing the default compiler.

## 1. Two languages, two gates, and neither is optional

```powershell
cargo test --no-default-features --manifest-path src-tauri/Cargo.toml   # the shell, without Tauri
cargo check --manifest-path src-tauri/Cargo.toml                        # the shell, with Tauri (shell.rs)
pnpm test        # vitest: node by default, four pages opt into jsdom
pnpm run lint    # eslint — and it is not decoration, see §4
pnpm run build   # vue-tsc -b + vite build
pnpm run shapes  # seven guards the README calls hard requirements — one of them now checks the README itself
```

★ **Both Cargo invocations.** `shell.rs` only compiles under the default feature, so `--no-default-features` alone cannot
see a mistake in it; CI runs only the first because a Linux runner has no WebKitGTK. That is a fact about CI, not about
your machine — the second command works here.

★ **`cargo run -p guards` is not in this list, and that is not an omission**: `guards` is a binary in the **core** workspace,
so it is run from `synra-rust-core/`, not from here. `cargo run -p guards` in this repository fails with "package not found in
workspace", which reads like a broken guard and is really a wrong directory.

## 2. Facts to hold while editing

| Fact | Why it matters |
| --- | --- |
| `src/events.ts` is the **only** subscriber to `KERNEL_EVENT_CHANNEL` | A second one means two sequence trackers on one channel, and both conclude they missed events |
| Four stores (`devices`, `plugins`, `intents`, `settings`) each have `apply(event): boolean` | `false` means "I cannot place this", which is the caller's cue to re-read — never "nothing happened" |
| Pages are **readers** of a store | A page that keeps its own copy goes stale the moment an event arrives |
| A card's description lives in `commands::devices` | `card_for` / `card_from_paired` / `card_from_seen` are the only builders; a second one drifts |
| `AppState.hosted_ui` records which plugin page is in which WebView | A token is a nonce: it cannot say which plugin a page is |

**Host UI boundary (28 R56):** pages provide device and plugin management, settings, diagnostics, and generic activity
views and controls. Content sharing, clipboard, direct text/file sending and file-opening launchers belong in plugin
pages. Plugin names, trace IDs and example task records are display data, not permission to add fixed plugin features.
Preserve generic task approvals and cancellation, and distinguish plugin management's Open button from a business action.

## 3. Types at the IPC boundary come from `@synra/protocol`

Never hand-write the shape of a command's answer. Doing that hid two defects at once: the settings page typed
`config_get` as a bare map when it answers `{ values, overridden }`, and then read `theme` where the kernel uses `ui.theme`.
The wrong annotation made `vue-tsc` pass for a dozen rounds while both controls showed "unset".

## 4. The guards and the linter disagree by default

`eslint --fix`'s `object-shorthand` writes `{ pluginId }`; `command-arguments` reads `{ pluginId: pluginId }`. The guard was
taught to expand shorthand, which is the right fix — but the lesson generalises: **a fixer and a checker disagreeing is
invisible until both run in the same place**, which is why `pnpm run lint` is a CI step.

## 5. Editing files

- **Any edit touching more than one line goes through the editor tool. Not "prefer" — do not use the shell.** PowerShell
  string replacement has silently done nothing, written a literal `\r\n`, inserted a bare CR into a doc comment, and merged
  two list items, seven times in this repository's history. Every one of those produced a *successful-looking* command whose
  failure only showed up in the next build. Single-line replacements with an exact anchor are all the shell may be used for,
  and even then the result is re-read.
- **The reason is mechanical, not stylistic**: inside a double-quoted PowerShell string a backtick is an escape — `` `t `` is a
  tab, `` `r `` a carriage return, `` `n `` a newline. Every comment in this repository is full of `` `identifier` `` markup,
  so a shell edit that looks like it copies a sentence quietly eats the character after each backtick. That is how
  `read by `transfer_list`` became `read by <TAB>ransfer_list`.
- Re-read after a tool rewrites a file: a stale read makes the next edit fail, and a failed edit that is retried with a
  looser anchor is how a heading gets deleted.
- When replacing an anchor, **write the anchor back**. The most common self-inflicted error here is an edit whose
  `new_string` omits the line it matched.

## 6. Tests

- Unit tests live beside their module (`commands/plugin_ui/tests.rs`); component tests are `*.mount.test.ts` next to the page.
- A mounted page needs three things jsdom lacks: `matchMedia` (a **placeholder that is not a function** — assign it
  unconditionally), `ResizeObserver`, `IntersectionObserver`; plus a fresh Pinia and, if the page uses it, a router.
- Answer mocks **by command name**. A blanket `{ ok: true, value: [] }` fails far from the call that was wrong.
- Assert a notice through its **props**, not its text; the text is a translated sentence.
- Visibility and focus tests mount with `attachTo: document.body` and always unmount. Detached trees can report misleading
  visibility; forms with segmented controls contain radio inputs, so target the named text input rather than the first input.
- An assertion against a field the DTO does not have passes on `undefined`. `INTENT.summary` did.
- **`crate::testing::state()` puts every test in one directory** (`temp_dir()/synra-app-tests`), while `file_state()` gets a
  named one. A test that writes files and uses `state()` therefore shares a data directory with every other such test, and
  `cargo test` runs them in parallel — so the failure looks like a flake and is really a collision. Use `file_state()` (or
  `root(name)`) for anything that touches the disk. One run of this suite did report a single failure that five reruns could
  not reproduce, and this is the first place to look.

## 7. Commits

Conventional Commits, subject ≤ 50 characters, no body, with GPG signing. One commit per unit of work; push after each. If a step is
part of a larger change, say so in the subject (`chore: depend on the UI session registry`) rather than pretending it is
finished.

- **Verify in one call, commit in the next. Never chain them.** A shell line that runs `pnpm test` and then `git commit`
  commits even when the test failed: the exit code is printed and ignored, and the output scrolls past. This has happened six
  times in this repository's history, each time producing a commit whose own message was contradicted by the line above it in
  the same output. The order that works is: run the gates, read the exit codes, and only then write the commit.

## 8. Recording what was learned

Design decisions and their reasons go into [`design-docs/08`](https://github.com/synra-rs/design-docs), §9, with a date.
That file is a status ledger, not a plan: what is done, what is not, and *why* — including the attempts that failed. A
failed approach recorded there is worth more than a successful one, because the next person will try it again otherwise.

★ **A table that exists is not a table in use.** Before treating a schema as a source of truth, find **who writes it and who
reads it** — `grep` for the insert and for the select. This session found three separate cases: `plugin_versions` had no
reader, `sync_records` had no writer outside tests, and an address the shell needed was in a table it was not consulting. Each
one would have produced a feature that compiled, returned a well-shaped answer, and was empty.
