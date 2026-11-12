# Déploiement de ReClip

Image Docker unique (`reclip-core`) : serveur Rust, interface web, yt-dlp, ffmpeg et Deno.
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

Les fichiers terminés restent téléchargeables pendant `RECLIP_FILE_TTL` secondes (défaut `86400`, soit 24 h), puis sont supprimés. Un nettoyage passe toutes les `RECLIP_CLEANUP_INTERVAL` secondes (défaut `600`). Les tâches expirées sont retirées de la base SQLite 7 jours après leur expiration.

Dans l'interface, chaque tâche affiche sa progression en direct et peut être annulée ou retirée. Les cartes sont restaurées après un rafraîchissement de la page.

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
- `--expires-days N` : la clé expire après N jours.

Utilisation :

```bash
curl -H "Authorization: Bearer rck_xxxxxxxx_..." https://reclip.example.com/api/v1/jobs
```

Le droit `delegate` permet à un bot d'agir pour des utilisateurs Telegram avec l'en-tête `X-Reclip-On-Behalf-Of: tg:<id>`. Attention : toute clé `delegate` voit les tâches de tous les utilisateurs `tg:*`. Ne la donnez qu'à votre propre bot.

Mode privé : avec `RECLIP_PUBLIC_WEB=false`, toute l'API exige une clé, sauf `GET /api/v1/config`.

Erreurs : `401 unauthorized` (clé absente ou invalide), `403 forbidden` (droit manquant) ou `403 forbidden_target` (adresse refusée), `429 rate_limited` (voir Limites).

Liens signés : `POST /api/v1/jobs/{id}/link` renvoie `{url, expires_at}`. L'adresse `GET /d/{token}` se télécharge sans clé et expire après `RECLIP_LINK_TTL` secondes (défaut `21600`) ou avec le fichier.

## Limites

Chaque appelant a trois limites : analyses par minute, tâches créées par heure, tâches actives en même temps. Elles dépendent de son type :

| Type | Analyses/min | Tâches/h | Actives |
|---|---|---|---|
| `WEB` (navigateur) | 120 | 300 | 200 |
| `KEY` (clé API) | 120 | 200 | 100 |
| `TG` (bot Telegram) | 30 | 40 | 10 |

Les variables se nomment `RECLIP_LIMIT_<TYPE>_PROBES_PER_MIN`, `RECLIP_LIMIT_<TYPE>_JOBS_PER_HOUR` et `RECLIP_LIMIT_<TYPE>_ACTIVE`. La valeur `0` supprime la limite. Une clé créée avec `--unlimited` n'est pas limitée.

Au-delà d'une limite, l'API répond `429 rate_limited` avec les en-têtes `Retry-After`, `RateLimit-Limit`, `RateLimit-Remaining` et `RateLimit-Reset`.

## Adresse publique et proxy

Derrière un proxy inverse (Traefik sur Dokploy, nginx, Caddy), ReClip ne croit les en-têtes `X-Forwarded-*` que s'ils viennent d'une adresse de `RECLIP_TRUSTED_PROXIES` (par défaut : loopback et réseaux privés).

- Définissez `RECLIP_PUBLIC_URL` (ex. `https://reclip.example.com`). C'est le réglage recommandé.
- Si elle est vide, ReClip l'apprend à la première requête passée par un proxy de confiance (`X-Forwarded-Proto` et `X-Forwarded-Host`), écrit un WARN dans les logs et ne s'en sert que pour les liens signés.
- Le proxy doit envoyer `X-Forwarded-Proto` (Traefik et Dokploy le font). Sinon, définissez `RECLIP_PUBLIC_URL`, faute de quoi les requêtes POST et DELETE des visiteurs sont refusées (contrôle du schéma de l'en-tête Origin).

Pour voir ou effacer l'adresse apprise :

```bash
docker compose -f deploy/compose.yaml exec core reclip-core url show
docker compose -f deploy/compose.yaml exec core reclip-core url reset
```

Redémarrez le conteneur après `url reset`. Si `RECLIP_PUBLIC_URL` est définie, elle continue de s'appliquer.

## Utilisation de l'API en ligne de commande

L'API rattache les tâches à un cookie anonyme `reclip_client`. Un script `curl` doit donc conserver ses cookies d'une requête à l'autre, sinon il ne retrouvera pas ses tâches :

```bash
curl -c cookies.txt -b cookies.txt http://localhost:8899/api/v1/jobs
```

## Mise à jour de yt-dlp

Au démarrage, le conteneur exécute `yt-dlp -U` (désactivable avec `RECLIP_NO_UPDATE=1`). Redémarrez le conteneur pour récupérer la dernière version.
