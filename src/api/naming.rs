//! Naming: the alias a newly reached system gets and the bookmark names for holes, both
//! decided by the map's naming strategy. Reads, so anyone who can see the map may ask.

use axum::Json;
use axum::extract::{Path, Query, State};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::ApiResult;
use super::extract::{Credentials, ShareQuery, read_map_as, require_actor};
use crate::app::AppState;
use crate::maps::map::NamingUpdate;
use crate::maps::naming::{
    AliasSuggestion, BookmarkName, EndBookmark, NameBookmark, NamingPreview, SuggestAlias,
};

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(suggest_alias))
        .routes(routes!(name_bookmark))
        .routes(routes!(connection_bookmarks))
        .routes(routes!(preview_naming))
}

/// `GET /api/maps/{id}/naming/alias`: the alias for a system reached from `origin`.
#[utoipa::path(
    get,
    path = "/api/maps/{id}/naming/alias",
    tag = "naming",
    params(("id" = i64, Path, description = "The map"), SuggestAlias, ShareQuery),
    responses((status = 200, body = AliasSuggestion, description = "OK"), super::SharedReadErrors),
    security((), ("bearer" = []), ("session" = [])),
)]
pub async fn suggest_alias(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Query(cmd): Query<SuggestAlias>,
    Query(share): Query<ShareQuery>,
) -> ApiResult<AliasSuggestion> {
    let reader = read_map_as(&state, &creds, map_id, &share).await?;
    Ok(Json(
        crate::maps::naming::suggest_alias(&state.db, reader, map_id, cmd).await?,
    ))
}

/// `POST /api/maps/{id}/naming/bookmark`: the bookmark name for a hole. A read, posted only
/// because the question has more to it than fits a query string.
#[utoipa::path(
    post,
    path = "/api/maps/{id}/naming/bookmark",
    tag = "naming",
    params(("id" = i64, Path, description = "The map"), ShareQuery),
    request_body = NameBookmark,
    responses((status = 200, body = BookmarkName, description = "OK"), super::SharedReadErrors),
    security((), ("bearer" = []), ("session" = [])),
)]
pub async fn name_bookmark(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Query(share): Query<ShareQuery>,
    Json(cmd): Json<NameBookmark>,
) -> ApiResult<BookmarkName> {
    let reader = read_map_as(&state, &creds, map_id, &share).await?;
    Ok(Json(
        crate::maps::naming::name_bookmark(&state.db, reader, map_id, cmd, reader.actor).await?,
    ))
}

/// `GET /api/maps/{id}/connections/{connection_id}/bookmarks`: the bookmark for each end.
#[utoipa::path(
    get,
    path = "/api/maps/{id}/connections/{connection_id}/bookmarks",
    tag = "naming",
    params(
        ("id" = i64, Path, description = "The map"),
        ("connection_id" = i64, Path, description = "The connection"),
        ShareQuery,
    ),
    responses((status = 200, body = Vec<EndBookmark>, description = "OK"), super::SharedReadErrors),
    security((), ("bearer" = []), ("session" = [])),
)]
pub async fn connection_bookmarks(
    State(state): State<AppState>,
    creds: Credentials,
    Path((map_id, connection_id)): Path<(i64, i64)>,
    Query(share): Query<ShareQuery>,
) -> ApiResult<Vec<EndBookmark>> {
    let reader = read_map_as(&state, &creds, map_id, &share).await?;
    Ok(Json(
        crate::maps::naming::connection_bookmarks(&state.db, reader, map_id, connection_id).await?,
    ))
}

/// `POST /api/naming/preview`: what a set of naming settings would produce, for the
/// settings page to show while they are being edited.
#[utoipa::path(
    post,
    path = "/api/naming/preview",
    tag = "naming",
    request_body = NamingUpdate,
    responses((status = 200, body = NamingPreview, description = "OK"), super::AuthedErrors),
)]
pub async fn preview_naming(
    State(state): State<AppState>,
    creds: Credentials,
    Json(settings): Json<NamingUpdate>,
) -> ApiResult<NamingPreview> {
    require_actor(&state.db, &creds).await?;
    // A form that does not say which strategy is the chain's, as a map that never chose.
    let settings = settings.over(crate::maps::NamingStrategyKind::default());
    Ok(Json(crate::maps::naming::preview(&settings)))
}
