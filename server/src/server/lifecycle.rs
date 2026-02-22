use crate::server::instance::MassiveGameServer;
use std::future::pending;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

pub fn request_shutdown(server: &Arc<MassiveGameServer>) {
    server.request_shutdown();
}

pub fn is_shutdown_requested(server: &MassiveGameServer) -> bool {
    server.is_shutdown_requested()
}

pub async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => Some(stream),
            Err(err) => {
                warn!("Failed to register SIGTERM listener: {}", err);
                None
            }
        };
        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(stream) => Some(stream),
            Err(err) => {
                warn!("Failed to register SIGINT listener: {}", err);
                None
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "ctrl_c",
            _ = async {
                if let Some(sig) = terminate.as_mut() {
                    let _ = sig.recv().await;
                } else {
                    pending::<()>().await;
                }
            } => "sigterm",
            _ = async {
                if let Some(sig) = interrupt.as_mut() {
                    let _ = sig.recv().await;
                } else {
                    pending::<()>().await;
                }
            } => "sigint",
        }
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => "ctrl_c",
            Err(err) => {
                warn!("Failed to listen for shutdown signal: {}", err);
                "signal_error"
            }
        }
    }
}

pub async fn request_shutdown_on_signal(server: Arc<MassiveGameServer>) {
    let signal = wait_for_shutdown_signal().await;
    info!("Shutdown signal received via {}.", signal);
    request_shutdown(&server);
}

pub async fn drain_game_loop_with_timeout(
    game_loop_handle: &mut JoinHandle<()>,
    timeout: Duration,
) {
    match tokio::time::timeout(timeout, &mut *game_loop_handle).await {
        Ok(join_result) => {
            if let Err(err) = join_result {
                error!("Game loop task join failed: {}", err);
            }
        }
        Err(_) => {
            warn!(
                "Game loop did not stop within {}s; aborting task.",
                timeout.as_secs()
            );
            game_loop_handle.abort();
            match game_loop_handle.await {
                Ok(()) => {}
                Err(err) => warn!("Game loop abort join result: {}", err),
            }
        }
    }
}
