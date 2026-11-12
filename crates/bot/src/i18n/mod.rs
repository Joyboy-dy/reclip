//! French and English texts. Plain text only (no Telegram markup).

use reclip_shared::ErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    Fr,
    #[default]
    En,
}

impl Lang {
    /// French when the Telegram `language_code` starts with `fr`, English otherwise.
    pub fn from_code(code: Option<&str>) -> Lang {
        match code {
            Some(c) if c.to_ascii_lowercase().starts_with("fr") => Lang::Fr,
            _ => Lang::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Fr => "fr",
            Lang::En => "en",
        }
    }

    /// Inverse of [`Lang::code`], for the persisted state.
    pub fn from_stored(code: &str) -> Lang {
        Lang::from_code(Some(code))
    }

    fn pick(self, fr: &str, en: &str) -> String {
        match self {
            Lang::Fr => fr.to_string(),
            Lang::En => en.to_string(),
        }
    }
}

pub fn welcome(l: Lang) -> String {
    l.pick(
        "Bienvenue sur ReClip. Envoie-moi le lien d'une vidéo et je te la télécharge.",
        "Welcome to ReClip. Send me a video link and I will download it for you.",
    )
}

pub fn help(l: Lang) -> String {
    l.pick(
        "Envoie un lien (YouTube, TikTok, Instagram, X...), choisis vidéo ou audio, puis la qualité. \
         Commandes : /start, /help, /jobs (téléchargements en cours), /cancel.",
        "Send a link (YouTube, TikTok, Instagram, X...), pick video or audio, then the quality. \
         Commands: /start, /help, /jobs (running downloads), /cancel.",
    )
}

pub fn not_a_link(l: Lang) -> String {
    l.pick(
        "Je n'ai pas trouvé de lien dans ton message. Envoie-moi l'adresse de la vidéo.",
        "I could not find a link in your message. Send me the video address.",
    )
}

pub fn analyzing(l: Lang) -> String {
    l.pick("Analyse du lien...", "Checking the link...")
}

pub fn choose_format(l: Lang, title: &str) -> String {
    match l {
        Lang::Fr => format!("{title}\n\nQue veux-tu télécharger ?"),
        Lang::En => format!("{title}\n\nWhat do you want to download?"),
    }
}

pub fn choose_quality(l: Lang, title: &str) -> String {
    match l {
        Lang::Fr => format!("{title}\n\nChoisis la qualité :"),
        Lang::En => format!("{title}\n\nPick the quality:"),
    }
}

pub fn btn_video(l: Lang) -> String {
    l.pick("Vidéo", "Video")
}

pub fn btn_audio(l: Lang) -> String {
    l.pick("Audio (MP3)", "Audio (MP3)")
}

pub fn btn_best(l: Lang) -> String {
    l.pick("Meilleure qualité", "Best quality")
}

pub fn btn_cancel(l: Lang) -> String {
    l.pick("✖ Annuler", "✖ Cancel")
}

pub fn queued(l: Lang, title: &str) -> String {
    match l {
        Lang::Fr => format!("{title}\n\nEn attente..."),
        Lang::En => format!("{title}\n\nQueued..."),
    }
}

/// Progress text; `pct` is 0..=100, `speed` bytes per second, `eta` seconds.
pub fn progress(
    l: Lang,
    title: &str,
    pct: Option<f32>,
    speed: Option<u64>,
    eta: Option<u64>,
) -> String {
    let mut line = match pct {
        Some(p) => format!("{:.0} %", p.clamp(0.0, 100.0)),
        None => l.pick("Téléchargement...", "Downloading..."),
    };
    if let Some(s) = speed {
        line.push_str(&format!(" · {}/s", human_size(s)));
    }
    if let Some(e) = eta {
        let eta_txt = match l {
            Lang::Fr => format!(" · reste {e} s"),
            Lang::En => format!(" · {e} s left"),
        };
        line.push_str(&eta_txt);
    }
    format!("{title}\n\n{line}")
}

pub fn processing(l: Lang, title: &str) -> String {
    match l {
        Lang::Fr => format!("{title}\n\nTraitement du fichier..."),
        Lang::En => format!("{title}\n\nProcessing the file..."),
    }
}

