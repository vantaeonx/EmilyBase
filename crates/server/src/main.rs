use emilybase_server::{Error, ProjectStore, router, serve};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().json().with_target(false).init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(error=%error,"startup_failed");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> emilybase_server::Result<()> {
    let master =
        std::env::var("EMILYBASE_MASTER_KEY").map_err(|_| Error::Config("master key required"))?;
    // Validate secrets before creating data directories; errors never contain their values.
    emilybase_auth::KeyDigest::from_token(&master)?;
    let root = std::env::var_os("EMILYBASE_DATA_DIR").unwrap_or_else(|| "emilybase-data".into());
    let listen = std::env::var("EMILYBASE_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:7000".into())
        .parse::<std::net::SocketAddr>()
        .map_err(|_| Error::Config("listen address"))?;
    let app = router(ProjectStore::open(root)?, &master)?;
    drop(master);
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(Error::Transport)?;
    tracing::info!(address=%listener.local_addr().map_err(Error::Transport)?,"experimental_server_listening");
    serve(listener, app, shutdown()).await
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=terminate.recv()=>{}}
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!("graceful_shutdown_requested");
}
