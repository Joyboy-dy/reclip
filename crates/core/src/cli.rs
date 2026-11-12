//! `reclip-core keys ...` and `reclip-core url ...`: manage API keys and
//! the learned public URL from inside the container.
//!
//! Exit codes: 0 done, 1 failure, 2 usage error.

use reclip_shared::{ApiKeyInfo, CreateKeyRequest, ErrorCode, Scope};
use tracing_subscriber::EnvFilter;

use crate::access::keys::{MAX_EXPIRES_DAYS, create_key};
use crate::config::Config;
use crate::store::Store;

mod url;

const USAGE: &str = "usage:
  reclip-core keys create --name <name> [--scopes a,b] [--unlimited] [--expires-days N]
  reclip-core keys list
  reclip-core keys revoke <id>
  reclip-core url show
  reclip-core url reset
scopes: probe, jobs:create, jobs:read, jobs:cancel, delegate";

#[derive(Debug)]
enum Command {
    Create(CreateKeyRequest),
    List,
    Revoke(String),
    Url(url::UrlCommand),
}

/// Run a `keys` or `url` subcommand and return its exit code, or `None`
/// when `args` (without the program name) are neither.
pub async fn run(args: &[String]) -> anyhow::Result<Option<i32>> {
    let cmd = match parse(args) {
        None => return Ok(None),
        Some(Ok(cmd)) => cmd,
        Some(Err(msg)) => {
            eprintln!("error: {msg}\n{USAGE}");
            return Ok(Some(2));
        }
    };
    // Warnings and errors only, on stderr: stdout is the command's output.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::new("warn"))
        .try_init();
    let cfg = Config::from_env(&|k| std::env::var(k).ok());
    let store = Store::open(&cfg.db_path()).await?;
    Ok(Some(execute(&cfg, &store, cmd).await?))
}

async fn execute(cfg: &Config, store: &Store, cmd: Command) -> anyhow::Result<i32> {
    match cmd {
        Command::Url(cmd) => url::execute(cfg, store, cmd).await,
        Command::Create(req) => match create_key(store, req).await {
            Ok(created) => {
                println!("{}", created.key);
                eprintln!(
                    "created key {} ({}); the key above is shown only once: store it now",
                    created.info.id, created.info.name
                );
                Ok(0)
            }
            Err(e) => {
                eprintln!("error: {}", e.message);
                Ok(if e.code == ErrorCode::InvalidRequest {
                    2
                } else {
                    1
                })
            }
        },
        Command::List => {
            for key in store.list_keys().await? {
                println!("{}", key_line(&key));
            }
            Ok(0)
        }
        Command::Revoke(id) => {
            if store.revoke_key(&id).await? {
                eprintln!("revoked key {id}");
                Ok(0)
            } else {
                eprintln!("error: no api key with id {id}");
                Ok(1)
            }
        }
    }
}

/// One line describing a key; never contains a secret.
fn key_line(k: &ApiKeyInfo) -> String {
    let opt = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let scopes: Vec<&str> = k.scopes.iter().map(|s| scope_name(*s)).collect();
    format!(
        "id={} name={:?} scopes={} unlimited={} created_at={} expires_at={} \
         revoked_at={} last_used_at={} request_count={}",
        k.id,
        k.name,
        scopes.join(","),
        k.unlimited,
        k.created_at,
        opt(&k.expires_at),
        opt(&k.revoked_at),
        opt(&k.last_used_at),
        k.request_count
    )
}

const SCOPES: [(&str, Scope); 5] = [
    ("probe", Scope::Probe),
    ("jobs:create", Scope::JobsCreate),
    ("jobs:read", Scope::JobsRead),
    ("jobs:cancel", Scope::JobsCancel),
    ("delegate", Scope::Delegate),
];

fn scope_name(s: Scope) -> &'static str {
    SCOPES.iter().find(|(_, v)| *v == s).map_or("?", |(n, _)| n)
}

/// `None` unless `args[0]` is `keys` or `url`; then the command or a usage
/// error.
fn parse(args: &[String]) -> Option<Result<Command, String>> {
    let (first, rest) = args.split_first()?;
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    match first.as_str() {
        "keys" => Some(parse_keys(&rest)),
        "url" => Some(url::parse(&rest).map(Command::Url)),
        _ => None,
    }
}

