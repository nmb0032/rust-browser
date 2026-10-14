use std::{sync::OnceLock, time::Duration};

/// Wall-clock cost of the most recent restyle/layout of a `Frame`. Cheap to
/// copy, so it travels with snapshots without affecting clone cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageTimings {
    /// Cascade run eagerly by `set_stylesheets` when input state changes.
    pub eager_restyle: Duration,
    /// Static cascade inside the container-query loop.
    pub cascade: Duration,
    /// Animated cascade inside the container-query loop.
    pub animated_cascade: Duration,
    pub box_build: Duration,
    pub layout: Duration,
    /// Snapshot comparison and scroll-linked resampling.
    pub feedback: Duration,
    /// Container-query convergence passes performed.
    pub passes: u32,
    /// Whole `layout_with_container_queries` call.
    pub total: Duration,
}

impl StageTimings {
    pub fn summary(&self) -> String {
        format!(
            "eager_restyle={} cascade={} animated={} boxes={} layout={} feedback={} passes={} total={}",
            ms(self.eager_restyle),
            ms(self.cascade),
            ms(self.animated_cascade),
            ms(self.box_build),
            ms(self.layout),
            ms(self.feedback),
            self.passes,
            ms(self.total),
        )
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.1}ms", duration.as_secs_f64() * 1e3)
}

/// `RUST_BROWSER_FRAME_TRACE=1` prints a per-frame timing breakdown to stderr.
pub fn frame_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("RUST_BROWSER_FRAME_TRACE").is_some_and(|value| value != "0")
    })
}
