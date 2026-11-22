# Tâches de la semaine (23 au 29 novembre 2026) : intégrations musique et suivi admin

Partie musique, rotation WARP et partie admin codées et testées le 22 novembre 2026. Règle de sélection : on intègre une plateforme seulement si elle marche **sans compte, sans cookie et sans clé d'API**. Sinon, on la met de côté.

## Deux façons d'obtenir l'audio

- **Flux direct** : la plateforme sert un audio non chiffré, yt-dlp le récupère tel quel. C'est le vrai fichier, sans erreur de correspondance.
- **Correspondance via YouTube Music** : la plateforme chiffre son audio. On lit seulement ses métadonnées publiques (titre, artiste, durée, ISRC, pochette), on cherche le morceau sur YouTube Music et on télécharge ce résultat. La qualité est celle de YouTube (Opus ~130-160 kbps).

Rappel : reclip envoie déjà n'importe quel lien à yt-dlp (pas de liste de sites autorisés) et `site_of` regroupe déjà `music.youtube.com` avec `youtube.com`.

## À faire cette semaine

### 1. SoundCloud (flux direct)
- [x] Tester un titre, une playlist et un profil avec yt-dlp tel qu'il est dans reclip
- [x] Vérifier en MP3 et en audio d'origine : tags (titre, artiste) et pochette intégrée
- [x] Repérer les titres « Go+ » (preview 30 s) et renvoyer une erreur claire

### 2. Bandcamp (flux direct)
- [x] Tester un titre et un album (flux de preview MP3 128 gratuit)
- [x] Vérifier les tags, la pochette et le numéro de piste sur un album

### 3. YouTube Music (flux direct)
- [x] Tester un titre, un album et une playlist `music.youtube.com`
- [x] Ajouter une option « garder l'audio d'origine » (`.opus` / `.m4a` sans réencodage) en plus du MP3

### 4. Spotify (correspondance via YouTube Music)
- [x] Lire les métadonnées depuis la page publique `open.spotify.com/embed/...` (sans clé)
- [x] Gérer les liens de titre, d'album et de playlist (la page embed donne jusqu'à 50 titres, la limite des playlists de reclip)
- [x] Chercher et choisir le résultat : écart de durée de ±3 s au plus, chaîne « - Topic » en priorité, refuser les versions live, reprise ou remix si l'original ne l'est pas
- [x] Écrire les tags et la pochette Spotify sur le fichier, sans réencoder l'audio
- [x] Prévenir l'utilisateur que l'audio vient de YouTube (pas de fausse mention « 320 kbps »)
- [x] Échec net quand aucun résultat ne passe le filtre, plutôt qu'un mauvais morceau

### 5. Deezer (correspondance via YouTube Music)
- [x] Lire les métadonnées depuis l'API publique `api.deezer.com/track/<id>` (sans clé, donne l'ISRC)
- [x] Réutiliser la recherche et le choix du résultat de Spotify

### 6. Apple Music (correspondance via YouTube Music)
- [x] Lire les métadonnées depuis l'API publique iTunes Lookup (`itunes.apple.com/lookup?id=...`, sans clé)
- [x] Réutiliser la recherche et le choix du résultat de Spotify

### Ce qui a été décidé en cours de route
- La recherche passe par `ytsearch` de yt-dlp : l'onglet « morceaux » de YouTube Music renvoie 0 résultat avec yt-dlp 2026.10.26. Le choix privilégie quand même les chaînes « - Topic » et la chaîne de l'artiste.
- L'analyse d'un lien Spotify, Deezer ou Apple ne lit que les métadonnées (rapide, même pour un album). La recherche sur YouTube se fait au moment du téléchargement.
- Une seule entrée dans l'interface : on colle n'importe quel lien. La carte affiche la pochette carrée, la plateforme et la mention « audio via YouTube ». Maquette : https://claude.ai/artifact/JM9NCfP1JYJmpLpGRuTkB6
- Limites : les mêmes que pour les vidéos (50 titres par playlist ou album, quotas par visiteur et par clé).
- Bot Telegram : un lien musique part directement en MP3, sans demander Vidéo ou Audio. Les albums et playlists renvoient vers le site, comme les playlists YouTube.
- Les playlists Apple Music ne sont pas publiques : refusées avec un message clair.

