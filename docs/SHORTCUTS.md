# Keyboard shortcuts

Also in the app: **Help → Keyboard shortcuts** (F1). On macOS read Ctrl as ⌘. The list is generated from one table (`crates/ui-egui/src/shortcuts.rs`), so the menus, tooltips and this dialog always agree.

| Group | Keys | Action |
|---|---|---|
| File | Ctrl+N / O / S | New / Open / Save |
| File | Ctrl+Shift+S | Save as |
| File | Ctrl+E | Export (cropped photos, cards or sheets, by workspace) |
| File | Ctrl+Q | Quit |
| Edit | Ctrl+Z, Ctrl+Alt+Z | Undo |
| Edit | Ctrl+Shift+Z, Ctrl+Y | Redo |
| Edit | Ctrl+J | Duplicate the selected field |
| Edit | Ctrl+D, Esc | Deselect |
| Edit | Delete, Backspace | Delete the selected field |
| Tools (Design) | V | Move: select, move, resize, rotate |
| Tools (Design) | T / P | Text field / Photo field tool: click or drag on the card |
| Tools | H | Hand (pan) |
| Tools | Z | Zoom (click in, Alt+click out) |
| View | Ctrl + / Ctrl + = | Zoom in |
| View | Ctrl + - | Zoom out |
| View | Ctrl+0 | Fit on screen (Crop: back to the automatic crop) |
| View | Ctrl+1 | Actual size, 100 % |
| View | Tab | Hide / show all panels |
| View | Ctrl+Shift+; | Snap on / off |
| Workspace | Alt+1 / 2 / 3 | Crop Photos / Design & Generate / Arrange & Print |
| Arrange | Ctrl+] / Ctrl+[ | Bring forward / send backward |
| Arrange | Ctrl+Shift+] / Ctrl+Shift+[ | Bring to front / send to back |
| Arrange | Alt+] / Alt+[ | Select next / previous field |
| Move | Arrow keys | Nudge the field 0.2 % (Shift: 2 %); in Crop, move the photo 1 % (Shift: 10 %) |
| Navigate | Page Up / Page Down | Previous / next photo (Crop) or sheet (Arrange) |
| Navigate | Home / End | First / last |
| Help | Ctrl+K | Command palette |
| Help | F1, Ctrl+Alt+Shift+K | Keyboard shortcuts |

## Mouse

| Gesture | Effect |
|---|---|
| Wheel | Pan |
| Ctrl + wheel, Alt + wheel, pinch | Zoom about the pointer |
| Space + drag, middle-button drag | Pan with any tool |
| Drag a field / a handle / the round handle | Move (Alt: no snapping) / resize (Shift: keep ratio) / rotate (Shift: 15° steps) |
| Crop: drag / wheel | Move / zoom the photo inside the frame |

## From the previous version

Kept: Ctrl+N/O/S/Z/Shift+Z/Y, V/T/P, arrow nudging (0.2 %, Shift 2 %), Delete, Space-drag, Ctrl +/-/0, Alt+wheel zoom, wheel pan.
Changed to follow Photoshop: **Ctrl+D is now Deselect** (duplicate moved to Ctrl+J), and T/P now select a tool instead of adding a field at once.
