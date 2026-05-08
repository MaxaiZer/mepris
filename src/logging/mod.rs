pub mod test;

use crate::logging::EventType::Unknown;
use colored::Colorize;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::io::Write;
use std::option::Option;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use strum_macros::{Display, EnumString};
use tracing::field::{Field, Visit};
use tracing::span::Attributes;
use tracing::{Event, Id, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, registry};

pub struct CustomLayer<W> {
    out: Mutex<W>,
}

impl<W> CustomLayer<W> {
    pub fn new(out: W) -> Self {
        Self {
            out: Mutex::new(out),
        }
    }
}

#[derive(Default, Clone, PartialEq)]
struct StepSpanData {
    step_id: Option<String>,
    number: Option<u32>,
    total_steps: Option<u32>,
}

#[derive(Default, Clone, PartialEq)]
struct SpanCommonData {
    start: Option<Instant>,
}

#[derive(EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum EventType {
    DryRunCompleted,
    RunCompleted,
    UserDecision,
    ScriptStarted,
    ScriptCompleted,
    ScriptsCheckCompleted,
    PackagesCheckCompleted,
    PackagesInstallStarted,
    StepCheckStarted,
    StepCheckFinished,
    FilterCompleted,
    StepRunStarted,
    StepRunFinished,
    CompletedStepSkipped,
    Unknown,
}

impl EventType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            EventType::UserDecision => "user_decision",
            EventType::PackagesCheckCompleted => "packages_check_completed",
            EventType::StepCheckStarted => "step_check_started",
            EventType::Unknown => "unknown",
            EventType::DryRunCompleted => "dry_run_completed",
            EventType::RunCompleted => "run_completed",
            EventType::ScriptCompleted => "script_completed",
            EventType::CompletedStepSkipped => "completed_step_skipped",
            EventType::StepCheckFinished => "step_check_finished",
            EventType::ScriptsCheckCompleted => "scripts_check_completed",
            EventType::FilterCompleted => "filter_completed",
            EventType::StepRunStarted => "step_run_started",
            EventType::StepRunFinished => "step_run_finished",
            EventType::PackagesInstallStarted => "packages_install_started",
            EventType::ScriptStarted => "script_started",
        }
    }
}

#[derive(EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum SpanType {
    StepCheck,
    Filter,
    Unknown,
}

impl SpanType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            SpanType::StepCheck => "step_check",
            SpanType::Unknown => "unknown",
            SpanType::Filter => "filter",
        }
    }
}

#[derive(Default)]
struct EventVisitor {
    fields: HashMap<String, String>,
}

impl Visit for EventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{:?}", value));
    }
}

