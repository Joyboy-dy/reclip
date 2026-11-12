# Déploiement de ReClip

Image Docker principale (`reclip-core`) : serveur Rust, interface web, yt-dlp, ffmpeg et Deno. Une seconde image optionnelle, `reclip-bot`, contient le bot Telegram (voir « Bot Telegram »).
Contexte de build = racine du dépôt.

## Lancement local

```bash
cp deploy/.env.example deploy/.env   # optionnel : adaptez les valeurs
docker compose -f deploy/compose.yaml up --build
```

Ouvrez <http://localhost:8899>. Pour un autre port hôte : `RECLIP_PORT=18900 docker compose -f deploy/compose.yaml up --build`.
Les téléchargements et la base SQLite sont dans le volume nommé `data` (`/data` dans le conteneur).
Le dossier des téléchargements appartient à ReClip : au démarrage, il supprime les dossiers de tâches (32 caractères hexadécimaux) qu'il ne connaît plus. Ne le partagez pas avec d'autres fichiers.
Pour tout supprimer : `docker compose -f deploy/compose.yaml down -v`.

## Dokploy

Passez le service de type « Application / Dockerfile » à **Compose** :

1. Type : **Compose**, source Git du dépôt, chemin du fichier : `deploy/compose.yaml`.
2. Onglet **Environment** : renseignez les variables de `deploy/.env.example` à modifier (aucun fichier `.env` n'est lu depuis le dépôt).
3. Onglet **Domains** : service `core`, port **8899**.
4. Déployez. Le conteneur est `healthy` quand `/healthz` répond.

## Image GHCR

À chaque push sur `main`, la CI publie l'image sur GitHub Container Registry :

- `ghcr.io/<propriétaire>/reclip-core:latest` : dernière version de `main` ;
- `ghcr.io/<propriétaire>/reclip-core:<sha>` : version exacte d'un commit, utile pour revenir en arrière.

Le propriétaire est en minuscules (ex. `ghcr.io/joyboy-dy/reclip-core:latest`).

Pour que Dokploy utilise cette image au lieu de la construire :

1. Dans la copie du `compose.yaml` que Dokploy déploie, remplacez le bloc `build:` du service `core` par la ligne `image: ghcr.io/<propriétaire>/reclip-core:latest`. Les variables d'environnement restent identiques.
2. Le paquet est privé par défaut. Soit vous le rendez public (page du paquet sur GitHub, réglages « Change visibility »), soit vous ajoutez un registre `ghcr.io` dans Dokploy avec un identifiant GitHub et un jeton ayant le droit `read:packages`.
3. Redéployez. Pour revenir à une version précise, remplacez `latest` par le SHA du commit voulu.

Le build local (`docker compose -f deploy/compose.yaml up --build`) reste disponible.

## Cookies YouTube

YouTube refuse la plupart des IP de datacenter (*« Sign in to confirm you're not a bot »*). Pour contourner :

1. Utilisez un compte Google secondaire : les comptes utilisés avec yt-dlp peuvent être signalés.
2. Dans une fenêtre privée, connectez-vous à YouTube, exportez les cookies avec une extension comme *Get cookies.txt LOCALLY*, puis fermez la fenêtre sans naviguer davantage (YouTube fait tourner les cookies des sessions ouvertes).
3. Copiez le fichier à côté de `compose.yaml` sous le nom `cookies.txt`, décommentez la ligne du montage dans `compose.yaml` et définissez `RECLIP_COOKIES=/run/secrets/cookies.txt`.

ReClip travaille sur une copie par requête : le fichier d'origine n'est jamais modifié. Quand les cookies expirent, le message d'erreur le signale : exportez-en de nouveaux.

YouTube exige aussi un runtime JavaScript : l'image embarque Deno 2.5.6.

## File d'attente et conservation des fichiers

Les téléchargements passent par une file d'attente équitable :

- `RECLIP_MAX_CONCURRENT` (défaut `4`) limite le nombre de téléchargements simultanés, tous sites confondus.
- `RECLIP_MAX_PER_SITE` (défaut `2`) limite le nombre de téléchargements simultanés pour un même site, afin qu'un seul site ne monopolise pas la file.

Les fichiers terminés restent téléchargeables pendant `RECLIP_FILE_TTL` secondes, puis sont supprimés (détails dans « Stockage »).

Dans l'interface, chaque tâche affiche sa progression en direct et peut être annulée ou retirée. Les cartes sont restaurées après un rafraîchissement de la page.

## Stockage

- Durée de vie d'un fichier terminé : `RECLIP_FILE_TTL` secondes (défaut `21600`, soit 6 h). Un nettoyage passe toutes les `RECLIP_CLEANUP_INTERVAL` secondes (défaut `600`).
- Taille maximale du dossier des téléchargements : `RECLIP_MAX_DISK` (défaut `5G`). Les valeurs acceptent les suffixes `K`, `M` et `G`, ou un nombre d'octets. Quand la limite est atteinte, les fichiers terminés les plus anciens sont supprimés en premier. La limite est souple : les téléchargements en cours et la copie finale par ffmpeg peuvent la dépasser brièvement.
- Historique : les titres restent affichés pendant `RECLIP_HISTORY_DAYS` jours (défaut `30`), même après la suppression du fichier.

Le volume `data` grossit au maximum jusqu'à `RECLIP_MAX_DISK`, plus la base SQLite et les fichiers en cours de téléchargement. Si l'hôte a peu d'espace, baissez `RECLIP_MAX_DISK` ou `RECLIP_FILE_TTL`.

## Page admin

La page d'administration est sur `/admin`. Elle n'est liée nulle part sur le site : partagez l'adresse seulement avec les administrateurs.

1. Listez les adresses autorisées dans `RECLIP_ADMIN_EMAILS`, séparées par des virgules, en minuscules (ex. `ann@example.com,bob@example.com`).
2. Définissez `RECLIP_ADMIN_DEFAULT_PASSWORD` : c'est le mot de passe initial de chaque administrateur. Il ne sert qu'à sa **première connexion** sur `/admin`. ReClip demande alors de le remplacer par un mot de passe d'au moins 12 caractères.
3. Une fois chaque administrateur connecté, retirez `RECLIP_ADMIN_DEFAULT_PASSWORD` de Dokploy ou de `deploy/.env` et redéployez. Ne le versionnez jamais dans Git.

Le jeton admin (`RECLIP_ADMIN_TOKEN`) et la commande `reclip-core keys` restent valables pour les scripts : la page admin ne les remplace pas.

## Serveur MCP et WebMCP

Le serveur MCP est sur `/mcp` (MCP en HTTP streamable). Il exige une clé API : `Authorization: Bearer rck_…`. Les droits et les limites de la clé sont appliqués comme sur l'API REST. La délégation Telegram n'est pas acceptée sur `/mcp`. Sans clé ou avec une clé invalide ou révoquée, la réponse est `401`.

Créez une clé avec la commande `keys create` ci-dessus, ou sur la page `/admin`. Puis, pour Claude Code :

```bash
claude mcp add --transport http reclip https://reclip.example.com/mcp --header "Authorization: Bearer rck_…"
```

Pour Claude Desktop, l'écran de connecteur personnalisé accepte une URL et des réglages OAuth facultatifs, pas d'en-têtes personnalisés. Utilisez le pont `mcp-remote` dans `claude_desktop_config.json` :

```json
{
  "mcpServers": {
    "reclip": {
      "command": "npx",
      "args": ["mcp-remote", "https://reclip.example.com/mcp", "--header", "Authorization:${RECLIP_AUTH}"],
      "env": { "RECLIP_AUTH": "Bearer rck_xxxxxxxx_..." }
    }
  }
}
```

La valeur de l'en-tête passe par une variable d'environnement, car certaines plateformes déforment les espaces dans les arguments.

Pour un autre client : tout client qui peut envoyer un en-tête personnalisé se connecte directement. Un client qui n'en est pas capable ne peut pas utiliser `/mcp`.

Les six outils (`probe_media`, `start_download`, `list_downloads`, `get_download`, `cancel_download`, `get_transcript`) sont aussi exposés par WebMCP sur la page publique, quand le navigateur gère l'API (Chrome, derrière un drapeau ou un origin trial). WebMCP n'utilise aucune clé : il agit avec le cookie du visiteur et ses limites. Les téléchargements lancés par un agent apparaissent sous forme de cartes sur la page, et le navigateur ne les enregistre pas automatiquement.

`RECLIP_WEBMCP_TRIAL_TOKEN` (vide par défaut) : quand il est défini, chaque page HTML porte la balise `<meta http-equiv="origin-trial">` avec ce jeton. Il est fourni par le fournisseur du navigateur pour l'origin trial. Vide, aucune balise n'est ajoutée : WebMCP reste disponible dans les navigateurs où l'API est activée par un drapeau.

## Jeton admin

L'API d'administration (`/api/v1/admin/keys`) est protégée par un jeton admin.

- Si `RECLIP_ADMIN_TOKEN` est vide, ReClip génère un jeton au premier démarrage et l'affiche une seule fois, dans les logs (niveau WARN) et sur la sortie d'erreur. Seul son hash est stocké : il ne peut pas être retrouvé ensuite. Relevez-le avec `docker compose -f deploy/compose.yaml logs core`.
- Pour choisir le jeton, définissez `RECLIP_ADMIN_TOKEN` (32 caractères aléatoires minimum, par exemple `openssl rand -hex 32`).

Appel de l'API d'administration :

```bash
curl -H "Authorization: Bearer <jeton admin>" https://reclip.example.com/api/v1/admin/keys
```

Routes : `POST /api/v1/admin/keys` (créer), `GET /api/v1/admin/keys` (lister), `DELETE /api/v1/admin/keys/{id}` (révoquer).

## Clés API

Les clés ont la forme `rck_<id>_<secret>`. Le secret n'est affiché qu'à la création : notez la clé tout de suite.

```bash
docker compose -f deploy/compose.yaml exec core reclip-core keys create --name agent
docker compose -f deploy/compose.yaml exec core reclip-core keys list
docker compose -f deploy/compose.yaml exec core reclip-core keys revoke <id>
```

Options de `keys create` :

- `--scopes probe,jobs:create,jobs:read,jobs:cancel` : droits de la clé (c'est la valeur par défaut). Le droit `delegate` s'ajoute explicitement.
- `--unlimited` : la clé n'est pas soumise aux limites.
- `--expires-days N` : la clé expire après N jours (de 1 à 36500).

Utilisation :

```bash
curl -H "Authorization: Bearer rck_xxxxxxxx_..." https://reclip.example.com/api/v1/jobs
```

Seul le schéma `Bearer` compte comme une clé. Un en-tête `Authorization` d'un autre type (par exemple les identifiants Basic transmis par un proxy) est ignoré : la requête est traitée comme celle d'un visiteur.

Le droit `delegate` permet à un bot d'agir pour des utilisateurs Telegram avec l'en-tête `X-Reclip-On-Behalf-Of: tg:<id>`. Attention : toute clé `delegate` voit les tâches de tous les utilisateurs `tg:*`. Ne la donnez qu'à votre propre bot.

Mode privé : avec `RECLIP_PUBLIC_WEB=false`, toute l'API exige une clé, sauf `GET /api/v1/config`.

Erreurs : `401 unauthorized` (clé invalide, expirée ou révoquée, requête sans clé en mode privé, mauvais jeton admin), `403 forbidden` (droit manquant, délégation refusée faute du droit `delegate`, ou requête sans clé dont l'en-tête Origin manque ou désigne un autre site), `403 forbidden_target` (adresse refusée), `429 rate_limited` (voir Limites).

Liens signés : `POST /api/v1/jobs/{id}/link` renvoie `{url, expires_at}`. L'adresse `GET /d/{token}` se télécharge sans clé et expire après `RECLIP_LINK_TTL` secondes (défaut `21600`) ou avec le fichier.

## Limites

Chaque appelant a trois limites : analyses par minute, tâches créées par heure, tâches actives en même temps. Elles dépendent de son type :

| Type | Analyses/min | Tâches/h | Actives |
|---|---|---|---|
| `WEB` (navigateur) | 120 | 300 | 200 |
| `KEY` (clé API) | 120 | 200 | 100 |
| `TG` (bot Telegram) | 30 | 40 | 10 |

Les variables se nomment `RECLIP_LIMIT_<TYPE>_PROBES_PER_MIN`, `RECLIP_LIMIT_<TYPE>_JOBS_PER_HOUR` et `RECLIP_LIMIT_<TYPE>_ACTIVE`. La valeur `0` supprime la limite. Une clé créée avec `--unlimited` n'est pas limitée. Les visiteurs du navigateur sont comptés par adresse IP (par préfixe /64 en IPv6).

Au-delà d'une limite, l'API répond `429 rate_limited` avec les en-têtes `Retry-After`, `RateLimit-Limit`, `RateLimit-Remaining` et `RateLimit-Reset`. Quand `Retry-After` ne dépasse pas 120 secondes, l'interface web attend puis réessaie, jusqu'à 3 fois.

## Adresse publique et proxy

Derrière un proxy inverse (Traefik sur Dokploy, nginx, Caddy), ReClip ne croit les en-têtes `X-Forwarded-*` que s'ils viennent d'une adresse de `RECLIP_TRUSTED_PROXIES` (par défaut : loopback et réseaux privés).

- Définissez `RECLIP_PUBLIC_URL` (ex. `https://reclip.example.com`). C'est le réglage recommandé. Schéma et hôte seulement, sans chemin : sinon elle est ignorée et un WARN est écrit dans les logs.
- Si elle est vide, ReClip l'apprend à la première requête passée par un proxy de confiance (`X-Forwarded-Proto` et `X-Forwarded-Host`), écrit un WARN dans les logs et ne s'en sert que pour les liens signés.
- Le proxy doit envoyer `X-Forwarded-Proto` (Traefik et Dokploy le font). Sinon, définissez `RECLIP_PUBLIC_URL`, faute de quoi les requêtes POST et DELETE des visiteurs sont refusées (contrôle du schéma de l'en-tête Origin).
- Avec nginx, le réglage par défaut `proxy_set_header Host $proxy_host` sans `X-Forwarded-Host` provoque aussi ce refus (403), sauf si `RECLIP_PUBLIC_URL` est définie.
- Avec Cloudflare devant Traefik, configurez `forwardedHeaders.trustedIPs` de Traefik avec les plages d'adresses de Cloudflare. Sinon, l'adresse vue est celle du nœud Cloudflare et les visiteurs qui passent par lui partagent les mêmes limites.

Pour voir ou effacer l'adresse apprise :

```bash
docker compose -f deploy/compose.yaml exec core reclip-core url show
docker compose -f deploy/compose.yaml exec core reclip-core url reset
```

Redémarrez le conteneur après `url reset`. Si `RECLIP_PUBLIC_URL` est définie, elle continue de s'appliquer.

## Bot Telegram

Le bot est un second conteneur (`reclip-bot`, profil compose `telegram`). Il n'appelle que l'API du cœur et n'a jamais accès à la base. Il parle en privé uniquement : les groupes et les canaux sont ignorés.

### Créer le bot

1. Dans Telegram, ouvrez [@BotFather](https://t.me/BotFather) et envoyez `/newbot`.
2. Nom affiché : `DL Manager`. Choisissez un nom d'utilisateur qui finit par `bot`.
3. Copiez le jeton que BotFather renvoie. Ne le mettez jamais dans git.
4. Pour le lien dans le pied de page du site, mettez le nom d'utilisateur (sans le `@`) dans `RECLIP_TELEGRAM_BOT_USERNAME`.

### Mode simple (jeton seul)

Le bot parle à `https://api.telegram.org`. Telegram limite alors les envois à 50 Mo. Au-delà, le bot envoie un lien de téléchargement signé.

Dans `deploy/.env` (ou dans l'onglet Environment de Dokploy) :

```
TELEGRAM_BOT_TOKEN=<jeton>
COMPOSE_PROFILES=telegram
```

Puis `docker compose -f deploy/compose.yaml --env-file deploy/.env up -d --build`.

### Mode 2 Go (serveur local)

Le profil `telegram-local` démarre un serveur Bot API local. Le bot peut alors envoyer des fichiers jusqu'à 2 000 Mo.

1. Sur [my.telegram.org](https://my.telegram.org), ouvrez « API development tools » et créez une application. Notez `api_id` et `api_hash`.
2. Telegram exige un appel `logOut` unique sur le serveur officiel avant de passer au serveur local. Le script s'en charge :

   ```
   sh deploy/setup.sh deploy/.env
   ```

   Il lit `TELEGRAM_BOT_TOKEN`, `TELEGRAM_API_ID`, `TELEGRAM_API_HASH` et `RECLIP_PUBLIC_URL` dans l'environnement, ou les demande. Il refuse d'écraser un fichier existant, n'affiche jamais le jeton et écrit le fichier en mode 600. Avec `--no-logout`, il saute l'appel `logOut`.
3. À la main, sans le script : `curl -X POST https://api.telegram.org/bot<jeton>/logOut`, puis dans `deploy/.env` :

   ```
   TELEGRAM_BOT_TOKEN=<jeton>
   TELEGRAM_API_ID=<api_id>
   TELEGRAM_API_HASH=<api_hash>
   TELEGRAM_API_URL=http://telegram-bot-api:8081
   COMPOSE_PROFILES=telegram,telegram-local
   ```

Après `logOut`, le jeton ne marche plus sur le serveur officiel pendant une dizaine de minutes. Pour revenir au mode simple, retirez `TELEGRAM_API_URL` et le profil `telegram-local`.

### Clé et état

Le cœur crée la clé du bot (`telegram-bot`) au démarrage et l'écrit dans `/run/reclip/bot.key`, un volume partagé en lecture seule avec le bot. Mettre `RECLIP_BOT_KEY_FILE` à vide désactive la création. L'état des téléchargements suivis est dans le volume `bot-data` (`/var/lib/reclip-bot/state.json`) : le bot reprend le suivi après un redémarrage.

Variables du bot : `TELEGRAM_BOT_TOKEN`, `TELEGRAM_API_URL`, `RECLIP_CORE_URL`, `RECLIP_BOT_KEY_FILE`, `RECLIP_BOT_STATE`, `RECLIP_BOT_MAX_UPLOAD` (voir `deploy/.env.example`).

## Utilisation de l'API en ligne de commande

Les scripts et les agents utilisent une clé API (voir « Clés API » ci-dessus et la section « API for agents » du README principal). Sans clé, l'API traite l'appelant comme un navigateur : les requêtes POST et DELETE exigent un en-tête Origin qui désigne le site, donc un simple `curl` reçoit `403 forbidden`.

## Mise à jour de yt-dlp

Au démarrage, le conteneur exécute `yt-dlp -U` (désactivable avec `RECLIP_NO_UPDATE=1`). Redémarrez le conteneur pour récupérer la dernière version.
