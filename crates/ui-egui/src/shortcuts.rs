//! Keyboard shortcuts: one table that drives key handling, menu labels, tooltips and the
//! "Keyboard Shortcuts" dialog, so they can never disagree. The set follows what users of
//! Photoshop and Illustrator expect, plus everything the previous version of this app had
//! (V/T/P tools, arrows, Delete, Space-pan, Ctrl +/-/0, Alt+wheel, Ctrl+N/O/S/Z/Y).

use egui::{Context, Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Export,
    Quit,
    Undo,
    Redo,
    Duplicate,
    Deselect,
    Delete,
    ToolMove,
    ToolText,
    ToolPhoto,
    ToolHand,
    ToolZoom,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    Zoom100,
    TogglePanels,
    ToggleSnap,
    TabCrop,
    TabDesign,
    TabArrange,
    Palette,
    Help,
    Forward,
    Backward,
    ToFront,
    ToBack,
    NextField,
    PrevField,
    NudgeLeft,
    NudgeRight,
    NudgeUp,
    NudgeDown,
    NudgeLeftBig,
    NudgeRightBig,
    NudgeUpBig,
    NudgeDownBig,
    PrevItem,
    NextItem,
    FirstItem,
    LastItem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Design,
    Crop,
    Arrange,
}

pub struct Binding {
    pub action: Action,
    pub mods: Modifiers,
    pub key: Key,
    pub group: &'static str,
    pub desc: &'static str,
    pub scope: Scope,
    /// Works while a text box has focus (file and edit shortcuts do; single keys must not).
    pub in_text: bool,
}

const N: Modifiers = Modifiers::NONE;
const C: Modifiers = Modifiers::COMMAND;
const S: Modifiers = Modifiers::SHIFT;
const A: Modifiers = Modifiers::ALT;
const CS: Modifiers = Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true };
const CA: Modifiers = Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: true };
const CAS: Modifiers = Modifiers { alt: true, ctrl: false, shift: true, mac_cmd: false, command: true };

macro_rules! b {
    ($a:ident, $m:expr, $k:ident, $g:literal, $d:literal, $s:ident, $t:literal) => {
        Binding { action: Action::$a, mods: $m, key: Key::$k, group: $g, desc: $d, scope: Scope::$s, in_text: $t }
    };
}

pub const BINDINGS: &[Binding] = &[
    b!(New, C, N, "File", "New project", Global, true),
    b!(Open, C, O, "File", "Open project", Global, true),
    b!(Save, C, S, "File", "Save project", Global, true),
    b!(SaveAs, CS, S, "File", "Save project as…", Global, true),
    b!(Export, C, E, "File", "Export (cards, cropped photos or sheets, depending on the workspace)", Global, true),
    b!(Quit, C, Q, "File", "Quit", Global, true),
    b!(Undo, C, Z, "Edit", "Undo", Global, false),
    b!(Undo, CA, Z, "Edit", "Step backward (undo)", Global, false),
    b!(Redo, CS, Z, "Edit", "Redo", Global, false),
    b!(Redo, C, Y, "Edit", "Redo", Global, false),
    b!(Duplicate, C, J, "Edit", "Duplicate the selected field", Design, false),
    b!(Deselect, C, D, "Edit", "Deselect", Design, false),
    b!(Deselect, N, Escape, "Edit", "Deselect / cancel the tool", Design, false),
    b!(Delete, N, Delete, "Edit", "Delete the selected field", Design, false),
    b!(Delete, N, Backspace, "Edit", "Delete the selected field", Design, false),
    b!(ToolMove, N, V, "Tools", "Move tool (select, move, resize, rotate)", Design, false),
    b!(ToolText, N, T, "Tools", "Text field tool (click or drag on the card)", Design, false),
    b!(ToolPhoto, N, P, "Tools", "Photo field tool (click or drag on the card)", Design, false),
    b!(ToolHand, N, H, "Tools", "Hand tool (pan)", Design, false),
    b!(ToolZoom, N, Z, "Tools", "Zoom tool (click zooms in, Alt+click out)", Design, false),
    b!(ZoomIn, C, Equals, "View", "Zoom in", Global, false),
    b!(ZoomIn, C, Plus, "View", "Zoom in", Global, false),
    b!(ZoomOut, C, Minus, "View", "Zoom out", Global, false),
    b!(ZoomFit, C, Num0, "View", "Fit on screen", Global, false),
    b!(Zoom100, C, Num1, "View", "Actual size (100%)", Global, false),
    b!(TogglePanels, N, Tab, "View", "Show / hide panels and tools", Global, false),
    b!(ToggleSnap, CS, Semicolon, "View", "Snap to guides on / off", Design, false),
    b!(TabCrop, A, Num1, "Workspace", "Crop Photos", Global, false),
    b!(TabDesign, A, Num2, "Workspace", "Design & Generate", Global, false),
    b!(TabArrange, A, Num3, "Workspace", "Arrange & Print", Global, false),
    b!(Palette, C, K, "Help", "Command palette", Global, true),
    b!(Help, CAS, K, "Help", "Keyboard shortcuts", Global, true),
    b!(Help, N, F1, "Help", "Keyboard shortcuts", Global, true),
    b!(Forward, C, CloseBracket, "Arrange", "Bring forward", Design, false),
    b!(Backward, C, OpenBracket, "Arrange", "Send backward", Design, false),
    b!(ToFront, CS, CloseBracket, "Arrange", "Bring to front", Design, false),
    b!(ToBack, CS, OpenBracket, "Arrange", "Send to back", Design, false),
    b!(NextField, A, CloseBracket, "Arrange", "Select the next field", Design, false),
    b!(PrevField, A, OpenBracket, "Arrange", "Select the previous field", Design, false),
    b!(NudgeLeft, N, ArrowLeft, "Move", "Nudge 0.2%  (Crop: move the photo 1%)", Design, false),
    b!(NudgeRight, N, ArrowRight, "Move", "Nudge 0.2%", Design, false),
    b!(NudgeUp, N, ArrowUp, "Move", "Nudge 0.2%", Design, false),
    b!(NudgeDown, N, ArrowDown, "Move", "Nudge 0.2%", Design, false),
    b!(NudgeLeftBig, S, ArrowLeft, "Move", "Nudge 2%  (Shift)", Design, false),
    b!(NudgeRightBig, S, ArrowRight, "Move", "Nudge 2%  (Shift)", Design, false),
    b!(NudgeUpBig, S, ArrowUp, "Move", "Nudge 2%  (Shift)", Design, false),
    b!(NudgeDownBig, S, ArrowDown, "Move", "Nudge 2%  (Shift)", Design, false),
    b!(PrevItem, N, PageUp, "Navigate", "Previous photo (Crop) / previous sheet (Arrange)", Global, false),
    b!(NextItem, N, PageDown, "Navigate", "Next photo (Crop) / next sheet (Arrange)", Global, false),
    b!(FirstItem, N, Home, "Navigate", "First photo / first sheet", Global, false),
    b!(LastItem, N, End, "Navigate", "Last photo / last sheet", Global, false),
];

