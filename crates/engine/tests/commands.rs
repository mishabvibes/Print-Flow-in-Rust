#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use forge_engine::{Engine, EngineError, commands, json};

fn demo() -> Engine {
    let mut e = Engine::new();
    e.persist_prefs = false;
    e.execute("project.demo", json!(null)).unwrap();
    e
}

#[test]
fn undo_redo_and_dirty_tracking() {
    let mut e = demo();
    assert!(!e.is_dirty() && !e.can_undo());
    let id = e.execute("design.add_text_field", json!({"x": 5, "y": 5})).unwrap()["id"].as_str().unwrap().to_string();
    assert!(e.is_dirty() && e.can_undo());
    assert_eq!(e.project.design.fields.len(), 5);
    e.execute("edit.undo", json!(null)).unwrap();
    assert_eq!(e.project.design.fields.len(), 4);
    assert!(!e.is_dirty() || e.can_redo());
    e.execute("edit.redo", json!(null)).unwrap();
    assert!(e.project.design.field(&id).is_some());
    // A command that changes nothing leaves no undo step.
    let depth = e.journal().len();
    e.execute("design.update_field", json!({"id": id, "patch": {"x": 5}})).unwrap();
    e.execute("edit.undo", json!(null)).unwrap();
    assert!(e.project.design.field(&id).is_none(), "the no-op did not add a step: undo removed the field");
    assert!(e.journal().len() > depth);
}

#[test]
fn slider_drags_coalesce_into_one_step() {
    let mut e = demo();
    let id = e.project.design.fields[1].id.clone();
    let x0 = e.project.design.fields[1].x;
    for i in 0..20 {
        e.execute_with(
            "design.update_field",
            json!({"id": id, "patch": {"x": 10 + i}}),
            forge_engine::ExecOpts { coalesce: Some(format!("x:{id}")) },
        )
        .unwrap();
    }
    assert_eq!(e.project.design.field(&id).unwrap().x, 29.0);
    e.execute("edit.undo", json!(null)).unwrap();
    assert_eq!(e.project.design.field(&id).unwrap().x, x0);
}

#[test]
fn failed_commands_change_nothing() {
    let mut e = demo();
    let before = e.project.design.clone();
    assert!(matches!(e.execute("design.remove_field", json!({"id": "nope"})), Err(EngineError::NotFound(_))));
    assert!(matches!(e.execute("design.update_field", json!({"id": "f1", "patch": 5})), Err(EngineError::Invalid(_))));
    assert!(matches!(e.execute("no.such", json!(null)), Err(EngineError::UnknownCommand(_))));
    assert!(matches!(e.execute("data.set_key_column", json!({"column": "Nope"})), Err(EngineError::NotFound(_))));
    assert_eq!(e.project.design, before);
}

#[test]
fn field_values_are_sanitised() {
    let mut e = demo();
    let id = e.project.design.fields[1].id.clone();
    e.execute("design.update_field", json!({"id": id, "patch": {"x": 1e300, "fontSize": -5, "opacity": 900, "rotation": 1e9, "w": 0}})).unwrap();
    let f = e.project.design.field(&id).unwrap();
    assert!(f.x <= 300.0 && f.w >= 0.1 && f.opacity == 100.0 && f.rotation <= 360.0);
    assert!(f.text().unwrap().font_size >= 1.0);
}

#[test]
fn journal_replays_to_the_same_project() {
    let mut a = demo();
    a.execute("sheet.update", json!({"patch": {"card": {"w": 9.0}, "dpi": 600}})).unwrap();
    a.execute("design.add_photo_field", json!({"name": "Second photo"})).unwrap();
    a.execute("sheet.preset", json!({"name": "A3"})).unwrap();
    let lines = a.journal_lines();
    let mut b = Engine::new();
    assert!(b.replay(&lines).is_empty());
    assert_eq!(b.project.sheet, a.project.sheet);
    assert_eq!(b.project.design.fields.len(), a.project.design.fields.len());
}

