//! Period math and the overview the UI renders.

mod daily;
mod overview;
mod periods;
mod window;

pub use daily::{daily_totals, DayTotal, MAX_DAYS};
pub use overview::{build_overview, AgentOverview, Overview};
pub use periods::{custom_range, local_midnight, range_for, Period, Range};
pub use window::{current_window, UsageWindow, WINDOW_MS};
