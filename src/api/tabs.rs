//! A map's tabs. Each is a map id in its own right, so what is drawn on one is read and
//! edited through the ordinary map routes under the tab's id; these only manage the bar.

use super::extract::Credentials;
use axum::Json;
use axum::extract::{Path, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::ApiResult;
use super::extract::acting_on;
use crate::app::AppState;
use crate::maps::tabs::{CreateTab, DeleteTab, MapTab, RenameTab, ReorderTabs};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_tab))
        .routes(routes!(rename_tab))
        .routes(routes!(reorder_tabs))
        .routes(routes!(delete_tab))
}

/// `POST /api/maps/{id}/tabs/create`, add an empty tab at the end of the bar. `{id}` is
/// the map or any of its tabs. The new tab's `id` is a map id: read it with
/// `GET /api/maps/{id}` and edit it through the same routes as any map. `Conflict` unless
/// the operator switched tabs on for the map (`TABS_MAP_IDS`).
#[utoipa::path(
    post,
    path = "/api/maps/{id}/tabs/create",
    tag = "tabs",
    params(("id" = i64, Path, description = "The map, or one of its tabs")),
    request_body = CreateTab,
    responses((status = 200, body = MapTab, description = "OK"), super::MapWriteErrors),
)]
pub async fn create_tab(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Json(cmd): Json<CreateTab>,
) -> ApiResult<MapTab> {
    let actor = acting_on(&state.db, &creds, map_id, cmd.map_id).await?;
    // The operator's switch. Checked after the actor, so it says nothing about a map to
    // somebody who could not see it.
    let root = crate::maps::tabs::root_of(&state.db, cmd.map_id).await?;
    if !state.tabs.allows(root) {
        return Err(crate::maps::MapError::Conflict(
            "tabs are not switched on for this map".into(),
        )
        .into());
    }
    Ok(Json(
        crate::maps::tabs::create_tab(&state.db, actor, cmd).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/maps/{id}/tabs/rename",
    tag = "tabs",
    params(("id" = i64, Path, description = "The tab")),
    request_body = RenameTab,
    responses((status = 200, body = MapTab, description = "OK"), super::MapWriteErrors),
)]
pub async fn rename_tab(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Json(cmd): Json<RenameTab>,
) -> ApiResult<MapTab> {
    let actor = acting_on(&state.db, &creds, map_id, cmd.map_id).await?;
    Ok(Json(
        crate::maps::tabs::rename_tab(&state.db, actor, cmd).await?,
    ))
}

/// `POST /api/maps/{id}/tabs/reorder`, put the tabs in a new order. The body names each
/// of the map's tabs once.
#[utoipa::path(
    post,
    path = "/api/maps/{id}/tabs/reorder",
    tag = "tabs",
    params(("id" = i64, Path, description = "The map, or one of its tabs")),
    request_body = ReorderTabs,
    responses((status = 200, body = Vec<MapTab>, description = "OK"), super::MapWriteErrors),
)]
pub async fn reorder_tabs(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Json(cmd): Json<ReorderTabs>,
) -> ApiResult<Vec<MapTab>> {
    let actor = acting_on(&state.db, &creds, map_id, cmd.map_id).await?;
    Ok(Json(
        crate::maps::tabs::reorder_tabs(&state.db, actor, cmd).await?,
    ))
}

/// `POST /api/maps/{id}/tabs/delete`, delete a tab and everything drawn on it. The map's
/// own first tab cannot be deleted this way: it is the map.
#[utoipa::path(
    post,
    path = "/api/maps/{id}/tabs/delete",
    tag = "tabs",
    params(("id" = i64, Path, description = "The tab")),
    request_body = DeleteTab,
    responses((status = 200, description = "Done; the body is `null`"), super::MapWriteErrors),
)]
pub async fn delete_tab(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Json(cmd): Json<DeleteTab>,
) -> ApiResult<()> {
    let actor = acting_on(&state.db, &creds, map_id, cmd.map_id).await?;
    crate::maps::tabs::delete_tab(&state.db, actor, cmd).await?;
    Ok(Json(()))
}