#[test]
fn data_load_maps_and_guesses_key() {
    let dir = std::env::temp_dir().join(format!("forge-engine-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("people.csv");
    std::fs::write(&csv, "Name,AdmissionNo\nAnu,1001\nBibin,1002\n").unwrap();
    let mut e = Engine::new();
    e.persist_prefs = false;
    let r = e.execute("data.load", json!({"path": csv, "createFields": true})).unwrap();
    assert_eq!(r["rows"], 2);
    assert_eq!(e.project.design.fields.len(), 2);
    assert_eq!(e.project.design.mapping.len(), 2);
    assert_eq!(e.project.design.photo_key_column, "AdmissionNo");
    e.execute("edit.undo", json!(null)).unwrap();
    assert!(e.project.data.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn save_open_round_trip_and_recent() {
    let dir = std::env::temp_dir().join(format!("forge-engine-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut a = demo();
    a.execute("design.add_text_field", json!(null)).unwrap();
    a.execute("project.save", json!({"path": dir.join("p")})).unwrap();
    assert!(!a.is_dirty());
    assert!(dir.join("p.forgeproject").exists());
    let mut b = Engine::new();
    b.persist_prefs = false;
    b.execute("project.open", json!({"path": dir.join("p.forgeproject")})).unwrap();
    assert_eq!(b.project.design.fields.len(), 5);
    assert_eq!(b.project.photos.len(), 6);
    assert!(!b.can_undo());
    std::fs::remove_dir_all(dir).ok();
}

/// Random garbage into every command: nothing may panic (the guard turning a panic into
/// `Internal` counts as a failure here), and the project must stay renderable.
#[test]
fn command_sweep_never_panics() {
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    fn value(rnd: &mut dyn FnMut() -> u64, depth: u32) -> serde_json::Value {
        match rnd() % 9 {
            0 => json!(null),
            1 => json!(rnd() as i64),
            2 => json!([1e300, -1e300, 0.0, 1e-300][(rnd() % 4) as usize]),
            3 => json!(["", "a", "f1", "all", "unaligned", "#zzzzzz", "A4", "portrait", &"x".repeat(300)][(rnd() % 9) as usize]),
            4 => json!(rnd().is_multiple_of(2)),
            5 if depth < 3 => json!((0..rnd() % 4).map(|_| value(rnd, depth + 1)).collect::<Vec<_>>()),
            _ if depth < 3 => {
                let keys = [
                    "id", "x", "y", "w", "h", "patch", "path", "paths", "keys", "key", "index", "row", "column", "value", "zoom", "offsetX", "name",
                    "from", "to", "card", "dpi", "gap", "kind", "field", "results", "fontSize", "opacity", "rotation", "color",
                ];
                let mut m = serde_json::Map::new();
                for _ in 0..rnd() % 5 {
                    m.insert(keys[(rnd() % keys.len() as u64) as usize].to_string(), value(rnd, depth + 1));
                }
                serde_json::Value::Object(m)
            }
            _ => json!(rnd() as f64 / 7.0),
        }
    }
    let ids: Vec<&str> = commands()
        .map(|c| c.id)
        .filter(|id| {
            !id.starts_with("project.save")
                && !id.starts_with("export.")
                && !id.starts_with("render.")
                && !id.starts_with("fonts.add")
                && !id.starts_with("sheet.save")
                && !id.starts_with("design.save")
                && *id != "photos.add_folder"
        })
        .collect();
    let mut e = demo();
    for round in 0..3000 {
        let id = ids[(rnd() % ids.len() as u64) as usize];
        let params = value(&mut rnd, 0);
        if let Err(EngineError::Internal(m)) = e.execute(id, params.clone()) {
            panic!("round {round}: {id} {params} panicked: {m}");
        }
        if round % 500 == 499 {
            // Still renders after all that abuse.
            let _ = e.renderer.render_row(&e.project, 0, forge_render::RenderOpts::PREVIEW);
            if e.project.design.template.is_none() {
                e.execute("project.demo", json!(null)).unwrap();
            }
        }
    }
}