pub fn done(l: Lang, title: &str) -> String {
    match l {
        Lang::Fr => format!("{title}\n\nTerminé, envoi du fichier..."),
        Lang::En => format!("{title}\n\nDone, sending the file..."),
    }
}

pub fn cancelled(l: Lang) -> String {
    l.pick("Téléchargement annulé.", "Download cancelled.")
}

pub fn not_yours(l: Lang) -> String {
    l.pick(
        "Ce téléchargement n'est pas le tien.",
        "This download is not yours.",
    )
}

pub fn playlist_refused(l: Lang, site: Option<&str>) -> String {
    let base = l.pick(
        "Les playlists et les publications à plusieurs vidéos ne sont pas gérées ici. \
         Envoie le lien d'une seule vidéo",
        "Playlists and multi-video posts are not handled here. Send the link of a single video",
    );
    match (site, l) {
        (Some(s), Lang::Fr) => format!("{base}, ou utilise le site : {s}"),
        (Some(s), Lang::En) => format!("{base}, or use the website: {s}"),
        (None, _) => format!("{base}."),
    }
}

pub fn link_expired(l: Lang) -> String {
    l.pick(
        "Ce lien a expiré. Renvoie-moi l'adresse de la vidéo pour recommencer.",
        "This link has expired. Send me the video address again to start over.",
    )
}

pub fn file_too_big(l: Lang, size: u64, link: &str) -> String {
    let s = human_size(size);
    match l {
        Lang::Fr => format!(
            "Le fichier ({s}) est trop gros pour Telegram. Télécharge-le ici (lien temporaire) :\n{link}"
        ),
        Lang::En => format!(
            "The file ({s}) is too big for Telegram. Download it here (temporary link):\n{link}"
        ),
    }
}

pub fn error(l: Lang, code: ErrorCode, retry_after: Option<u64>) -> String {
    let (fr, en) = match code {
        ErrorCode::BotCheck => (
            "Le site demande une vérification anti-robot, impossible de télécharger.",
            "The site asks for a bot check, the download is not possible.",
        ),
        ErrorCode::UnsupportedUrl => (
            "Ce lien n'est pas pris en charge.",
            "This link is not supported.",
        ),
        ErrorCode::Unavailable => (
            "Cette vidéo est indisponible.",
            "This video is unavailable.",
        ),
        ErrorCode::Private => ("Cette vidéo est privée.", "This video is private."),
        ErrorCode::GeoBlocked => (
            "Cette vidéo n'est pas disponible dans la région du serveur.",
            "This video is not available in the server's region.",
        ),
        ErrorCode::TooLarge => (
            "Cette vidéo est trop volumineuse.",
            "This video is too large.",
        ),
        ErrorCode::TooLong => ("Cette vidéo est trop longue.", "This video is too long."),
        ErrorCode::Timeout => (
            "Le téléchargement a pris trop de temps.",
            "The download took too long.",
        ),
        ErrorCode::Interrupted => (
            "Le téléchargement a été interrompu. Réessaie.",
            "The download was interrupted. Try again.",
        ),
        ErrorCode::Cancelled => ("Téléchargement annulé.", "Download cancelled."),
        ErrorCode::RateLimited => {
            return match (l, retry_after) {
                (Lang::Fr, Some(s)) => format!("Trop de demandes. Réessaie dans {s} s."),
                (Lang::En, Some(s)) => format!("Too many requests. Try again in {s} s."),
                (Lang::Fr, None) => "Trop de demandes. Réessaie dans un moment.".into(),
                (Lang::En, None) => "Too many requests. Try again in a moment.".into(),
            };
        }
        ErrorCode::ForbiddenTarget => (
            "Cette adresse n'est pas autorisée.",
            "This address is not allowed.",
        ),
        ErrorCode::Unauthorized | ErrorCode::Forbidden => (
            "Le bot n'est pas autorisé à utiliser ReClip. Préviens l'administrateur.",
            "The bot is not allowed to use ReClip. Tell the administrator.",
        ),
        ErrorCode::InvalidRequest => ("Demande invalide.", "Invalid request."),
        ErrorCode::Internal => (
            "Erreur interne. Réessaie plus tard.",
            "Internal error. Try again later.",
        ),
    };
    l.pick(fr, en)
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

#[cfg(test)]
mod tests;
