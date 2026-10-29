//! Recording a jump a pilot has made, which places, connects and links in one step, and the
//! desktop app saying which pilots it records them for.

use super::extract::Credentials;
use axum::Json;
use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::ApiResult;
use super::extract::{acting_on, require_actor};
use crate::app::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(track_jump))
        .routes(routes!(desktop_heartbeat))
}

/// `POST /api/maps/{id}/track-jump`, record a jump: place the system, connect it, and
/// link the signature it turned out to be. Member+. One command, so it undoes as one step;
/// it can touch a system, a connection and a signature at once, so clients just refetch.
#[utoipa::path(
    post,
    path = "/api/maps/{id}/track-jump",
    tag = "tracking",
    params(("id" = i64, Path, description = "The map")),
    request_body = crate::maps::tracking::TrackJump,
    responses((status = 200, description = "Done; the body is `null`"), super::MapWriteErrors),
)]
pub async fn track_jump(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Json(cmd): Json<crate::maps::tracking::TrackJump>,
) -> ApiResult<()> {
    let actor = acting_on(&state.db, &creds, map_id, cmd.map_id).await?;
    crate::maps::tracking::track_jump(&state.db, actor, cmd).await?;
    Ok(Json(()))
}

/// `POST /api/desktop/heartbeat`, a desktop app saying which characters it maps jumps for,
/// so the website leaves those to it. Sent every 30 seconds, and once with `active: false`
/// when it stops.
#[utoipa::path(
    post,
    path = "/api/desktop/heartbeat",
    tag = "tracking",
    request_body = crate::pilots::desktop::DesktopHeartbeat,
    responses((status = 200, description = "Done; the body is `null`"), super::AuthedErrors),
)]
pub async fn desktop_heartbeat(
    State(state): State<AppState>,
    creds: Credentials,
    Json(beat): Json<crate::pilots::desktop::DesktopHeartbeat>,
) -> ApiResult<()> {
    let actor = require_actor(&state.db, &creds).await?;
    crate::pilots::desktop::heartbeat(&state.db, &state.user_hub, actor.user_id, beat).await?;
    Ok(Json(()))
}
