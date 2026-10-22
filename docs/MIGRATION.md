# From the Electron app

| Old | New |
|---|---|
| Crop Photos tab | `ui-egui/tabs/crop.rs`, commands `photos.*` |
| Design & Generate tab | `tabs/design.rs`, commands `design.*`, `data.*` |
| Sheet Composer | `tabs/arrange.rs`, commands `sheet.*`, `export.*` |
| `.idcardproject` `{cropping, studio, composer}` | `.forgeproject` ZIP; legacy files import |
| design preset `{template, fields}` | same JSON (`design.save_preset/load_preset`) |
| layout preset `{sheet, margin, card, gap, gridMode, customGrid, gridBorder}` | same JSON |
| face-api / canvas | `forge-vision` (BlazeFace + SeetaFace) and `forge-render` |
| jsPDF / html2canvas | `forge-export::pdf` (streamed) |
| main.js system font list | `forge-text` font scan (background at start-up) |

## Changed on purpose

* Newlines in a multi-line text field start a new line (the old app treated them as spaces).
* A photo crop whose aspect differs slightly from its box is scaled uniformly and centred, not stretched.
* PDFs are written page by page, so a 500-card job no longer holds every sheet in memory.
* File names in exports keep non-Latin letters (Malayalam names are no longer reduced to underscores) and are made unique.

* Ctrl+D is Deselect (as in Photoshop); duplicating a field is Ctrl+J. T and P select a tool; click or drag on the card to place the field.

## Known limits

* The UI toolkit (egui) cannot shape Indic scripts in its own widgets, so Malayalam in the data table shows boxes. The card canvas, previews and exports are correct (they use `harfrust`).
* No Indic font is bundled; text needs a system font that covers the script.
* BlazeFace short-range can miss very small faces; SeetaFace then tries, and "needs review" photos can be cropped by hand.
* No full bidirectional reordering.
* The GUI was exercised headlessly (Xvfb + software GL, screenshots of all tabs). Native file dialogs and drag handles were not driven by a human here, so expect small rough edges.
