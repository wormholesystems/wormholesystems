use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::CookieJar;
use axum_extra::extract::cookie::{Cookie, SameSite};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::PgTokenStore;
use crate::esi::scopes::Scope;
use crate::esi::token::TokenStore;
use crate::oauth_flow::{self, FlowKind};
use crate::session;

#[derive(Deserialize)]
pub struct LoginQuery {
    /// `?link=true` adds the authenticated character to the currently signed-in user
    /// instead of resolving/creating an account.
    #[serde(default)]
    link: bool,
    /// Comma-separated ESI scopes to ask for on top of what the signed-in character has
    /// already granted. Unknown names are ignored rather than rejected.
    #[serde(default)]
    scopes: Option<String>,
    /// Where to land afterwards, so topping up permissions returns to the map that asked.
    #[serde(default)]
    return_to: Option<String>,
}

/// Only our own pages. An absolute URL, or a protocol-relative `//host` that a browser
/// would treat as one, would turn this into an open redirect.
fn safe_return_to(path: &str) -> Option<String> {
    let ok = path.starts_with('/') && !path.starts_with("//") && !path.contains('\\');
    ok.then(|| path.to_string())
}

/// `GET /auth/login`: record a one-time CSRF `state` and redirect to the EVE SSO. With
/// `?link=true` and an active session, the new character links to the current user;
/// `?scopes=` re-consents for more permissions on the character already signed in.
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<LoginQuery>,
) -> Response {
    let actor = match jar.get(session::SESSION_COOKIE) {
        Some(cookie) => session::actor_for_session(&state.db, cookie.value())
            .await
            .ok()
            .flatten(),
        None => None,
    };
    let link_user_id: Option<i64> = if query.link {
        actor.as_ref().map(|actor| actor.user_id)
    } else {
        None
    };

    // Everything the acting character already consented to, plus whatever was asked for.
    // SSO replaces the token wholesale, so leaving the existing scopes out of the request
    // would revoke them.
    let mut scopes: Vec<Scope> = Vec::new();
    if let (Some(requested), Some(actor)) = (query.scopes.as_deref(), actor.as_ref()) {
        match crate::identity::characters::granted_scopes(&state.db, actor.character_id).await {
            Ok(granted) => scopes.extend(granted),
            Err(err) => return server_error(err),
        }
        for name in requested.split(',') {
            if let Some(scope) = Scope::parse(name.trim())
                && !scopes.contains(&scope)
            {
                scopes.push(scope);
            }
        }
    }

    let return_to = query.return_to.as_deref().and_then(safe_return_to);
    let csrf = match oauth_flow::start(
        &state.db,
        FlowKind::EveLogin,
        link_user_id,
        return_to.as_deref(),
    )
    .await
    {
        Ok(csrf) => csrf,
        Err(err) => return server_error(err),
    };
    let url = if scopes.is_empty() {
        state.sso.authorize_url(&csrf)
    } else {
        state.sso.authorize_url_for(&csrf, &scopes)
    };
    let jar = jar.add(oauth_flow::browser_cookie(
        FlowKind::EveLogin,
        &csrf,
        state.secure_cookies,
    ));
    (jar, Redirect::to(&url)).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

/// `GET /auth/callback`: validate the handshake, persist the character and token, open a
/// session, set the cookie, and redirect into the app.
pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<CallbackQuery>,
) -> Response {
    // Only the browser that started this login may finish it. Checked before the state is
    // consumed, so a forged callback cannot burn somebody's login in flight either.
    if !oauth_flow::started_here(&jar, FlowKind::EveLogin, &query.state) {
        return (StatusCode::BAD_REQUEST, "invalid or expired state").into_response();
    }
    let cleared = oauth_flow::clear_cookie(jar.clone(), FlowKind::EveLogin);
    (cleared, finish_login(&state, jar, query).await).into_response()
}

