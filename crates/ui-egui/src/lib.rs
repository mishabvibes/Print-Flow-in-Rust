//! Forge Studio's egui frontend. Panels only draw and issue commands: every state change goes through
//! the engine, so nothing here is needed to run, test or script the application.
#![forbid(unsafe_code)]
// Nested `if let` in event handlers reads better than let-chains here.
#![allow(clippy::collapsible_if)]

pub mod geometry;
pub mod platform;
pub mod shell;
pub mod shortcuts;
pub mod tabs;
pub mod theme;
pub mod view;
pub mod widgets;
pub mod workers;

mod app;
pub use app::{App, Tab};
