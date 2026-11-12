//! The MCP handler: lists the shared tool definitions and runs each tool
//! through the same service functions as the REST routes.

use std::sync::Arc;

use axum::http::request::Parts;
use reclip_shared::tools::tool_defs;
use reclip_shared::{
    ApiError, Clip, CreateJobRequest, ErrorCode, JobList, JobState, MediaFormat, ProbeRequest,
    TranscriptRequest,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::access::caller::Caller;
use crate::access::ip::Forwarded;
use crate::app::AppState;
use crate::http::links::{base_url, signed_link};
use crate::http::{api_error, internal};
use crate::service::{self, manager_api_error};
use crate::transcript::transcript;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UrlArgs {
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartArgs {
    url: String,
    #[serde(default)]
    format: Option<MediaFormat>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    clip: Option<Clip>,
    #[serde(default)]
    strip_metadata: bool,
    #[serde(default)]
    playlist_item: Option<u32>,
}

impl From<StartArgs> for CreateJobRequest {
    fn from(a: StartArgs) -> Self {
        CreateJobRequest {
            url: a.url,
            format: a.format.unwrap_or(MediaFormat::Mp4),
            format_id: a.quality,
            title: None,
            thumbnail: None,
            clip: a.clip,
            strip_metadata: a.strip_metadata,
            playlist_item: a.playlist_item,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranscriptArgs {
    url: String,
    #[serde(default)]
    language: Option<String>,
}

/// One tool call's outcome: a JSON object, or the API's `{code, message}`.
type Outcome = Result<Value, ApiError>;

fn json(v: impl serde::Serialize) -> Outcome {
    serde_json::to_value(v).map_err(|e| internal(e).0)
}

fn args<T: DeserializeOwned>(raw: Map<String, Value>) -> Result<T, ApiError> {
    serde_json::from_value(Value::Object(raw)).map_err(|e| {
        api_error(
            ErrorCode::InvalidRequest,
            &format!("Invalid arguments: {e}"),
        )
    })
}

/// One tool call, for the caller `mcp_auth` resolved.
struct Call<'a> {
    s: &'a AppState,
    caller: &'a Caller,
    fwd: &'a Forwarded,
}

impl Call<'_> {
    /// `None` for a tool that does not exist.
    async fn run(&self, name: &str, raw: Map<String, Value>) -> Option<Outcome> {
        let out = match name {
            "probe_media" => self.probe(raw).await,
            "start_download" => self.start(raw).await,
            "list_downloads" => self.list(raw).await,
            "get_download" => self.get(raw).await,
            "cancel_download" => self.cancel(raw).await,
            "get_transcript" => self.transcript(raw).await,
            _ => return None,
        };
        Some(out)
    }

    async fn probe(&self, raw: Map<String, Value>) -> Outcome {
        let UrlArgs { url } = args(raw)?;
        let (probed, _) = service::probe(self.s, self.caller, ProbeRequest { url })
            .await
            .map_err(|e| e.api_error())?;
        json(probed)
    }

    async fn start(&self, raw: Map<String, Value>) -> Outcome {
        let req = args::<StartArgs>(raw)?.into();
        let (job, _) = service::create_job(self.s, self.caller, req)
            .await
            .map_err(|e| e.api_error())?;
        json(job)
    }

    async fn list(&self, raw: Map<String, Value>) -> Outcome {
        let NoArgs {} = args(raw)?;
        let jobs = service::list_jobs(self.s, self.caller).await?;
        json(JobList { jobs })
    }

    /// The job, plus an absolute signed `download_url` once it is done.
    async fn get(&self, raw: Map<String, Value>) -> Outcome {
        let IdArgs { id } = args(raw)?;
        let job = service::get_job(self.s, self.caller, &id)
            .await
            .map_err(manager_api_error)?;
        let link = match job.state {
            JobState::Done => Some(signed_link(self.s, &base_url(self.s, self.fwd), &job)?),
            _ => None,
        };
        let mut out = json(job)?;
        if let (Some(link), Value::Object(map)) = (link, &mut out) {
            map.insert("download_url".into(), Value::String(link.url));
        }
        Ok(out)
    }

    async fn cancel(&self, raw: Map<String, Value>) -> Outcome {
        let IdArgs { id } = args(raw)?;
        let job = service::cancel_job(self.s, self.caller, &id)
            .await
            .map_err(manager_api_error)?;
        json(job)
    }

    async fn transcript(&self, raw: Map<String, Value>) -> Outcome {
        let TranscriptArgs { url, language } = args(raw)?;
        let (found, _) = transcript(self.s, self.caller, TranscriptRequest { url, language })
            .await
            .map_err(|e| e.api_error())?;
        json(found)
    }
}

/// The shared definitions, as MCP tools.
fn tool_list() -> Vec<Tool> {
    tool_defs()
        .into_iter()
        .map(|d| {
            let schema = match d.input_schema {
                Value::Object(m) => m,
                _ => Map::new(),
            };
            Tool::new(d.name, d.description, Arc::new(schema))
        })
        .collect()
}

/// The MCP server handler; one is built per request (stateless transport).
#[derive(Clone)]
pub struct Tools {
    state: AppState,
}

impl Tools {
    pub fn new(state: AppState) -> Tools {
        Tools { state }
    }
}

impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("reclip", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Download videos and audio, or read their transcripts. Probe a link first, \
                 start a download, then poll get_download until it is done to get the file link.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tool_list()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool_list().into_iter().find(|t| t.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        // Inserted by `mcp_auth`, carried by the transport: missing is a bug.
        let parts = context.extensions.get::<Parts>();
        let caller = parts.and_then(|p| p.extensions.get::<Caller>());
        let fwd = parts.and_then(|p| p.extensions.get::<Forwarded>());
        let (Some(caller), Some(fwd)) = (caller, fwd) else {
            tracing::error!("mcp call without a resolved caller");
            return Err(ErrorData::internal_error("Internal error", None));
        };
        let call = Call {
            s: &self.state,
            caller,
            fwd,
        };
        let raw = request.arguments.unwrap_or_default();
        let Some(out) = call.run(&request.name, raw).await else {
            let message = format!("Unknown tool: {}", request.name);
            return Err(ErrorData::invalid_params(message, None));
        };
        let result = match out {
            Ok(v) => CallToolResult::structured(v),
            Err(e) => CallToolResult::structured_error(json(e).unwrap_or(Value::Null)),
        };
        Ok(result.into())
    }
}
