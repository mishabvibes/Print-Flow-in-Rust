# Architecture

Copied from VectorCraft on purpose: it is what keeps the app fast and the code easy to change.

## Layers

A crate may only depend on crates of a lower layer; `cargo xtask layers` enforces it from the `Cargo.toml` files. Nothing below `ui-egui` knows that egui exists, so the whole product runs headless (`forge-cli`) and every behaviour is testable without a window.

## Everything is a command

`forge-engine` is the only way to change a project. A command has an id (`design.update_field`), a label, menu path, shortcut, one-line doc and a function over `(engine, JSON params)`. The menus, the command palette (Ctrl+K), the CLI (`forge-cli commands`) and the tests all use the same table.

* **Undo/redo**: a stack of `Project` snapshots. The heavy parts (table, photo library) are `Arc`s, so a snapshot is a handful of pointer copies. A drag or slider is one step (commands carry a coalescing key).
* **Journal**: each executed command is recorded as id + params, replayable with `forge-cli run` to reproduce a session.
* **Guard**: a panic inside a command is caught, the project is rolled back and the caller gets an error value. The workspace denies `unwrap`, `expect`, `panic!`, `todo!`, `unreachable!` and `unsafe` by lint.

## Rendering

CPU vector rendering with `vello_cpu`: template, photos (with circle masks, borders, opacity, rotation) and text are all drawn as paths, so output is sharp at any DPI and identical for preview, PNG, PDF and the sheet. One render context per card; whole sheets render their cards in parallel with rayon.

Text: `harfrust` shapes each run (Malayalam and other complex scripts, ligatures, small caps), `skrifa` supplies outlines. A line is shaped once and then scaled, so shrink-to-fit is cheap. Layout covers wrapping, grapheme-safe breaking, hyphenation, justify, ellipsis, max lines, super/subscript, underline and strike.

Faces: `forge-vision` contains a small TFLite interpreter written for BlazeFace (no native runtime) and the SeetaFace fallback.

## Responsiveness

The UI thread draws and issues commands. Decoding, thumbnails, face detection, sheet previews and exports run on a rayon pool and report back through a channel. Previews are "latest wins": a render request that was overtaken before it started does nothing, and the previous image stays on screen until the new one arrives. Exports take a snapshot, show progress, can be cancelled and write through a `.part` file that is renamed on success.

## Files

`.forgeproject` is a ZIP: `project.json` plus `media/…` entries referenced as `zip:media/…`. Writes are atomic. The reader also accepts the old JSON project (the two photo stores are merged, cropping wins).
