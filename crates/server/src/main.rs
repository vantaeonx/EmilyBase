use emilybase_server::{AccountRoot, Error, ProjectStore, account_router, router, serve};
use std::process::ExitCode;
mod config;
mod selection;
use selection::{AccountLayout, DataSelection};

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
    let selected = DataSelection::from_environment().map_err(Error::Config)?;
    // Bounded secret-file reads run off the reactor, before any data creation.
    let master = tokio::task::spawn_blocking(config::load_master)
        .await
        .map_err(|_| Error::Config("configuration worker failed"))??;
    let listen = std::env::var("EMILYBASE_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:7000".into())
        .parse::<std::net::SocketAddr>()
        .map_err(|_| Error::Config("listen address"))?;
    let app = tokio::task::spawn_blocking(move || match selected {
        DataSelection::Accounts { path, layout } => {
            let pool = emilybase_auth::password::PasswordPool::new(1)
                .map_err(|_| Error::Config("password workspace"))?;
            let root = match layout {
                AccountLayout::V1 => AccountRoot::open(path, pool)?,
                AccountLayout::V2 => AccountRoot::open_v2(path, pool)?,
            };
            account_router(root, &master)
        }
        DataSelection::Registry(path) => router(ProjectStore::open(path)?, &master),
    })
    .await
    .map_err(|_| Error::Config("startup worker failed"))??;
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
