//! The remote MCP server at `/mcp`, driven by a minimal JSON-RPC client.

mod common;

use std::time::{Duration, Instant};

use common::{ON_BEHALF_OF, TestServer};
use reclip_shared::Scope;
use reclip_shared::tools::tool_defs;
use reqwest::{Client, RequestBuilder};
use serde_json::{Value, json};

const PROTOCOL: &str = "2025-06-18";

/// A JSON-RPC client for `/mcp`, holding one API key (or none).
struct Mcp {
    url: String,
    key: Option<String>,
    client: Client,
}

impl Mcp {
    fn new(s: &TestServer, key: Option<&str>) -> Mcp {
        Mcp {
            url: format!("{}/mcp", s.base),
            key: key.map(String::from),
            client: Client::new(),
        }
    }

    async fn agent(s: &TestServer, scopes: &[Scope]) -> Mcp {
        Mcp::new(s, Some(&s.key(Some(scopes.to_vec())).await))
    }

    fn post(&self, method: &str, params: Value) -> RequestBuilder {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let mut req = self
            .client
            .post(&self.url)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", PROTOCOL)
            .json(&body);
        if let Some(key) = &self.key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// The JSON-RPC `result` of `method` (asserting a 200 and no error).
    async fn rpc(&self, method: &str, params: Value) -> Value {
        let r = self.post(method, params).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let mut body: Value = r.json().await.unwrap();
        assert!(body["error"].is_null(), "{body}");
        body["result"].take()
    }

    /// `tools/call`: `Ok(structuredContent)`, or `Err` of the error object.
    async fn tool(&self, name: &str, args: Value) -> Result<Value, Value> {
        let params = json!({"name": name, "arguments": args});
        let mut res = self.rpc("tools/call", params).await;
        let structured = res["structuredContent"].take();
        // The text block carries the same JSON for clients without
        // structured content.
        let text: Value =
            serde_json::from_str(res["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, structured);
        match res["isError"].as_bool() {
            Some(true) => Err(structured),
            _ => Ok(structured),
        }
    }

    async fn error_code(&self, name: &str, args: Value) -> String {
        let e = self.tool(name, args).await.unwrap_err();
        e["code"].as_str().unwrap().to_string()
    }

    async fn start(&self, url: &str) -> String {
        let job = self
            .tool("start_download", json!({"url": url}))
            .await
            .unwrap();
        assert_eq!(job["state"], "queued");
        job["id"].as_str().unwrap().to_string()
    }

    async fn wait_done(&self, id: &str) -> Value {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            let job = self.tool("get_download", json!({"id": id})).await.unwrap();
            if job["state"] == "done" {
                return job;
            }
            assert!(job.get("download_url").is_none(), "{job}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("download never finished");
    }
}

async fn expect_status(req: RequestBuilder, status: u16, code: &str) {
    let r = req.send().await.unwrap();
    assert_eq!(r.status(), status);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], code, "{b}");
}

#[tokio::test]
async fn mcp_requires_a_key() {
    let s = TestServer::start(|_| {}).await;
    let anon = Mcp::new(&s, None);
    expect_status(anon.post("tools/list", json!({})), 401, "unauthorized").await;
    // A visitor's cookie and Origin do not open /mcp.
    let visitor = s
        .visitor
        .client
        .post(format!("{}/mcp", s.base))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}));
    expect_status(visitor, 401, "unauthorized").await;
    let bad = Mcp::new(&s, Some("rck_00000000_nope"));
    expect_status(bad.post("tools/list", json!({})), 401, "unauthorized").await;
    let basic = anon
        .post("tools/list", json!({}))
        .basic_auth("user", Some("pass"));
    expect_status(basic, 401, "unauthorized").await;
    // Acting for a Telegram user is API-only, even for a delegate key.
    let key = s.key(Some(vec![Scope::JobsRead, Scope::Delegate])).await;
    let delegated = Mcp::new(&s, Some(&key))
        .post("tools/list", json!({}))
        .header(ON_BEHALF_OF, "tg:42");
    expect_status(delegated, 403, "forbidden").await;
    // Stateless server: no session stream to open or close.
    let get = s.visitor.client.get(format!("{}/mcp", s.base));
    assert_eq!(get.bearer_auth(&key).send().await.unwrap().status(), 405);
}

