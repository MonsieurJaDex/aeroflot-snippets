use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logger(debug: bool) {
    match debug {
        true => {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "aeroflot_snippets=info,tower_http=debug".into()),
                )
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
        false => {
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "aeroflot_snippets=warn,tower_http=warn".into()),
                )
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
    }
}
