//! Native UI modules (egui). Shared types and mock data come from `app-api` and are re-exported so every module's `crate::view` / `crate::mock` paths keep working.

pub use app_api::{mock, view};

pub mod diagnostics;
pub mod expanded;
pub mod motion;
pub mod settings;
pub mod statistics;
pub mod theme;
pub mod web;
