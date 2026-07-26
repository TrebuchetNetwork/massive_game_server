// massive_game_server/server/src/routes/master_map.rs

use crate::world::master_map::MasterMap;
use parking_lot::RwLock;
use std::sync::Arc;
use warp::Filter;

/// Public read-only endpoint serving the assembled MasterMap so clients and
/// neighboring regions can discover grid layout, tile size, and map seed.
pub fn build_master_map_route(
    master_map: Arc<RwLock<MasterMap>>,
) -> impl Filter<Extract = (warp::reply::Response,), Error = warp::Rejection> + Clone {
    warp::path("api")
        .and(warp::path("master_map"))
        .and(warp::path::end())
        .and(warp::get())
        .map(move || {
            let map = master_map.read().clone();
            warp::reply::Reply::into_response(warp::reply::json(&map))
        })
}