fn weight(m: Modifiers) -> u8 {
    u8::from(m.alt) + u8::from(m.shift) + u8::from(m.command || m.ctrl || m.mac_cmd)
}

/// The shortcut text of the first binding of `a` ("Ctrl+Shift+Z"), formatted for this platform.
pub fn label(ctx: &Context, a: Action) -> Option<String> {
    BINDINGS.iter().find(|b| b.action == a).map(|b| ctx.format_shortcut(&KeyboardShortcut::new(b.mods, b.key)))
}

/// Consumes the shortcuts pressed this frame. `typing` is true while a text box has focus: then only
/// the shortcuts marked `in_text` are used. Longer combinations are tried first so that
/// Ctrl+Shift+Z is not read as Ctrl+Z.
pub fn poll(ctx: &Context, typing: bool) -> Vec<Action> {
    let mut order: Vec<&Binding> = BINDINGS.iter().collect();
    order.sort_by_key(|b| std::cmp::Reverse(weight(b.mods)));
    let mut out = Vec::new();
    ctx.input_mut(|i| {
        for b in order {
            if typing && !b.in_text {
                continue;
            }
            if i.consume_shortcut(&KeyboardShortcut::new(b.mods, b.key)) {
                out.push(b.action);
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, RawInput};

    fn press(key: Key, mods: Modifiers) -> Vec<Action> {
        let ctx = Context::default();
        let mut got = Vec::new();
        let raw = RawInput {
            events: vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: mods }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            got = poll(ui.ctx(), false);
        });
        out.textures_delta.clear();
        got
    }

    #[test]
    fn no_two_bindings_share_keys() {
        let mut seen = std::collections::HashSet::new();
        for b in BINDINGS {
            assert!(seen.insert((format!("{:?}", b.mods), b.key)), "duplicate binding for {:?}", b.action);
        }
    }

    #[test]
    fn photoshop_style_keys_map_to_actions() {
        assert_eq!(press(Key::Z, Modifiers::COMMAND), vec![Action::Undo]);
        assert_eq!(press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT), vec![Action::Redo]);
        assert_eq!(press(Key::Num0, Modifiers::COMMAND), vec![Action::ZoomFit]);
        assert_eq!(press(Key::Num1, Modifiers::COMMAND), vec![Action::Zoom100]);
        assert_eq!(press(Key::Minus, Modifiers::COMMAND), vec![Action::ZoomOut]);
        assert_eq!(press(Key::Equals, Modifiers::COMMAND), vec![Action::ZoomIn]);
        assert_eq!(press(Key::J, Modifiers::COMMAND), vec![Action::Duplicate]);
        assert_eq!(press(Key::V, Modifiers::NONE), vec![Action::ToolMove]);
        assert_eq!(press(Key::T, Modifiers::NONE), vec![Action::ToolText]);
        assert_eq!(press(Key::P, Modifiers::NONE), vec![Action::ToolPhoto]);
        assert_eq!(press(Key::ArrowLeft, Modifiers::SHIFT), vec![Action::NudgeLeftBig]);
        assert_eq!(press(Key::ArrowLeft, Modifiers::NONE), vec![Action::NudgeLeft]);
        assert_eq!(press(Key::S, Modifiers::COMMAND | Modifiers::SHIFT), vec![Action::SaveAs]);
        assert_eq!(press(Key::N, Modifiers::COMMAND), vec![Action::New]);
    }

    #[test]
    fn single_keys_are_ignored_while_typing() {
        let ctx = Context::default();
        let mut got = vec![Action::New];
        let raw = RawInput {
            events: vec![Event::Key { key: Key::V, physical_key: Some(Key::V), pressed: true, repeat: false, modifiers: Modifiers::NONE }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| got = poll(ui.ctx(), true));
        out.textures_delta.clear();
        assert!(got.is_empty());
    }
}
