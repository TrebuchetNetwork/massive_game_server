// massive_game_server/server/src/operational/monitoring/tracing.rs

use anyhow::Context;
use opentelemetry::global;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace as sdktrace;
use opentelemetry_sdk::Resource;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_trace_id() -> u64 {
    TRACE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn with_trace_fields<R>(label: &str, f: impl FnOnce() -> R) -> R {
    let trace_id = next_trace_id();
    let span = tracing::info_span!("distributed_trace", trace_id, label = label);
    let _guard = span.enter();
    f()
}

struct HeaderMapExtractor<'a> {
    headers: &'a HashMap<String, String>,
}

impl<'a> Extractor for HeaderMapExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.keys().map(String::as_str).collect()
    }
}

struct HeaderMapInjector<'a> {
    headers: &'a mut HashMap<String, String>,
}

impl<'a> Injector for HeaderMapInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        self.headers.insert(key.to_ascii_lowercase(), value);
    }
}

pub fn extract_remote_context(
    traceparent: Option<&str>,
    tracestate: Option<&str>,
) -> opentelemetry::Context {
    let mut headers = HashMap::with_capacity(2);
    if let Some(value) = traceparent {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            headers.insert("traceparent".to_string(), trimmed.to_string());
        }
    }
    if let Some(value) = tracestate {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            headers.insert("tracestate".to_string(), trimmed.to_string());
        }
    }
    global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderMapExtractor { headers: &headers })
    })
}

pub fn inject_current_context_headers() -> HashMap<String, String> {
    let mut headers = HashMap::new();
    let cx = tracing_opentelemetry::OpenTelemetrySpanExt::context(&tracing::Span::current());
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(
            &cx,
            &mut HeaderMapInjector {
                headers: &mut headers,
            },
        );
    });
    headers
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

pub fn init_tracing_subscriber(default_filter: &str) -> anyhow::Result<()> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter.to_string().into());
    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_line_number(true);

    if env_flag("MGS_OTEL_ENABLED") {
        let otlp_endpoint = std::env::var("MGS_OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://127.0.0.1:4317".to_string());
        let otlp_timeout_ms = std::env::var("MGS_OTEL_EXPORTER_TIMEOUT_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(3000);

        let tracer_provider = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(otlp_endpoint.clone())
                    .with_timeout(Duration::from_millis(otlp_timeout_ms)),
            )
            .with_trace_config(
                sdktrace::Config::default().with_resource(Resource::new(vec![
                    KeyValue::new("service.name", "massive_game_server"),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                    KeyValue::new("service.namespace", "trebuchet"),
                ])),
            )
            .install_batch(Tokio)
            .context("failed to initialize OTLP tracing pipeline")?;

        let tracer = tracer_provider.tracer("massive_game_server_core");
        global::set_tracer_provider(tracer_provider);
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .with(otel_layer);
        tracing::subscriber::set_global_default(subscriber).map_err(|e| {
            anyhow::anyhow!("failed to set tracing subscriber with OpenTelemetry: {}", e)
        })?;

        tracing::info!(
            otlp_endpoint = %otlp_endpoint,
            timeout_ms = otlp_timeout_ms,
            "Tracing subscriber initialized with OTLP export."
        );
        return Ok(());
    }

    let subscriber = Registry::default().with(env_filter).with(fmt_layer);
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("failed to set tracing subscriber: {}", e))?;
    tracing::info!("Tracing subscriber initialized.");
    Ok(())
}
