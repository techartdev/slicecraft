<!-- craft-house-rules:start core source=storytold/craft-contrib sha256=pending -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
<!-- craft-house-rules:end -->

<!-- craft-house-rules:start rust source=storytold/craft-contrib sha256=pending -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion.
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.
<!-- craft-house-rules:end -->

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `LicenseRef-Public-Domain`

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.

# SliceCraft — instructions for agents

SliceCraft is a clean-room, open-source, pure-Rust 3D print slicer targeting
Simplify3D-class behaviour — import, plate, profiles, perimeters, infill, support,
G-code — plus what Simplify3D does not have: agent control over MCP, a scriptable
CLI, a Linux and BSD build, and a free licence. Membership of the ArtCraft suite is
**pending approval**; build to the suite's conventions anyway.

Siblings with the same conventions: `../cadcraft`, `../photocraft`, `../vectorcraft`,
`../filmcraft`, `../lightcraft`, `../codecraft`, `../pdfcraft`, `../effectcraft`,
`../designcraft`. Approved community apps so far: SolidCraft, MailCraft.

## Start every session here

Read [`ROADMAP.md`](ROADMAP.md) and [`docs/gaps.md`](docs/gaps.md). `plan/` is an
optional gitignored notebook (screenshots, scratch notes). Do not ask for it and do
not stop when it is absent. Licence choices stay inside the allowlist above.

## Never crash

A slicer that crashes loses a print and the user's trust. **This outranks feature work.**

- **No panics in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`,
  `todo!`, `unimplemented!`; no `unsafe` (`unsafe_code = "forbid"`).
- **Errors are `Result<T, E>`** through the crate's error type and `?`. An unfinished
  feature returns an error or says "not implemented yet"; it never panics and never
  claims success.
- **Input-derived numbers are hostile** (STL/3MF/OBJ files, G-code, profiles, JSON
  control params): `get()` not `[i]`, checked arithmetic, no NaN reaching geometry,
  and every loop or allocation an input can size gets a cap
  (`MAX_TRIANGLES`, `MAX_LAYERS`, …).
- **Bound recursion** (nested objects, group graphs).
- **Prove it:** every crash fix lands with a regression test.

## Non-negotiables

- **Clean room.** Simplify3D may be *observed* black-box: run it, slice synthetic
  models, read the G-code it writes, use your own models and your own profiles.
  **Never** copy its code, strings, icons, profile files, support-material files,
  firmware definitions, or bundled models. Our profile keys, flavour table and
  G-code templates are our own design informed by observed output and public
  specifications (RepRap/Marlin/Klipper documentation, STL and 3MF specs).
  Its program is never decompiled or disassembled: behaviour is learned from
  running it and from the files it writes.
- **Assets are original.** Every icon is drawn in code. Any file asset is original,
  public domain/CC0, OSI-licensed or redistributable CC, and **must have a row in
  `ATTRIBUTION.md`**. Fonts live in `storytold/craft-fonts`, never here.
- **Test corpora live elsewhere** (`storytold/slicecraft-corpus`), never as large
  binaries in this repository.
- **Everything is a command.** User-visible behaviour = a `CommandSpec` in
  `crates/engine/src/lib.rs` (`id`, `label`, `state`) + a test. The UI, the command
  line, the CLI, the control channel and MCP all reach the same ids. A tool in
  `slicecraft-mcp` must name the engine command it drives.
- **Programmatic calls never open dialogs.**
- **Layering** is enforced by `cargo xtask layers`: L0 `geom` → L1 `doc` →
  L2 `render` → L3 `io` → L4 `engine` → L5 `ui-egui`, `mcp` → apps. Nothing below
  L5 depends on egui/eframe/winit/wgpu/rfd. **The UI crate is swappable.**
- **The UI is thin:** panels read engine state and act through commands.
- **Rust only** (no handwritten JS/TS). **Never break the wasm build** once it exists.
- **Quality gates before every commit:** `cargo xtask ci`.

## Working in this repository

- `cargo test --workspace` is fast and must stay fast.
- `cargo run -p slicecraft-cli -- info <file.stl>` and `slice <file.stl> <out.gcode>`
  are the headless checks; keep them working.
- Research notes go in `docs/research/`, numbered, each with a "what is evidence,
  what is inference" split and a list of open questions.
- Do not add a dependency without checking its licence against the allowlist above.
