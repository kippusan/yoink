use tracing::warn;
use tracing_subscriber::EnvFilter;

pub(crate) fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let format = std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".to_string());
    let mut fallback_from = None;

    match format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .json()
                .with_current_span(false)
                .with_span_list(false)
                .with_target(true)
                .init();
        }
        "pretty" => {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .compact()
                .with_target(false)
                .init();
        }
        _ => {
            fallback_from = Some(format);
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .compact()
                .with_target(false)
                .init();
        }
    }

    if let Some(value) = fallback_from {
        warn!(
            provided = %value,
            "Invalid LOG_FORMAT, defaulting to pretty"
        );
    }
}
