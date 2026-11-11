use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn init_logger(debug: bool) {
    let default_filter = if debug {
        "aeroflot_snippets=info,tower_http=debug"
    } else {
        "aeroflot_snippets=warn,tower_http=warn"
    };

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());

    let fmt_layer: Box<dyn Layer<_> + Send + Sync> = if debug {
        tracing_subscriber::fmt::layer().boxed()
    } else {
        tracing_subscriber::fmt::layer().json().boxed()
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .init();
}
