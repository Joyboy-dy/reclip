//! Remote MCP server for agents at `/mcp` (streamable HTTP transport).
//!
//! Every request must carry an API key (`Authorization: Bearer rck_...`); the
//! key's scopes and limits apply exactly as on the REST API. The server is
//! stateless: each POST is answered on its own with a JSON body, so there is
//! no session to bind to a key, and GET/DELETE answer 405.

mod tools;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use reclip_shared::ErrorCode;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

use crate::access::caller::{Caller, ON_BEHALF_OF, error, key_caller, unauthorized};
use crate::access::request::{bearer_key, no_store, request_origin};
use crate::app::AppState;

pub use tools::Tools;

/// Largest `/mcp` request body (a JSON-RPC call with a link and options).
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// The `/mcp` route, with its own key check (it is not behind `caller_layer`,
/// which would let cookie visitors in).
pub fn routes(state: AppState) -> Router<AppState> {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        // Tool arguments are tiny; do not rely on the library default.
        .with_max_request_body_bytes(MAX_BODY_BYTES)
        // A public server: the API key, not the Host header, is the gate.
        // Safe against DNS rebinding because only `Bearer` keys are accepted
        // here: no cookie or other ambient credential a browser would send.
        .disable_allowed_hosts()
        .with_cancellation_token(state.jobs.shutdown_token().child_token());
    let tools_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(Tools::new(tools_state.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    );
    Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn_with_state(state, mcp_auth))
}

/// Resolve the key's caller (401 without a valid key), then run the server.
async fn mcp_auth(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    let mut res = match authenticate(&s, &parts.headers).await {
        Err(res) => res,
        Ok(caller) => {
            let fwd = request_origin(&s, &parts);
            // Only accepted requests may teach the public URL.
            s.site.learn(&s.store, &fwd).await;
            parts.extensions.insert(fwd);
            parts.extensions.insert(caller);
            next.run(Request::from_parts(parts, body)).await
        }
    };
    no_store(&mut res);
    res
}

/// A key caller built exactly as `caller_layer` builds one. Acting for a
/// Telegram user is an API-only feature: refused here.
async fn authenticate(s: &AppState, headers: &axum::http::HeaderMap) -> Result<Caller, Response> {
    let Some(auth) = headers.get(AUTHORIZATION) else {
        return Err(unauthorized("An API key is required"));
    };
    let info = bearer_key(s, auth).await?;
    if headers.contains_key(ON_BEHALF_OF) {
        return Err(error(
            ErrorCode::Forbidden,
            "X-Reclip-On-Behalf-Of is not accepted on /mcp",
        ));
    }
    Ok(key_caller(&s.cfg, info))
}
