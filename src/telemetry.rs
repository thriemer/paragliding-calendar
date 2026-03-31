use std::env;

use anyhow::Result;
use opentelemetry::global;
use opentelemetry_sdk::{Resource, metrics::SdkMeterProvider, trace::SdkTracerProvider};
use tracing_subscriber::{EnvFilter, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_telemetry() -> Result<()> {
    let otel_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    if otel_endpoint
        .as_ref()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
    {
        println!("Initializing OpenTelemetry for production");
        init_production_telemetry()?;
    } else {
        println!("Initializing stdout logging for development");
        init_development_logging();
    }

    Ok(())
}

fn init_production_telemetry() -> Result<()> {
    let _tracer_provider = init_tracing()?;
    let _meter_provider = init_metrics()?;

    let tracer = global::tracer("travelai");
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    Registry::default()
        .with(fmt::layer())
        .with(telemetry_layer)
        .init();

    Ok(())
}

fn init_development_logging() {
    Registry::default()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
}

fn init_tracing() -> Result<SdkTracerProvider> {
    let _otlp_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".to_string());

    let service_name = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "travelai".to_string());

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()?;

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(Resource::builder().with_service_name(service_name).build())
        .build();

    global::set_tracer_provider(tracer_provider.clone());

    Ok(tracer_provider)
}

fn init_metrics() -> Result<SdkMeterProvider> {
    let _otlp_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4318".to_string());

    let service_name = env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "travelai".to_string());

    let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_http()
        .build()?;

    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(Resource::builder().with_service_name(service_name).build())
        .build();

    global::set_meter_provider(meter_provider.clone());

    Ok(meter_provider)
}
