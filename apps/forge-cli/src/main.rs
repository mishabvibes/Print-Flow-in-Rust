//! Forge Studio on the command line: the same engine the app uses, without a window.
//!
//! ```text
//! forge-cli commands                         list every command with its parameters
//! forge-cli demo OUT.forgeproject            write the demo project
//! forge-cli render PROJECT OUT.png [ROW]     render one card
//! forge-cli sheet PROJECT OUT.png [INDEX]    render one print sheet
//! forge-cli export PROJECT KIND OUT          KIND = cards | cropped | pdf | sheets
//! forge-cli run PROJECT JOURNAL.jsonl [OUT]  replay a journal, optionally save the result
//! forge-cli exec PROJECT ID [JSON] [OUT]     run one command (and save to OUT)
//! forge-cli bench PROJECT                    time the renderer
//! forge-cli detect IMAGE                     find the face in a photo
//! ```
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use forge_engine::jobs::{ExportJob, run_export};
use forge_engine::{Engine, Value, commands, json};
use forge_export::Control;
use forge_render::RenderOpts;

fn engine_with(project: Option<&str>) -> Result<Engine, String> {
    let mut e = Engine::new();
    e.persist_prefs = false;
    if let Some(p) = project {
        e.execute("project.open", json!({ "path": p })).map_err(|err| format!("cannot open {p}: {err}"))?;
    }
    Ok(e)
}

fn run(args: &[String]) -> Result<(), String> {
    let a = |i: usize| args.get(i).map(String::as_str);
    let need = |i: usize, what: &str| a(i).ok_or_else(|| format!("missing {what}\n\n{}", usage()));
    match a(0) {
        Some("commands") => {
            for c in commands() {
                println!("{:<26} {}", c.id, c.doc);
            }
        }
        Some("demo") => {
            let out = need(1, "OUT.forgeproject")?;
            let mut e = engine_with(None)?;
            e.execute("project.demo", Value::Null).map_err(|e| e.to_string())?;
            e.execute("project.save", json!({ "path": out })).map_err(|e| e.to_string())?;
            println!("wrote {out}");
        }
        Some("render") => {
            let e = engine_with(Some(need(1, "PROJECT")?))?;
            let out = need(2, "OUT.png")?;
            let row: usize = a(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let img = e.renderer.render_row(&e.project, row, RenderOpts::EXPORT).map_err(|e| e.to_string())?;
            std::fs::write(out, forge_image::encode_png(&img, false).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            println!("wrote {out} ({}x{})", img.width, img.height);
        }
        Some("sheet") => {
            let mut e = engine_with(Some(need(1, "PROJECT")?))?;
            let out = need(2, "OUT.png")?;
            let index: usize = a(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            e.execute("render.sheet", json!({ "index": index, "path": out })).map_err(|e| e.to_string())?;
            println!("wrote {out}");
        }
        Some("export") => {
            let e = engine_with(Some(need(1, "PROJECT")?))?;
            let kind = need(2, "KIND")?;
            let path = PathBuf::from(need(3, "OUT")?);
            let dpi = e.project.sheet.dpi;
            let job = match kind {
                "cards" => ExportJob::CardsZip { path },
                "cropped" => ExportJob::CroppedZip { path, long_edge: 600, quality: 92 },
                "pdf" => ExportJob::SheetsPdf { path, dpi, jpeg_quality: Some(92) },
                "sheets" => ExportJob::SheetsZip { path, dpi },
                other => return Err(format!("unknown export kind `{other}`")),
            };
            let t = Instant::now();
            let ctl = Control::with_progress(|d, n| eprint!("\r{d}/{n}"));
            let rep = run_export(&job, &e.snapshot(), &e.renderer, &ctl).map_err(|e| e.to_string())?;
            eprintln!();
            println!("wrote {} items to {} in {:.1}s", rep.written, rep.path.display(), t.elapsed().as_secs_f32());
            for (n, why) in &rep.failed {
                eprintln!("skipped {n}: {why}");
            }
        }
        Some("run") => {
            let mut e = engine_with(Some(need(1, "PROJECT")?))?;
            let journal = std::fs::read_to_string(need(2, "JOURNAL.jsonl")?).map_err(|e| e.to_string())?;
            let errs = e.replay(&journal);
            for (line, err) in &errs {
                eprintln!("line {line}: {err}");
            }
            if let Some(out) = a(3) {
                e.execute("project.save", json!({ "path": out })).map_err(|e| e.to_string())?;
            }
            if !errs.is_empty() {
                return Err(format!("{} journal lines failed", errs.len()));
            }
        }
        Some("exec") => {
            let mut e = engine_with(Some(need(1, "PROJECT")?))?;
            let id = need(2, "ID")?;
            let params: Value = a(3).map(serde_json::from_str).transpose().map_err(|e| format!("bad JSON: {e}"))?.unwrap_or(Value::Null);
            let v = e.execute(id, params).map_err(|e| e.to_string())?;
            println!("{v}");
            if let Some(out) = a(4) {
                e.execute("project.save", json!({ "path": out })).map_err(|e| e.to_string())?;
            }
        }
        Some("bench") => {
            let e = engine_with(Some(need(1, "PROJECT")?))?;
            let n = e.project.card_count().max(1);
            let t = Instant::now();
            e.renderer.render_row(&e.project, 0, RenderOpts::EXPORT).map_err(|e| e.to_string())?;
            println!("first card (loads fonts and the template): {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
            let t = Instant::now();
            for i in 0..n {
                e.renderer.render_row(&e.project, i, RenderOpts::EXPORT).map_err(|e| e.to_string())?;
            }
            println!("{n} cards, one after another: {:.1} ms each", t.elapsed().as_secs_f64() * 1e3 / n as f64);
        }
        Some("detect") => {
            let p = need(1, "IMAGE")?;
            let img = forge_image::decode(&std::fs::read(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            let t = Instant::now();
            let d = forge_vision::detect_face(&img);
            println!("{d:?} in {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
        }
        _ => return Err(usage().to_string()),
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: forge-cli commands | demo OUT | render PROJECT OUT [ROW] | sheet PROJECT OUT [INDEX] | export PROJECT cards|cropped|pdf|sheets OUT | run PROJECT JOURNAL [OUT] | exec PROJECT ID [JSON] [OUT] | bench PROJECT | detect IMAGE"
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
