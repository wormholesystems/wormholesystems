//! The HTTP test harness: the real API router over the test pool, driven in-process with
//! `tower::ServiceExt::oneshot`. No network, no auth server; sessions are inserted
//! directly and carried as cookies.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header};
use sqlx::PgPool;
use tower::ServiceExt;
use wormholesystems::allowlist::AffiliationAllowlist;
use wormholesystems::app::AppState;
use wormholesystems::esi::sso::{Sso, SsoConfig};
use wormholesystems::maps::Actor;
use wormholesystems::server_status::{ServerState, ServerStatus, ServerWatch};

/// The real router with a test `AppState`. SSO and the server watch are stubs: nothing
/// under test touches EVE, but every route, extractor and error mapping is the production
/// one, the WebSockets included.
pub(crate) fn app(pool: &PgPool) -> Router {
    wormholesystems::app::router(state(pool))
}

/// The state the router runs on, for the code that takes it directly, such as the bot.
pub(crate) fn state(pool: &PgPool) -> AppState {
    let http = reqwest::Client::new();
    let sso = Sso::stub(
        http,
        SsoConfig {
            client_id: "test".into(),
            client_secret: "test".into(),
            redirect_uri: "http://localhost/auth/callback".into(),
            scopes: Vec::new(),
        },
    );
    AppState {
        sso: Arc::new(sso),
        tokens: Arc::new(wormholesystems::esi::TokenCache::new()),
        esi: wormholesystems::esi::EsiClient::new(),
        db: pool.clone(),
        user_hub: wormholesystems::realtime::UserHub::new(),
        server: ServerWatch::fixed(ServerStatus {
            state: ServerState::Online,
            players: 1,
            server_version: None,
            start_time: None,
            checked_at: None,
        }),
        discord: None,
        secure_cookies: false,
        backgrounds: wormholesystems::maps::BackgroundStore::new(
            std::env::temp_dir().join("ws-test-uploads"),
        ),
        allowed_logins: AffiliationAllowlist::default(),
        map_creators: AffiliationAllowlist::default(),
        announcement: None,
        eve_scout: Arc::new(wormholesystems::integrations::eve_scout::EveScout::new(
            "http://127.0.0.1:9/unreachable",
        )),
        sentry_tunnel: None,
    }
}

/// A signed-in session for `actor`, as the cookie header value the browser would send.
pub(crate) async fn session_cookie(pool: &PgPool, actor: Actor) -> String {
    let id = wormholesystems::session::create_session(pool, actor.user_id, actor.character_id)
        .await
        .expect("create_session");
    format!("ws_session={id}")
}

pub(crate) struct TestResponse {
    pub status: StatusCode,
    pub body: serde_json::Value,
    pub headers: header::HeaderMap,
}

impl TestResponse {
    /// Every `Set-Cookie` the response carries, as sent.
    pub(crate) fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().expect("header is ASCII").to_string())
            .collect()
    }

    pub(crate) fn location(&self) -> Option<&str> {
        self.headers
            .get(header::LOCATION)
            .map(|v| v.to_str().expect("header is ASCII"))
    }
}

/// Any request, for the ones `get` and `request_json` cannot build.
pub(crate) async fn send(app: Router, req: Request<Body>) -> TestResponse {
    let response: Response<Body> = app.oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    TestResponse {
        status,
        body,
        headers,
    }
}

pub(crate) async fn get(app: Router, path: &str, cookie: Option<&str>) -> TestResponse {
    let mut builder = Request::builder().uri(path);
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    send(app, builder.body(Body::empty()).expect("build request")).await
}

pub(crate) async fn request_json(
    app: Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: serde_json::Value,
) -> TestResponse {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    send(
        app,
        builder
            .body(Body::from(body.to_string()))
            .expect("build request"),
    )
    .await
}
