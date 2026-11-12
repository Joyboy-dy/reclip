//! Texts for job lists, cancellation and expiry.

use super::Lang;

pub fn nothing_to_cancel(l: Lang) -> String {
    l.pick("Aucun téléchargement en cours.", "No download in progress.")
}

pub fn cancelled_count(l: Lang, n: usize) -> String {
    match l {
        Lang::Fr => format!("Annulation demandée pour {n} téléchargement(s)."),
        Lang::En => format!("Cancellation requested for {n} download(s)."),
    }
}

/// `titles` are the running downloads of the user.
pub fn jobs_list(l: Lang, titles: &[String]) -> String {
    if titles.is_empty() {
        return nothing_to_cancel(l);
    }
    let mut out = l.pick("Téléchargements en cours :", "Running downloads:");
    for t in titles {
        out.push_str("\n• ");
        out.push_str(t);
    }
    out
}

pub fn cancelling(l: Lang) -> String {
    l.pick("Annulation...", "Cancelling...")
}

pub fn job_gone(l: Lang) -> String {
    l.pick(
        "Ce téléchargement n'est plus disponible.",
        "This download is no longer available.",
    )
}

/// The upload through Telegram failed (not a size problem).
pub fn send_failed_link(l: Lang, link: &str) -> String {
    match l {
        Lang::Fr => format!(
            "L'envoi par Telegram a échoué. Télécharge le fichier ici (lien temporaire) :\n{link}"
        ),
        Lang::En => format!(
            "Sending through Telegram failed. Download the file here (temporary link):\n{link}"
        ),
    }
}

/// No usable public address for a download link.
pub fn link_unavailable(l: Lang, too_big: bool) -> String {
    let first = match (l, too_big) {
        (Lang::Fr, true) => "Le fichier est trop gros pour Telegram.",
        (Lang::Fr, false) => "Le fichier n'a pas pu être envoyé par Telegram.",
        (Lang::En, true) => "The file is too big for Telegram.",
        (Lang::En, false) => "The file could not be sent through Telegram.",
    };
    let rest = l.pick(
        "Il pourra être téléchargé depuis le site ReClip une fois son adresse configurée.",
        "It can be downloaded from the ReClip website once its address is configured.",
    );
    format!("{first} {rest}")
}

pub fn file_expired(l: Lang) -> String {
    l.pick(
        "Le fichier a expiré avant l'envoi. Renvoie le lien pour recommencer.",
        "The file expired before it could be sent. Send the link again.",
    )
}