#[tokio::test]
async fn tools_list_matches_shared_defs() {
    let s = TestServer::start(|_| {}).await;
    let mcp = Mcp::agent(&s, &[Scope::Probe]).await;
    let init = mcp
        .rpc(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1"}
            }),
        )
        .await;
    assert_eq!(init["serverInfo"]["name"], "reclip");
    assert!(init["capabilities"]["tools"].is_object(), "{init}");
    let listed = mcp.rpc("tools/list", json!({})).await;
    let tools: Vec<Value> = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| json!({"name": t["name"], "description": t["description"], "inputSchema": t["inputSchema"]}))
        .collect();
    assert_eq!(
        Value::Array(tools),
        serde_json::to_value(tool_defs()).unwrap()
    );
    let r = mcp.post("tools/call", json!({"name": "nope", "arguments": {}}));
    let body: Value = r.send().await.unwrap().json().await.unwrap();
    assert_eq!(body["error"]["code"], -32602, "{body}");
}

#[tokio::test]
async fn start_and_get_download_with_signed_link() {
    let s = TestServer::start(|_| {}).await;
    let mcp = Mcp::agent(&s, &[Scope::JobsCreate, Scope::JobsRead]).await;
    let id = mcp.start("https://ok").await;
    let job = mcp.wait_done(&id).await;
    let link = job["download_url"].as_str().unwrap();
    assert!(link.starts_with(&format!("{}/d/{id}.", s.base)), "{link}");
    let r = Client::new().get(link).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(!r.bytes().await.unwrap().is_empty());
    let list = mcp.tool("list_downloads", json!({})).await.unwrap();
    assert_eq!(list["jobs"][0]["id"], id.as_str());
    // Bad arguments are a tool error with the API's code.
    let code = mcp.error_code("get_download", json!({"job": id})).await;
    assert_eq!(code, "invalid_request");
}

#[tokio::test]
async fn other_keys_jobs_are_not_found() {
    let s = TestServer::start(|_| {}).await;
    let all = [Scope::JobsCreate, Scope::JobsRead, Scope::JobsCancel];
    let (alice, bob) = (Mcp::agent(&s, &all).await, Mcp::agent(&s, &all).await);
    let id = alice.start("https://slow.example/1").await;
    for tool in ["get_download", "cancel_download"] {
        let e = bob.tool(tool, json!({"id": id})).await.unwrap_err();
        assert_eq!(
            e,
            json!({"code": "invalid_request", "message": "Job not found"})
        );
    }
    let list = bob.tool("list_downloads", json!({})).await.unwrap();
    assert_eq!(list["jobs"], json!([]));
    // Bob's attempt left it alone: the owner still cancels it.
    let job = alice
        .tool("cancel_download", json!({"id": id}))
        .await
        .unwrap();
    assert_eq!(job["state"], "cancelled");
}

#[tokio::test]
async fn mcp_respects_scopes_and_quotas() {
    let s = TestServer::start(|c| c.limits.key.jobs_per_hour = 1).await;
    let reader = Mcp::agent(&s, &[Scope::JobsRead]).await;
    let code = reader
        .error_code("start_download", json!({"url": "https://ok"}))
        .await;
    assert_eq!(code, "forbidden");
    let code = reader
        .error_code("probe_media", json!({"url": "https://ok"}))
        .await;
    assert_eq!(code, "forbidden");
    let code = reader
        .error_code("get_transcript", json!({"url": "https://ok"}))
        .await;
    assert_eq!(code, "forbidden");

    let mcp = Mcp::agent(&s, &[Scope::JobsCreate, Scope::Probe]).await;
    let private = json!({"url": "http://127.0.0.1/"});
    let code = mcp.error_code("start_download", private.clone()).await;
    assert_eq!(code, "forbidden_target");
    let code = mcp.error_code("probe_media", private).await;
    assert_eq!(code, "forbidden_target");
    let probed = mcp.tool("probe_media", json!({"url": "https://ok"})).await;
    assert!(probed.is_ok(), "{probed:?}");
    // The refused target used no slot: one creation, then the hourly cap.
    mcp.start("https://slow.example/1").await;
    let code = mcp
        .error_code("start_download", json!({"url": "https://ok"}))
        .await;
    assert_eq!(code, "rate_limited");
    let code = mcp.error_code("list_downloads", json!({})).await;
    assert_eq!(code, "forbidden");
}

#[tokio::test]
async fn transcript_tool_returns_text() {
    let s = TestServer::start(|_| {}).await;
    let mcp = Mcp::agent(&s, &[Scope::Probe]).await;
    let t = mcp
        .tool(
            "get_transcript",
            json!({"url": "https://subs-auto.example/v"}),
        )
        .await
        .unwrap();
    assert_eq!(t["automatic"], true);
    assert_eq!(t["text"], "hello world\nsecond part");
    let args = json!({"url": "https://subs-manual.example/v", "language": "en.*;rm"});
    assert_eq!(
        mcp.error_code("get_transcript", args).await,
        "invalid_request"
    );
}
