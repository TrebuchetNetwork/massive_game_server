use crate::server::instance::MassiveGameServer;
use std::sync::Arc;

pub fn request_shutdown(server: &Arc<MassiveGameServer>) {
    server.request_shutdown();
}

pub fn is_shutdown_requested(server: &MassiveGameServer) -> bool {
    server.is_shutdown_requested()
}
