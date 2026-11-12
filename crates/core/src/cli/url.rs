//! `reclip-core url show|reset`: the public URL used for absolute links.

use crate::access::secrets::PUBLIC_URL;
use crate::config::Config;
use crate::store::Store;

#[derive(Debug, PartialEq, Eq)]
pub enum UrlCommand {
    Show,
    Reset,
}

#[derive(Debug, PartialEq, Eq)]
enum Source {
    Configured,
    Learned,
}

pub fn parse(rest: &[&str]) -> Result<UrlCommand, String> {
    match rest {
        ["show"] => Ok(UrlCommand::Show),
        ["reset"] => Ok(UrlCommand::Reset),
        [] => Err("missing url subcommand".into()),
        _ => Err(format!(
            "unexpected arguments after url: {}",
            rest.join(" ")
        )),
    }
}

pub async fn execute(cfg: &Config, store: &Store, cmd: UrlCommand) -> anyhow::Result<i32> {
    match cmd {
        UrlCommand::Show => match current(cfg, store).await? {
            Some((url, source)) => {
                println!("{url}");
                let from = match source {
                    Source::Configured => "set by RECLIP_PUBLIC_URL",
                    Source::Learned => "learned from the reverse proxy",
                };
                eprintln!("({from})");
            }
            None => println!("none"),
        },
        UrlCommand::Reset => {
            match reset(store).await? {
                Some(url) => {
                    eprintln!("forgot the learned public URL {url}; restart the server to apply")
                }
                None => eprintln!("no learned public URL stored"),
            }
            if let Some(url) = &cfg.public_url {
                eprintln!("RECLIP_PUBLIC_URL is set and still applies: {url}");
            }
        }
    }
    Ok(0)
}

/// `RECLIP_PUBLIC_URL`, else the learned one, as the server would load it.
async fn current(cfg: &Config, store: &Store) -> anyhow::Result<Option<(String, Source)>> {
    if let Some(url) = &cfg.public_url {
        return Ok(Some((url.clone(), Source::Configured)));
    }
    let learned = store.setting(PUBLIC_URL).await?;
    Ok(learned.map(|u| (u, Source::Learned)))
}

/// Delete the learned URL; returns it when one was stored.
async fn reset(store: &Store) -> anyhow::Result<Option<String>> {
    let old = store.setting(PUBLIC_URL).await?;
    let deleted = store.delete_setting(PUBLIC_URL).await?;
    Ok(old.filter(|_| deleted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url_commands() {
        assert_eq!(parse(&["show"]), Ok(UrlCommand::Show));
        assert_eq!(parse(&["reset"]), Ok(UrlCommand::Reset));
        for bad in [&[][..], &["frob"], &["show", "x"], &["reset", "--all"]] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn show_prefers_configured_then_learned() {
        let (mut cfg, store) = (Config::from_env(&|_| None), Store::memory().await);
        assert_eq!(current(&cfg, &store).await.unwrap(), None);
        store
            .set_setting(PUBLIC_URL, "https://learned.example")
            .await
            .unwrap();
        let learned = current(&cfg, &store).await.unwrap();
        assert_eq!(
            learned,
            Some(("https://learned.example".into(), Source::Learned))
        );
        cfg.public_url = Some("https://conf.example".into());
        let conf = current(&cfg, &store).await.unwrap();
        assert_eq!(
            conf,
            Some(("https://conf.example".into(), Source::Configured))
        );
    }

    #[tokio::test]
    async fn reset_forgets_learned_url() {
        let store = Store::memory().await;
        assert_eq!(reset(&store).await.unwrap(), None);
        store
            .set_setting(PUBLIC_URL, "https://learned.example")
            .await
            .unwrap();
        store
            .set_setting("link_key", &"ab".repeat(32))
            .await
            .unwrap();
        let gone = reset(&store).await.unwrap();
        assert_eq!(gone.as_deref(), Some("https://learned.example"));
        assert_eq!(store.setting(PUBLIC_URL).await.unwrap(), None);
        assert!(store.setting("link_key").await.unwrap().is_some());
        assert_eq!(reset(&store).await.unwrap(), None);
    }
}
