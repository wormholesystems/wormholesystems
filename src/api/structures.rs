//! The structures a map's members share with it: recent attacks and fuel warnings, and
//! the fuel timers. What is shared and why is [`crate::maps::structures`].

use axum::Json;
use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::ApiResult;
use super::extract::Credentials;
use crate::app::AppState;
use crate::maps::Role;
use crate::maps::structures::MapStructures;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(map_structures))
}

/// `GET /api/maps/{id}/structures`: what happened to the shared structures this week, and
/// how long their fuel lasts. Member+: when a structure runs dry is not for someone
/// handed a link, and a viewer is one grant away from that.
#[utoipa::path(
    get,
    path = "/api/maps/{id}/structures",
    tag = "structures",
    params(("id" = i64, Path, description = "The map")),
    responses((status = 200, body = MapStructures, description = "OK"), super::MapReadErrors),
)]
pub async fn map_structures(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
) -> ApiResult<MapStructures> {
    super::extract::require_role_on_map(&state, &creds, map_id, Role::Member).await?;
    Ok(Json(
        crate::maps::structures::for_map(
            &state.db,
            super::extract::map_of(&state.db, map_id).await?,
        )
        .await?,
    ))
}