impl<S, W> Layer<S> for CustomLayer<W>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    W: Write + Send + 'static,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let span = ctx.span(id).unwrap();

        let common_data = SpanCommonData {
            start: Some(Instant::now()),
        };
        let mut step_data = StepSpanData::default();

        struct SpanVisitor<'a> {
            step_data: &'a mut StepSpanData,
        }

        impl<'a> Visit for SpanVisitor<'a> {
            fn record_u64(&mut self, field: &Field, value: u64) {
                match field.name() {
                    "number" => self.step_data.number = Some(value as u32),
                    "total" => self.step_data.total_steps = Some(value as u32),
                    _ => {}
                }
            }

            fn record_str(&mut self, field: &Field, value: &str) {
                if field.name() == "step_id" {
                    self.step_data.step_id = Some(value.to_string());
                }
            }

            fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
        }

        attrs.record(&mut SpanVisitor {
            step_data: &mut step_data,
        });

        span.extensions_mut().insert(common_data);
        if step_data != StepSpanData::default() {
            span.extensions_mut().insert(step_data);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut out = self.out.lock().unwrap();
        let mut v = EventVisitor::default();
        event.record(&mut v);

        let level = *event.metadata().level();

        let (common_data, step_data) = fill_span_data(event, &ctx);
        let current_step_id: String = step_data.step_id.unwrap_or("?".to_string());
        let current_step_num: u32 = step_data.number.unwrap_or_default();
        let total_steps: u32 = step_data.total_steps.unwrap_or_default();
        let duration_from_nearest_span = common_data.start.unwrap_or(Instant::now()).elapsed();
        let duration_from_nearest_span_str = format_duration(duration_from_nearest_span);

        let mut indent = 0;

        if let Some(scope) = ctx.event_scope(event) {
            let spans_with_indent = [SpanType::StepCheck.as_str(), SpanType::Filter.as_str()];
            for span in scope.from_root() {
                if spans_with_indent.contains(&span.metadata().name()) {
                    indent += 1;
                }
            }
        }

        let pad = if indent > 0 {
            "|".to_owned() + &"  ".repeat(indent)
        } else {
            "".to_string()
        };
        let progress = {
            let width = total_steps.to_string().len();
            format!("[{:>width$}/{}]", current_step_num, total_steps)
        };

        let event_type = v
            .fields
            .get("event_type")
            .and_then(|v| EventType::from_str(v).ok())
            .unwrap_or(Unknown);
        match event_type {
            EventType::UserDecision => {
                _ = writeln!(
                    out,
                    "\n{progress} What do you want to do? ({}): ",
                    v.fields.get("options").unwrap_or(&"?".into())
                )
            }
            EventType::StepCheckStarted => {
                _ = writeln!(out, "[DEBUG] Checking step '{current_step_id}'")
            }
            EventType::StepCheckFinished => {
                _ = writeln!(
                    out,
                    "[DEBUG] Step check completed in {}",
                    duration_from_nearest_span_str
                )
            }
            EventType::ScriptStarted => {
                let kind = v.fields.get("kind").map(|s| s.as_str()).unwrap_or("?");
                _ = writeln!(out, "⚙️ {progress} Running {kind}...",)
            }
            EventType::ScriptCompleted => {
                let code = v
                    .fields
                    .get("code")
                    .unwrap_or(&"0".into())
                    .parse()
                    .unwrap_or(0);
                let kind = v.fields.get("kind").map(|s| s.as_str()).unwrap_or("?");
                let elapsed_secs = v.fields.get("elapsed_secs").unwrap().parse().unwrap_or(0.0);
                let duration =
                    format_duration(Duration::from_millis((elapsed_secs * 1000.0) as u64));
                if code > 0 {
                    _ = writeln!(
                        out,
                        "{pad}[DEBUG] {} of step '{}' exited with code {} in {}",
                        kind, current_step_id, code, duration
                    );
                } else {
                    _ = writeln!(
                        out,
                        "{pad}[DEBUG] {} of step '{}' succeeded in {}",
                        kind, current_step_id, duration
                    );
                }
            }
            EventType::CompletedStepSkipped => {
                _ = writeln!(
                    out,
                    "✅ {progress} Step '{current_step_id}' already completed, skipping"
                );
            }
            EventType::PackagesCheckCompleted => {
                _ = writeln!(
                    out,
                    "{pad}[DEBUG] Package checks completed in {}",
                    duration_from_nearest_span_str
                );
            }
            EventType::ScriptsCheckCompleted => {
                _ = writeln!(
                    out,
                    "[DEBUG] Checked {} scripts in {}\n-------",
                    v.fields.get("count").unwrap_or(&"?".into()),
                    duration_from_nearest_span_str
                )
            }
            EventType::FilterCompleted => {
                _ = writeln!(
                    out,
                    "[DEBUG] Step filtering completed in {}\n-------",
                    duration_from_nearest_span_str
                )
            }
            EventType::StepRunStarted => {
                _ = writeln!(out, "🚀 {progress} Running step '{current_step_id}'...")
            }
            EventType::StepRunFinished => {
                if duration_from_nearest_span >= Duration::from_secs(1) {
                    _ = writeln!(
                        out,
                        "✅ {progress} Step '{current_step_id}' completed in {duration_from_nearest_span_str}"
                    )
                } else {
                    _ = writeln!(out, "✅ {progress} Step '{current_step_id}' completed")
                }
            }
            EventType::PackagesInstallStarted => {
                _ = writeln!(
                    out,
                    "📦 {progress} Installing packages: {}",
                    v.fields.get("packages").unwrap_or(&"?".into())
                );
            }
            EventType::DryRunCompleted => {
                _ = writeln!(out, "[DEBUG] Dry-run completed in {duration_from_nearest_span_str}");
            }
            EventType::RunCompleted => {
                let interactive = v
                    .fields
                    .get("interactive")
                    .and_then(|s| s.parse::<bool>().ok())
                    .unwrap_or(false);
                if duration_from_nearest_span >= Duration::from_secs(1) && !interactive {
                    _ = writeln!(out, "✅ Run completed in {duration_from_nearest_span_str}")
                } else {
                    _ = writeln!(out, "✅ Run completed")
                }
            }
            Unknown => {
                let Some(msg) = v.fields.get("message") else {
                    return;
                };

                match level {
                    Level::WARN => _ = writeln!(out, "{} {}", "Warning:".yellow(), msg),
                    Level::DEBUG => _ = writeln!(out, "{pad}[DEBUG] {}", msg),
                    _ => _ = writeln!(out, "{}", msg),
                }
            },
        }
    }
}

fn fill_span_data<S>(event: &Event<'_>, ctx: &Context<'_, S>) -> (SpanCommonData, StepSpanData)
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    let mut common_data = SpanCommonData::default();
    let mut step_data = StepSpanData::default();

    let Some(scope) = ctx.event_scope(event) else {
        return (common_data, step_data);
    };

    let mut spans: Vec<_> = scope.from_root().collect();
    spans.reverse();

    if let Some(span) = spans
        .iter()
        .find(|s| s.extensions().get::<StepSpanData>().is_some())
    {
        let exts = span.extensions();
        let data = exts.get::<StepSpanData>().unwrap();

        step_data.step_id = data.step_id.clone();
        step_data.number = data.number;
        step_data.total_steps = data.total_steps;
    }

    if let Some(span) = spans
        .iter()
        .find(|s| s.extensions().get::<SpanCommonData>().is_some())
    {
        let exts = span.extensions();
        let data = exts.get::<SpanCommonData>().unwrap();
        common_data.start = data.start;
    }

    (common_data, step_data)
}

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();

    if secs < 0.001 {
        format!("{:.0} µs", secs * 1_000_000.0)
    } else if secs < 1.0 {
        let ms = secs * 1000.0;

        if ms.fract() < 0.05 {
            format!("{:.0} ms", ms)
        } else {
            format!("{:.1} ms", ms)
        }
    } else {
        format!("{:.2} s", secs)
    }
}

pub fn setup_tracing(debug: bool) {
    let filter = if debug {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    registry()
        .with(filter)
        .with(CustomLayer::new(std::io::stdout()))
        .init();
}
