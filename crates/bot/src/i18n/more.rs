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

pub fn file_expired(l: Lang) -> String {
    l.pick(
        "Le fichier a expiré avant l'envoi. Renvoie le lien pour recommencer.",
        "The file expired before it could be sent. Send the link again.",
    )
}