async fn finish_login(state: &AppState, jar: CookieJar, query: CallbackQuery) -> Response {
    // The `state` is single-use and short-lived: consume it, rejecting unknown/expired.
    let flow = match oauth_flow::consume(&state.db, FlowKind::EveLogin, &query.state).await {
        Ok(Some(flow)) => flow,
        Ok(None) => return (StatusCode::BAD_REQUEST, "invalid or expired state").into_response(),
        Err(err) => return server_error(err),
    };
    if let Some(user_id) = flow.link_user_id
        && !oauth_flow::signed_in_as(&state.db, &jar, user_id).await
    {
        return (
            StatusCode::BAD_REQUEST,
            "this character was being added to an account you are not signed in to",
        )
            .into_response();
    }

    let (token, claims) = match state.sso.exchange_code(&query.code).await {
        Ok(pair) => pair,
        Err(err) => return upstream_error("EVE's login did not complete", err),
    };

    // Corp/alliance drive access checks, so they're required and refreshed on every login.
    // We also cache the entity rows (name/ticker) the character's deferred FKs reference.
    let affiliation = match state.esi.affiliation(&[claims.character_id]).await {
        Ok(found) => found.into_iter().next(),
        Err(err) => return upstream_error("could not resolve character affiliation", err),
    };
    let Some(affiliation) = affiliation else {
        return upstream_error(
            "could not resolve character affiliation",
            format!("ESI had no affiliation for {}", claims.character_id),
        );
    };

    // Before anything is stored: a refused character gets no account, no token and no
    // session, and is not added to the account it was being linked to either.
    if !state.allowed_logins.allows(
        claims.character_id,
        Some(affiliation.corporation_id),
        affiliation.alliance_id,
    ) {
        return Redirect::to(refused_destination(flow.link_user_id.is_some())).into_response();
    }

    let corp = match state.esi.corporation(affiliation.corporation_id).await {
        Ok(corp) => corp,
        Err(err) => return upstream_error("could not resolve corporation", err),
    };
    let corporation = session::Entity {
        id: affiliation.corporation_id,
        name: corp.name,
        ticker: corp.ticker,
    };
    let alliance = match affiliation.alliance_id {
        Some(alliance_id) => match state.esi.alliance(alliance_id).await {
            Ok(a) => Some(session::Entity {
                id: alliance_id,
                name: a.name,
                ticker: a.ticker,
            }),
            Err(err) => return upstream_error("could not resolve alliance", err),
        },
        None => None,
    };

    let user_id = match session::persist_identity(
        &state.db,
        &claims,
        corporation,
        alliance,
        flow.link_user_id,
    )
    .await
    {
        Ok(user_id) => user_id,
        Err(err) => return server_error(err),
    };

    // Persist the ESI token for this character (refresh token is the sensitive credential).
    if let Err(err) = PgTokenStore::new(state.db.clone())
        .save(claims.character_id, &token)
        .await
    {
        return server_error(err);
    }

    let current = jar.get(session::SESSION_COOKIE).map(|c| c.value());
    let session_id =
        match session::resume_or_create(&state.db, current, user_id, claims.character_id).await {
            Ok(id) => id,
            Err(err) => return server_error(err),
        };

    let cookie = session_cookie(session_id, state.secure_cookies);
    let destination = flow.redirect_to.unwrap_or_else(|| "/".to_string());
    (jar.add(cookie), Redirect::to(&destination)).into_response()
}

/// The cookie that carries a session. It has a lifetime so it outlasts a browser restart:
/// without one it is a browser-session cookie, gone whenever the browser (or a phone's
/// tab) closes.
pub fn session_cookie(session_id: String, secure: bool) -> Cookie<'static> {
    Cookie::build((session::SESSION_COOKIE, session_id))
        .path("/")
        .max_age(time::Duration::days(session::SESSION_DAYS))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .build()
}

/// Where a character the instance does not let in is sent, with a flag the page turns into
/// an explanation. Adding an alt starts from the characters page and returns there; a
/// plain login has no page of its own to go back to but the login page.
fn refused_destination(linking: bool) -> &'static str {
    if linking {
        "/settings/characters?refused=1"
    } else {
        "/login?refused=1"
    }
}

/// `GET /auth/logout`: end the session and clear the cookie.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(session::SESSION_COOKIE) {
        let _ = session::delete_session(&state.db, cookie.value()).await;
    }
    (
        // The path has to match the one it was set with, or the browser keeps the cookie.
        jar.remove(Cookie::build(session::SESSION_COOKIE).path("/")),
        Redirect::to("/"),
    )
        .into_response()
}

/// A failure on our side. The detail is for the log: a database error describes the schema,
/// and the person mid-login can do nothing with it.
/// EVE or ESI let the login down. The detail (status codes, response bodies, URLs) is for
/// the log; the person signing in gets what failed and that trying again may work.
fn upstream_error(what: &str, err: impl std::fmt::Display) -> Response {
    tracing::warn!("login failed: {what}: {err}");
    (StatusCode::BAD_GATEWAY, format!("{what}, please try again")).into_response()
}

fn server_error(err: impl std::fmt::Display) -> Response {
    tracing::error!("login failed: {err}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "login failed on our side, please try again",
    )
        .into_response()
}