## Mis de côté (compte, cookie ou jeton nécessaire)

- **Deezer en FLAC** : demande un cookie ARL et du déchiffrement.
- **Tidal** : jeton de compte, même pour les métadonnées.
- **Qobuz** : jeton de session.
- **Apple Music en AAC 256 / ALAC** : FairPlay, impossible côté serveur.
- **Spotify en Ogg d'origine (librespot)** : compte nécessaire, et c'est un contournement de DRM. On ne le fait pas.

## Admin : suivi des utilisateurs, monitoring et blocage

But : voir qui utilise reclip (web et Telegram), combien chacun télécharge, repérer un abus ou un partage du lien à d'autres personnes, et bloquer ou débloquer qui on veut.

Ce qui existe déjà : chaque job enregistre `owner` (`web:<cookie>`, `key:<id>` ou `tg:<id Telegram>`) et `quota_key` (`ip:<adresse>` pour le web, IPv6 regroupé par /64). Les clés d'API ont `last_used_at` et `request_count`, et on peut déjà les révoquer. Il n'existe ni page d'activité ni blocage d'un appareil, d'une IP ou d'un compte Telegram.

### À faire
- [x] Enregistrer pour chaque job : IP, `User-Agent` résumé (navigateur, système, mobile ou ordinateur), source (web, Telegram, clé), taille du fichier et durée du traitement
- [x] Côté Telegram : transmettre au cœur le `@username`, le prénom et le nom en plus de l'id (le bot les reçoit à chaque message)
- [x] Page admin « Utilisateurs » : un appareil web (cookie) ou un compte Telegram par ligne, avec les IP vues, le nombre de téléchargements (jour, semaine, total), le volume en Go et la dernière activité
- [x] Repérer le partage : un même cookie vu depuis beaucoup d'IP, beaucoup de cookies derrière une même IP, des nouveaux appareils qui arrivent d'un coup
- [x] Bloquer et débloquer un appareil (cookie), une IP ou plage, ou un compte Telegram, avec un motif. Le blocage refuse les nouvelles analyses et les nouveaux jobs avec un message clair
- [x] Page admin « Serveur » : CPU, mémoire, disque utilisé par les fichiers, jobs en cours et en file, débit réseau. Le coût moyen d'un téléchargement (durée, CPU, Go)
- [x] Durée de conservation des IP et du journal d'activité (ex. 30 jours), puis purge automatique

### Ce qui a été décidé en cours de route
- Un « utilisateur » web est un navigateur (son cookie). L'IPv6 est regroupée par /64.
- Signes de partage : 5 adresses ou plus en 24 h pour un navigateur, 4 navigateurs ou plus sur une adresse en 7 jours, 10 nouveaux navigateurs ou plus en une heure. Ce sont des indices affichés, pas des blocages automatiques.
- Blocage par navigateur, compte Telegram, clé, adresse ou plage (au moins /16 en IPv4, /32 en IPv6). Le motif reste côté admin ; l'utilisateur voit un message neutre (code `blocked`).
- Le coût CPU par téléchargement est une moyenne depuis le dernier démarrage (temps CPU du cœur et de ses outils / téléchargements finis), pas une mesure par tâche.
- L'activité suit `RECLIP_HISTORY_DAYS` (30 jours) et est purgée par le nettoyage toutes les 10 minutes.
- Les pages admin s'ouvrent sur « Users ».

### Limites à garder en tête
- Sur le web, on n'obtient pas le nom d'une personne : seulement une IP, un cookie et le navigateur. Une même personne peut changer d'IP (4G) ou vider ses cookies. Le nom n'existe que côté Telegram (et seulement si la personne en a mis un).
- Derrière Cloudflare ou un proxy, l'IP réelle vient de l'en-tête transmis (déjà géré par `access/ip.rs`) : à vérifier sur le déploiement.
- Les IP sont des données personnelles : ne les garder que le temps utile.

