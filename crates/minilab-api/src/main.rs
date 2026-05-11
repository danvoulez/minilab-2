use minilab_api::{build_app, ApiConfig, AppState};
use minilab_store::StoreClient;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() {
    install_tracing();

    let config = ApiConfig::from_env().unwrap_or_else(|err| {
        tracing::error!(error = %err, "failed to load api config");
        std::process::exit(1);
    });
    let bind_addr = config.bind_addr;
    let state = AppState {
        store: StoreClient::from_env().unwrap_or_else(|err| {
            tracing::error!(error = %err, "failed to build store client");
            std::process::exit(1);
        }),
        config: std::sync::Arc::new(config),
    };
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, %bind_addr, "failed to bind http listener");
            std::process::exit(1);
        });

    tracing::info!(%bind_addr, "minilab-api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|err| {
            tracing::error!(error = %err, "http server failed");
            std::process::exit(1);
        });
}

fn install_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("minilab_api=info,tower_http=info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl-c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
