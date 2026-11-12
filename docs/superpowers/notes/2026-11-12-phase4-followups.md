# Phase 4 : suites à donner

Points connus après la revue finale de la phase 4 (agents IA : `/mcp`, WebMCP, transcript). Aucun ne bloque la mise en production.

## À vérifier sur l'image déployée (yt-dlp réel)

yt-dlp n'est pas installé en local ; les tests utilisent un faux binaire. À tester une fois déployé :

- transcript d'une vidéo YouTube avec sous-titres automatiques (texte complet, sans doublons) ;
- transcript d'une vidéo YouTube avec sous-titres manuels dans une autre langue que l'anglais ;
- transcript d'une vidéo Bilibili (sous-titres `srt` convertis en `vtt`, piste `danmaku` ignorée) ;
- transcript d'un lien de playlist (refus « Pass a single video link ») ;
- `/mcp` depuis Claude Code (`claude mcp add --transport http …`) et depuis Claude Desktop via `mcp-remote`.

## Reporté

- **Titre des téléchargements lancés par `/mcp`** : `title` est vide, le fichier s'appelle `<id>.mp4` et l'historique affiche un titre vide. Correction : remplir le titre depuis les métadonnées de yt-dlp dans le worker quand il manque (utile aussi pour l'API REST).
- **Couches** : `access/quota.rs` dépend de `service::ServiceError`, et `transcript.rs` est un petit module séparé de `service.rs`. À ranger lors d'une prochaine modification de ces fichiers.
- **WebMCP, chemin `playlist_item`** : l'identifiant `quality` n'est pas vérifié contre les formats ; le serveur le refuse ou yt-dlp retombe sur `best`.
- **Les deux surfaces** gardent quelques écarts documentés : WebMCP ajoute les états `pending` et `waiting`, et son lien de fichier est relatif.

## Hors phase 4 (déjà présents avant)

- Les erreurs yt-dlp brutes peuvent contenir des chemins ou des noms d'hôte de proxy.
- Les redirections suivies par yt-dlp après le contrôle SSRF ne sont pas revérifiées.
- Pas de plafond global de processus yt-dlp pour les analyses et transcripts, seulement les seaux par appelant.
