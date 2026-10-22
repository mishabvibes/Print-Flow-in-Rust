# Forge Studio (pure Rust)

A complete rebuild of the Electron "Data Merge / ID Card Studio" in pure Rust, with the architecture and performance habits of the VectorCraft reference project. No JavaScript, no web view, no native libraries to install: `cargo build --release` produces one `forge-studio` executable (plus the `forge-cli` tool).

## What it does

1. **Crop Photos**: add photos or a folder; faces are found automatically (BlazeFace with eye keypoints, SeetaFace as fallback) and framed at the chosen shape (35×45 mm, 2×2 in or custom). Drag to pan, wheel or slider to zoom, "Apply to unaligned / all", background replacement, export the cropped photos as a ZIP (600 px long edge, JPEG).
2. **Design & Generate**: a template image with text and photo fields (percent geometry, rotation, opacity, smart-guide snapping, 8 resize handles), CSV/Excel data, column mapping, a photo-key column, per-row preview, editable data table, design presets, export of every card as PNG in a ZIP.
3. **Arrange & Print**: A3/A4/A5/Letter/Legal, orientation, margins, card size, gap, auto or custom grid, cell borders, cm/mm/in/px, 150/300/600 dpi, a live paged sheet preview, export as PDF (streamed, one page per sheet) or ZIP of PNGs, layout presets.

Projects are single `.forgeproject` files (a ZIP; photos and templates are stored once). Projects from the old app (`.idcardproject`) open and are converted on save.

## Look and keys

The interface follows the conventions of professional creative tools: neutral dark (or light) grey, a single blue accent, a tool strip, an options bar, collapsible docked panels (Properties, Layers, Template, Data) and a status bar with the zoom. It uses no third-party artwork or fonts. Shortcuts are in [docs/SHORTCUTS.md](docs/SHORTCUTS.md); `Tab` hides the panels, `Ctrl+0` / `Ctrl+1` / `Ctrl +/-` zoom, `Space`-drag pans.

## Build and run

```
cargo run --release -p forge-studio            # the app
cargo run --release -p forge-studio -- my.forgeproject
cargo run --release -p forge-cli -- demo demo.forgeproject
cargo run --release -p forge-cli -- export demo.forgeproject pdf sheets.pdf
cargo xtask ci                                  # fmt, clippy -D warnings, tests, layers, assets
```

On Linux the window needs X11 or Wayland and OpenGL (Mesa software rendering is enough).

## Layout

| Layer | Crates |
|---|---|
| 0 | `geom`, `color` |
| 1 | `image`, `doc` |
| 2 | `data`, `text`, `vision` |
| 3 | `render` |
| 4 | `export`, `format` |
| 5 | `engine` |
| 6 | `ui-egui` |
| apps | `forge-studio`, `forge-cli`, `xtask` |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design and [docs/MIGRATION.md](docs/MIGRATION.md) for how each feature of the old app maps to the new one and what changed on purpose.

## Licence

MIT OR Apache-2.0 for the code. Assets: see [ASSETS.md](ASSETS.md); **the bundled Stem fonts have no known licence**.
