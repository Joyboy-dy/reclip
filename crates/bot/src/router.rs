//! Turns an update into a handler call: commands, links and button data.

use std::sync::Arc;

use crate::app::App;
use crate::core_client::Core;
use crate::telegram::{ChatKind, Telegram, Update};
use crate::{choice, flow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Start,
    Help,
    /// `/mp4 <link>`: skip the format choice.
    Mp4(Option<String>),
    /// `/mp3 <link>`: download the audio directly.
    Mp3(Option<String>),
    Cancel,
    Jobs,
    Unknown,
}

/// Button data. Each form stays under Telegram's 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    /// `f:<choice>:v` (video) or `f:<choice>:a` (audio).
    Format { choice: String, audio: bool },
    /// `q:<choice>:<index>` or `q:<choice>:b` (best).
    Quality { choice: String, pick: Option<usize> },
    /// `x:<job id>`.
    Cancel { job: String },
}

pub async fn handle_update<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    upd: Update,
) {
    if let Some(msg) = upd.message {
        if msg.chat.kind != ChatKind::Private {
            return; // groups and channels: ignored silently
        }
        flow::on_message(app, msg).await;
    } else if let Some(cb) = upd.callback_query {
        if cb
            .message
            .as_ref()
            .is_some_and(|m| m.chat.kind != ChatKind::Private)
        {
            app.answer(&cb.id, None).await;
            return;
        }
        match cb.data.as_deref().and_then(parse_callback) {
            Some(Callback::Format { choice, audio }) => {
                choice::on_format(app, &cb, &choice, audio).await
            }
            Some(Callback::Quality { choice, pick }) => {
                choice::on_quality(app, &cb, &choice, pick).await
            }
            Some(Callback::Cancel { job }) => flow::on_cancel_button(app, &cb, &job).await,
            None => app.answer(&cb.id, None).await,
        }
    }
}

pub fn parse_command(text: &str) -> Option<Command> {
    let text = text.trim_start();
    let rest = text.strip_prefix('/')?;
    let (word, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    // `/start@SomeBot` in a chat with several bots.
    let name = word.split('@').next().unwrap_or("").to_ascii_lowercase();
    Some(match name.as_str() {
        "start" => Command::Start,
        "help" => Command::Help,
        "mp4" => Command::Mp4(find_link(args)),
        "mp3" => Command::Mp3(find_link(args)),
        "cancel" => Command::Cancel,
        "jobs" => Command::Jobs,
        _ => Command::Unknown,
    })
}

pub fn parse_callback(data: &str) -> Option<Callback> {
    let (kind, rest) = data.split_once(':')?;
    match kind {
        "x" if !rest.is_empty() => Some(Callback::Cancel { job: rest.into() }),
        "f" | "q" => {
            let (choice, arg) = rest.split_once(':')?;
            if choice.is_empty() {
                return None;
            }
            let choice = choice.to_string();
            match (kind, arg) {
                ("f", "v") => Some(Callback::Format {
                    choice,
                    audio: false,
                }),
                ("f", "a") => Some(Callback::Format {
                    choice,
                    audio: true,
                }),
                ("q", "b") => Some(Callback::Quality { choice, pick: None }),
                ("q", n) => Some(Callback::Quality {
                    choice,
                    pick: Some(n.parse().ok()?),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The first `http://` or `https://` address of the text.
pub fn find_link(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = [lower.find("https://"), lower.find("http://")]
        .into_iter()
        .flatten()
        .min()?;
    let link: &str = text[start..]
        .split(char::is_whitespace)
        .next()
        .unwrap_or("");
    let link = link.trim_end_matches(['.', ',', ';', '!', '?', ')', ']', '>', '"', '\'']);
    // Something must follow the scheme.
    let host = link.split_once("://").map_or("", |(_, rest)| rest);
    (!host.is_empty()).then(|| link.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(parse_command("/start"), Some(Command::Start));
        assert_eq!(parse_command("/HELP@ReclipBot"), Some(Command::Help));
        assert_eq!(
            parse_command("/mp3  https://y.be/x  "),
            Some(Command::Mp3(Some("https://y.be/x".into())))
        );
        assert_eq!(parse_command("/mp4"), Some(Command::Mp4(None)));
        assert_eq!(parse_command("/what"), Some(Command::Unknown));
        assert_eq!(parse_command("hello /start"), None);
    }

    #[test]
    fn callbacks() {
        assert_eq!(
            parse_callback("f:ab12:a"),
            Some(Callback::Format {
                choice: "ab12".into(),
                audio: true
            })
        );
        assert_eq!(
            parse_callback("q:ab12:3"),
            Some(Callback::Quality {
                choice: "ab12".into(),
                pick: Some(3)
            })
        );
        assert_eq!(
            parse_callback("q:ab12:b"),
            Some(Callback::Quality {
                choice: "ab12".into(),
                pick: None
            })
        );
        assert_eq!(
            parse_callback("x:0190-aa"),
            Some(Callback::Cancel {
                job: "0190-aa".into()
            })
        );
        for bad in ["", "x:", "f:ab:z", "q:ab:-1", "f::v", "zz:1", "f:ab"] {
            assert_eq!(parse_callback(bad), None, "{bad}");
        }
    }

    #[test]
    fn links() {
        assert_eq!(
            find_link("look HTTPS://Y.be/AbC?t=1, nice").as_deref(),
            Some("HTTPS://Y.be/AbC?t=1")
        );
        assert_eq!(
            find_link("(https://a.b/c) http://first.example").as_deref(),
            Some("https://a.b/c")
        );
        assert_eq!(find_link("no link here"), None);
        assert_eq!(find_link("https:// nothing"), None);
    }
}
