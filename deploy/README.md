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

## Mise à jour de yt-dlp

Au démarrage, le conteneur exécute `yt-dlp -U` (désactivable avec `RECLIP_NO_UPDATE=1`). Redémarrez le conteneur pour récupérer la dernière version.
