use std::env;

use anyhow::Result;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::PeriodicReader;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

pub fn init_telemetry() -> Result<()> {
    let otel_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    let service_name = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "travelai".to_string());

    if otel_endpoint
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        println!("Initializing OpenTelemetry for production");
        init_production_telemetry(otel_endpoint.unwrap(), service_name)?;
    } else {
        println!("Initializing stdout logging for development");
        init_development_logging();
    }

    Ok(())
}

fn init_production_telemetry(otel_endpoint: String, service_name: String) -> Result<()> {
    let resource = Resource::builder()
        .with_service_name(service_name.clone())
        .build();

    // Trace exporter
    let http_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(otel_endpoint.clone())
        .with_protocol(Protocol::HttpJson)
        .build()?;

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(http_exporter)
        .with_resource(resource.clone())
        .build();

    let tracer = tracer_provider.tracer(service_name);
    global::set_tracer_provider(tracer_provider);

    // Metrics exporter
    let metrics_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .with_endpoint(otel_endpoint.clone())
        .with_protocol(Protocol::HttpJson)
        .build()?;

    let meter_provider = opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(metrics_exporter).build())
        .with_resource(resource.clone())
        .build();
    global::set_meter_provider(meter_provider);

    // Logs exporter (if supported)
    let logs_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .with_endpoint(otel_endpoint)
        .with_protocol(Protocol::HttpJson)
        .build()?;

    let logger_provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
        .with_batch_exporter(logs_exporter)
        .with_resource(resource)
        .build();
    // Create a new OpenTelemetryTracingBridge using the above LoggerProvider.
    let otel_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    // To prevent a telemetry-induced-telemetry loop, OpenTelemetry's own internal
    // logging is properly suppressed. However, logs emitted by external components
    // (such as reqwest, tonic, etc.) are not suppressed as they do not propagate
    // OpenTelemetry context. Until this issue is addressed
    // (https://github.com/open-telemetry/opentelemetry-rust/issues/2877),
    // filtering like this is the best way to suppress such logs.
    //
    // The filter levels are set as follows:
    // - Allow `info` level and above by default.
    // - Completely restrict logs from `hyper`, `tonic`, `h2`, and `reqwest`.
    //
    // Note: This filtering will also drop logs from these components even when
    // they are used outside of the OTLP Exporter.
    let filter_otel = EnvFilter::new("info")
        .add_directive("hyper=off".parse().unwrap())
        .add_directive("tonic=off".parse().unwrap())
        .add_directive("h2=off".parse().unwrap())
        .add_directive("reqwest=off".parse().unwrap());
    let otel_layer = otel_layer.with_filter(filter_otel);

    // Create a new tracing::Fmt layer to print the logs to stdout. It has a
    // default filter of `info` level and above, and `debug` and above for logs
    // from OpenTelemetry crates. The filter levels can be customized as needed.
    let filter_fmt = EnvFilter::new("info").add_directive("opentelemetry=info".parse().unwrap());
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_thread_names(true)
        .with_filter(filter_fmt);

    // Initialize the tracing subscriber with the OpenTelemetry layer and the
    // Fmt layer.
    tracing_subscriber::registry()
        .with(otel_layer)
        .with(fmt_layer)
        .init();
    Ok(())
}

fn init_development_logging() {
    tracing_subscriber::fmt()
        .event_format(
            tracing_subscriber::fmt::format()
                .with_file(true)
                .with_line_number(true),
        )
        .init();
}
