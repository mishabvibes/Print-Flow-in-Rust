# Conventions

* New behaviour = a command in `crates/engine/src/cmd/`. Panels call `shell.cmd(...)`; they never edit the project directly.
* Respect the layers (`cargo xtask layers`). No `unwrap`/`expect`/`panic!`/`unsafe` (lints deny them).
* Keep the UI thread free of decoding, detection, rendering of sheets and exports: use `Workers`.
* Colours and sizes come from `ui-egui/src/theme.rs`.
* `cargo xtask ci` must pass.
