//! The six agent tools, defined once for the MCP server and for WebMCP.
//!
//! `tool_defs()` is exported to `web/src/api/tools.gen.json` by
//! `tests/export_ts.rs`; the page registers exactly those definitions.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::MediaFormat;

/// One tool: a name, a description written for an LLM, and a JSON Schema
/// (draft 2020-12) of its arguments.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

const FORMATS: [MediaFormat; 8] = [
    MediaFormat::Mp4,
    MediaFormat::Mkv,
    MediaFormat::Mov,
    MediaFormat::Mp3,
    MediaFormat::M4a,
    MediaFormat::Opus,
    MediaFormat::Flac,
    MediaFormat::Wav,
];

const SCHEMA: &str = "https://json-schema.org/draft/2020-12/schema";

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "$schema": SCHEMA,
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool(name: &str, description: &str, input_schema: Value) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema,
    }
}

fn url_prop(what: &str) -> Value {
    json!({"type": "string", "format": "uri", "description": what})
}

fn id_prop() -> Value {
    json!({"type": "string", "description": "Download id, as returned by start_download or list_downloads."})
}

fn start_download_schema() -> Value {
    let formats: Vec<Value> = FORMATS
        .iter()
        .map(|f| serde_json::to_value(f).unwrap_or(Value::Null))
        .collect();
    object(
        json!({
            "url": url_prop("Link to the video or audio page."),
            "format": {
                "type": "string",
                "enum": formats,
                "default": "mp4",
                "description": "Output format: mp4, mkv or mov for video; mp3, m4a, opus, flac or wav for audio only."
            },
            "quality": {
                "type": "string",
                "description": "A format id from probe_media (formats[].id) to pick a resolution. Omit for the best quality."
            },
            "clip": {
                "type": "object",
                "description": "Keep only a time range of the media.",
                "properties": {
                    "start": {"type": "number", "minimum": 0, "description": "Start, in seconds."},
                    "end": {"type": "number", "minimum": 0, "description": "End, in seconds. Omit to run to the end."}
                },
                "required": ["start"],
                "additionalProperties": false
            },
            "strip_metadata": {
                "type": "boolean",
                "default": false,
                "description": "Remove title, author and other metadata from the output file."
            },
            "playlist_item": {
                "type": "integer",
                "minimum": 1,
                "description": "1-based item to download when the link is a multi-video post (see probe_media playlist entries)."
            }
        }),
        &["url"],
    )
}

/// The six tools, in a stable order.
pub fn tool_defs() -> Vec<ToolDef> {
    vec![
        tool(
            "probe_media",
            "Look up a video or audio link without downloading it. Use it first to learn the title, duration, uploader and the available qualities (format ids); for a playlist or multi-video post it lists the entries instead.",
            object(
                json!({"url": url_prop("Link to the video, audio or playlist page.")}),
                &["url"],
            ),
        ),
        tool(
            "start_download",
            "Start downloading a video or audio link in the background and return the new download right away (state queued). Poll get_download with its id until the state is done, then use the file link it returns.",
            start_download_schema(),
        ),
        tool(
            "list_downloads",
            "List the caller's recent downloads with their state (queued, downloading, processing, done, cancelled, failed, expired; in the browser (WebMCP) a download not yet started shows as pending or waiting) and progress. Use it to find a download id or to see what is still running.",
            object(json!({}), &[]),
        ),
        tool(
            "get_download",
            "Get one download's state, progress and error (in the browser (WebMCP) a download not yet started shows as pending or waiting). When the file is ready (state done) the result also carries a link to fetch it.",
            object(json!({"id": id_prop()}), &["id"]),
        ),
        tool(
            "cancel_download",
            "Cancel a queued or running download. It has no effect on a download that already finished.",
            object(json!({"id": id_prop()}), &["id"]),
        ),
        tool(
            "get_transcript",
            "Return a video's subtitles as plain text, without downloading the media: manual subtitles when they exist, otherwise automatic captions. Use it to read, summarize or search what is said. Fails with unavailable when the video has no subtitles.",
            object(
                json!({
                    "url": url_prop("Link to the video page."),
                    "language": {
                        "type": "string",
                        "pattern": "^[A-Za-z0-9-]{1,35}$",
                        "description": "BCP-47 language code such as en, fr or pt-BR. Default: the video's original language, then en."
                    }
                }),
                &["url"],
            ),
        ),
    ]
}
