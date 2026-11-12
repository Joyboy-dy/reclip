# Phase 5 : suites à donner

Points connus après la revue finale de la phase 5 (bot Telegram DL Manager). Aucun ne bloque la mise en production.

## À vérifier avec un vrai bot

Aucun appel réel à Telegram n'a été fait ; les tests utilisent des faux. À tester une fois déployé :

- mode simple (jeton seul, `COMPOSE_PROFILES=telegram`) : `/start`, un lien YouTube et un lien Instagram en MP4 puis en MP3, un fichier de plus de 50 Mo (lien signé) ;
- passage en mode 2 Go (`setup.sh --update`, ou variables dans Dokploy après `logOut`) et un fichier de plus de 50 Mo envoyé directement ;
- 12 liens envoyés d'un coup : les boutons arrivent tous, à environ un message par seconde ;
- redémarrage du conteneur `bot` pendant un téléchargement : la livraison reprend ;
- `RECLIP_TELEGRAM_BOT_USERNAME` défini : le lien Telegram apparaît dans le pied de page du site.

## Reporté

- **Message final perdu après une rafale de 429** : si l'édition puis l'envoi du lien ou du texte final échouent tous les deux trois fois, l'entrée est retirée de l'état. Correction : rendre `Retry` quand `replace` échoue sur un 429.
- **Rechargement de la clé** : deux requêtes qui reçoivent un 401 en même temps ; la seconde voit la clé déjà remplacée et ne réessaie pas tout de suite (elle réussit au tour suivant). Correction : comparer avec la clé utilisée par la requête.
- **Miniatures en mode local** : une adresse `http://` vers le réseau local (192.168.x.x, localhost) est encore transmise à `sendPhoto`. Le serveur Bot API local pourrait la lire. Correction : refuser les hôtes privés, comme le contrôle SSRF du cœur.
- **Révocation des anciennes clés du bot** : toutes les clés actives nommées `telegram-bot` sont révoquées à la rotation, même sans le droit `delegate`. Correction : filtrer aussi sur ce droit.
- **Analyses simultanées** : aucun plafond du nombre d'analyses en cours, par utilisateur ou au total (seulement les limites par minute du niveau `tg`).
- **Petits points d'interface** : boutons laissés sur le message photo après un choix, ancien message de progression gardé après un envoi de secours, `answerCallbackQuery` et `deleteMessage` non réessayés sur 429.
- **État du bot** : `state.json` est écrit et synchronisé sur le disque sous le verrou.

## Hors phase 5

- Les tests propres à Linux (permissions du fichier de clé, arrêt sur SIGTERM) tournent en CI et dans un conteneur Linux ; ils ne tournent pas sous Windows.
