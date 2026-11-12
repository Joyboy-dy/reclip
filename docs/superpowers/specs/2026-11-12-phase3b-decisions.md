# Phase 3b : admin, formats et idées reprises des PR amont

Décisions prises le 2026-11-12 à partir des réponses du propriétaire (fichier `_r.md`) et de la lecture des 42 PR de `averygan/reclip`. Elles complètent `2026-11-11-reclip-platform-design.md` ; la phase 3b se place entre la phase 3 (accès et limites) et la phase 4 (agents).

Maquettes à valider avant implémentation : https://claude.ai/artifact/CxA76XnrtR2hN5jEey7UXG

## Accès

- Le site reste **public**, avec les limites de la phase 3.
- **Page `/admin`**, absente de la navigation publique.
  - Connexion par email et mot de passe. Les emails autorisés viennent de `RECLIP_ADMIN_EMAILS` (liste séparée par des virgules).
  - Mot de passe par défaut `RECLIP_ADMIN_DEFAULT_PASSWORD`, valable seulement pour la première connexion de chaque compte. Le changement de mot de passe est alors obligatoire, 12 caractères minimum.
  - Chaque admin peut ensuite changer son mot de passe.
  - Mots de passe hachés avec Argon2id.
  - Session par cookie `HttpOnly`, `Secure`, `SameSite=Strict`, d'une durée de 12 h, avec contrôle `Origin`.
  - Après 5 échecs en 15 minutes pour une même IP ou un même email, les tentatives sont freinées.
- L'admin y crée, liste et révoque les clés API : nom, droits, expiration, limites ou illimitée. La clé n'est affichée qu'une fois.
- Le jeton admin de la phase 3 et la commande `reclip-core keys` restent disponibles pour les scripts.
- WebMCP (phase 4) restera utilisable sans clé, avec les limites visiteur.

## Formats et qualité

| # PR | Décision |
|---|---|
| #95, #47 | Oui : H.264 + AAC par défaut ; une qualité disponible seulement en VP9 ou AV1 porte l'étiquette du codec (« 2160p VP9 »). |
| #73 (WhatsApp) | Non. |
| #36 (formats audio) | Oui : MP3, M4A (AAC), Opus, FLAC, WAV. |
| #36 (puces de codec) | Non : les étiquettes de #95 suffisent et évitent d'encombrer la carte. |
| #71 | Oui : conteneurs MKV et MOV en plus de MP4. |
| #20 (GIF) | Non. |
| #75 (faststart) | Oui : `-movflags +faststart` sur les MP4 et MOV. |
| #75 (extrait) | Oui : option Extrait (début et fin), via `--download-sections`. |
| #97 | Oui : option « Remove metadata » (ffmpeg `-map_metadata -1`, sans dépendance externe). |

## Sites

| # PR | Décision |
|---|---|
| #73, #99 | Oui : imitation de navigateur (curl-cffi / `--impersonate`) pour Instagram et TikTok ; vérifier que le binaire yt-dlp de l'image la prend en charge. |
| #83, #71 | Oui : correctifs Bilibili (en-tête `Origin` contre l'erreur 412, formats sans `vcodec`). |
| #83 (fragments) | Oui : `--concurrent-fragments 4` (`RECLIP_CONCURRENT_FRAGMENTS`), car les flux HLS se téléchargent bien plus vite pour un coût faible. |
| #41 | Oui : un post X ou Twitter avec plusieurs vidéos donne une carte par vidéo. |
| #89, #94, #85, #69 | Oui : vérifier et corriger les titres sur plusieurs lignes, les URL YouTube `/show/`, le bouton Fetch après une erreur de playlist, et le nom du fichier égal au titre. |

## Interface

| # PR | Décision |
|---|---|
| #101 (thème sombre) | Non. |
| #101 (historique) | Oui : historique de 30 jours par visiteur (titre, miniature locale de petite taille, format, taille, date), avec « Save » si le fichier existe encore et « Download again » sinon ; bouton « Clear history ». |
| #5 (langues) | Non : interface en anglais seulement. |
| #68 | Oui : « Resume » sur une tâche annulée ou échouée, qui reprend les fichiers `.part` de yt-dlp dans le même dossier. Les fichiers partiels d'une tâche annulée sont donc gardés jusqu'à son expiration. |

## Agents, outils, déploiement

| # PR | Décision |
|---|---|
| #60 (transcript) | Oui, en phase 4 : action « transcript » qui renvoie les sous-titres en texte. |
| #61 (Whisper) | Non. |
| #60 (CLI utilisateur) | Non : l'API, WebMCP et `/mcp` couvrent ce besoin. |
| #17 | Oui : image publiée sur GHCR par la CI, que Dokploy peut tirer. |
| #12 (plafond disque) | Oui : `RECLIP_MAX_DISK` (20 Go par défaut). Au-delà, les fichiers terminés les plus anciens expirent en premier ; s'il n'y en a plus, les nouvelles tâches attendent. |
| #53 | Phase 5 : le bot reste ouvert à tous, sans liste d'accès. |
| #82, #64, #33, #70 | Non. |