fn parse_keys(rest: &[&str]) -> Result<Command, String> {
    match rest {
        ["create", flags @ ..] => parse_create(flags).map(Command::Create),
        ["list"] => Ok(Command::List),
        ["revoke", id] => Ok(Command::Revoke(id.to_string())),
        [] => Err("missing keys subcommand".into()),
        [other, ..] => Err(format!("unexpected arguments after keys: {other}")),
    }
}

fn parse_create(mut flags: &[&str]) -> Result<CreateKeyRequest, String> {
    let mut req = CreateKeyRequest {
        name: String::new(),
        scopes: None,
        unlimited: false,
        expires_in_days: None,
        limits: None,
    };
    let mut name = None;
    while let Some((flag, tail)) = flags.split_first() {
        flags = tail;
        if *flag == "--unlimited" {
            req.unlimited = true;
            continue;
        }
        let Some((value, tail)) = flags.split_first() else {
            return Err(format!("{flag} needs a value"));
        };
        flags = tail;
        match *flag {
            "--name" => name = Some(value.to_string()),
            "--scopes" => req.scopes = Some(parse_scopes(value)?),
            "--expires-days" => match value.parse::<u32>() {
                Ok(n) if (1..=MAX_EXPIRES_DAYS).contains(&n) => req.expires_in_days = Some(n),
                _ => {
                    return Err(format!(
                        "--expires-days must be between 1 and {MAX_EXPIRES_DAYS}: {value}"
                    ));
                }
            },
            other => return Err(format!("unknown option {other}")),
        }
    }
    req.name = name.ok_or("--name is required")?;
    Ok(req)
}

fn parse_scopes(list: &str) -> Result<Vec<Scope>, String> {
    list.split(',')
        .map(str::trim)
        .map(|n| {
            SCOPES
                .iter()
                .find(|(name, _)| *name == n)
                .map(|(_, s)| *s)
                .ok_or_else(|| format!("unknown scope {n}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parses_create_flags() {
        let a = args(&[
            "keys",
            "create",
            "--name",
            "bot",
            "--scopes",
            "probe,delegate",
            "--unlimited",
            "--expires-days",
            "30",
        ]);
        let Some(Ok(Command::Create(req))) = parse(&a) else {
            panic!("not a create command");
        };
        assert_eq!(req.name, "bot");
        assert_eq!(req.scopes, Some(vec![Scope::Probe, Scope::Delegate]));
        assert!(req.unlimited);
        assert_eq!(req.expires_in_days, Some(30));
        let Some(Ok(Command::Create(req))) = parse(&args(&["keys", "create", "--name", "x"]))
        else {
            panic!("not a create command");
        };
        assert_eq!(
            (req.scopes, req.unlimited, req.expires_in_days),
            (None, false, None)
        );
        assert!(matches!(
            parse(&args(&["keys", "list"])),
            Some(Ok(Command::List))
        ));
        assert!(matches!(
            parse(&args(&["keys", "revoke", "abcd1234"])),
            Some(Ok(Command::Revoke(id))) if id == "abcd1234"
        ));
    }

    #[test]
    fn bad_keys_arguments_are_usage_errors() {
        for bad in [
            &["keys"][..],
            &["keys", "frob"],
            &["keys", "create"],
            &["keys", "create", "--name"],
            &["keys", "create", "--name", "x", "--scopes", "probe,root"],
            &["keys", "create", "--name", "x", "--expires-days", "0"],
            &["keys", "create", "--name", "x", "--expires-days", "soon"],
            &["keys", "create", "--name", "x", "--expires-days", "36501"],
            &[
                "keys",
                "create",
                "--name",
                "x",
                "--expires-days",
                "4294967295",
            ],
            &["keys", "create", "--name", "x", "--color", "red"],
            &["keys", "list", "extra"],
            &["keys", "revoke"],
            &["keys", "revoke", "a", "b"],
        ] {
            assert!(matches!(parse(&args(bad)), Some(Err(_))), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn unknown_subcommand_is_none() {
        assert!(parse(&args(&[])).is_none());
        assert!(parse(&args(&["serve"])).is_none());
        assert!(parse(&args(&["healthcheck"])).is_none());
        assert!(matches!(
            parse(&args(&["url", "show"])),
            Some(Ok(Command::Url(url::UrlCommand::Show)))
        ));
        assert!(matches!(parse(&args(&["url"])), Some(Err(_))));
        assert_eq!(run(&args(&["serve"])).await.unwrap(), None);
    }
}
