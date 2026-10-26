//! Map import and export in the legacy-compatible file format: downloading a map as a
//! JSON file, merging a file into an existing map, and creating a new map from one.
//!
//! The file travels as text inside a JSON body rather than as a multipart upload: the
//! client already has it in memory (it peeks inside to offer the section choices), and the
//! server validates the text the same way either way.

use super::extract::Credentials;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequest, Path, Query, Request, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::extract::{require_actor, require_role_on_map};
use super::{ApiError, ApiResult};
use crate::app::AppState;
use crate::maps::Role;
use crate::maps::transfer::{
    ImportSummary, SectionSet, TransferCounts, check_may_import_as_new, export_map, import_map,
    import_map_as_new, parse_export, transfer_counts,
};

/// Alliance-scale exports run to a few megabytes; comfortably above that, and still a
/// ceiling.
const IMPORT_BODY_LIMIT: usize = 32 * 1024 * 1024;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(counts))
        .routes(routes!(export))
        .routes(routes!(import))
        .routes(routes!(import_new))
        .layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT))
}

/// `GET /api/maps/{id}/transfer/counts`: how much of the map each section carries.
/// Manager+.
#[utoipa::path(
    get,
    path = "/api/maps/{id}/transfer/counts",
    tag = "transfer",
    params(("id" = i64, Path, description = "The map")),
    responses((status = 200, body = TransferCounts, description = "OK"), super::MapReadErrors),
)]
async fn counts(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
) -> ApiResult<TransferCounts> {
    let actor = require_actor(&state.db, &creds).await?;
    Ok(Json(transfer_counts(&state.db, actor, map_id).await?))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
struct ExportQuery {
    /// Comma-separated section names.
    sections: String,
}

/// `GET /api/maps/{id}/transfer/export?sections=...`: the selected sections as a JSON file
/// download. Manager+. A GET with a `content-disposition`, so the browser does the saving.
#[utoipa::path(
    get,
    path = "/api/maps/{id}/transfer/export",
    tag = "transfer",
    params(("id" = i64, Path, description = "The map"), ExportQuery),
    responses((status = 200, body = crate::maps::transfer::ExportFile, description = "The export, served as a file download"), (status = 400, response = super::BadRequest), super::MapReadErrors),
)]
async fn export(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let actor = require_actor(&state.db, &creds).await?;
    let sections = SectionSet::from_names(
        &query
            .sections
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
    )?;
    let payload = export_map(&state.db, actor, map_id, sections).await?;

    let filename = format!(
        "{}-export-{}.json",
        slug(&payload.map_name),
        payload.exported_at.format("%Y-%m-%d"),
    );
    let body = serde_json::to_string_pretty(&payload).map_err(|e| ApiError {
        status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        message: e.to_string(),
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        body,
    )
        .into_response())
}

/// The map name as a filename: lowercase, runs of anything else collapsed to one dash.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "map".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
struct ImportBody {
    sections: Vec<String>,
    /// The uploaded file, verbatim.
    content: String,
}

/// `POST /api/maps/{id}/transfer/import`: merge a file's selected sections into the map.
/// Manager+.
#[utoipa::path(
    post,
    path = "/api/maps/{id}/transfer/import",
    tag = "transfer",
    params(("id" = i64, Path, description = "The map")),
    request_body = ImportBody,
    responses((status = 200, body = ImportSummary, description = "OK"), super::MapWriteErrors),
)]
async fn import(
    State(state): State<AppState>,
    creds: Credentials,
    Path(map_id): Path<i64>,
    request: Request,
) -> ApiResult<ImportSummary> {
    let actor = require_role_on_map(&state, &creds, map_id, Role::Manager).await?;
    let parsed = decode(&state, request, |body: ImportBody| {
        let sections = SectionSet::from_names(&body.sections)?;
        Ok(parse_export(&body.content, sections, false)?)
    })
    .await?;
    Ok(Json(import_map(&state.db, actor, map_id, &parsed).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
struct ImportNewBody {
    /// Overrides the file's map name when present.
    #[serde(default)]
    name: Option<String>,
    sections: Vec<String>,
    content: String,
}

/// `POST /api/maps/transfer/import-new`: create a fresh map from a file, owned by the
/// acting character.
#[utoipa::path(
    post,
    path = "/api/maps/transfer/import-new",
    tag = "transfer",
    request_body = ImportNewBody,
    responses((status = 200, body = crate::maps::Map, description = "OK"), (status = 400, response = super::BadRequest), (status = 401, response = super::Unauthorized), (status = 403, response = super::Forbidden)),
)]
async fn import_new(
    State(state): State<AppState>,
    creds: Credentials,
    request: Request,
) -> ApiResult<crate::maps::Map> {
    let actor = require_actor(&state.db, &creds).await?;
    check_may_import_as_new(&state.db, actor, &state.map_creators).await?;
    let (parsed, name) = decode(&state, request, |body: ImportNewBody| {
        let sections = SectionSet::from_names(&body.sections)?;
        Ok((parse_export(&body.content, sections, true)?, body.name))
    })
    .await?;
    Ok(Json(
        import_map_as_new(&state.db, actor, &state.map_creators, parsed, name).await?,
    ))
}

/// Read an import body and turn it into what the import needs. Only called once the caller
/// has been let in, so a stranger cannot make the server buffer and decode 32 MB. Decoding
/// and parsing a file that size is CPU work, so it runs on the blocking pool rather than
/// holding an async worker.
async fn decode<B, T>(
    state: &AppState,
    request: Request,
    parse: impl FnOnce(B) -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError>
where
    B: DeserializeOwned,
    T: Send + 'static,
{
    let is_json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        .is_some_and(|v| v == "application/json" || v.ends_with("+json"));
    if !is_json {
        return Err(ApiError::bad_request(
            "Expected request with `Content-Type: application/json`",
        ));
    }
    // `Bytes` honors the router's `DefaultBodyLimit`, so the 32 MB ceiling still holds.
    let bytes = Bytes::from_request(request, state)
        .await
        .map_err(|rejection| ApiError {
            status: rejection.status(),
            message: rejection.body_text(),
        })?;
    tokio::task::spawn_blocking(move || {
        let body = serde_json::from_slice::<B>(&bytes).map_err(|e| {
            ApiError::bad_request(format!("Failed to deserialize the JSON body: {e}"))
        })?;
        parse(body)
    })
    .await
    .map_err(|e| ApiError {
        status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        message: e.to_string(),
    })?
}
