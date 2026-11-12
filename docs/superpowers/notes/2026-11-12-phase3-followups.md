# Phase 3 : décisions prises et suites à donner

Bilan de l'exécution du plan `docs/superpowers/plans/2026-11-12-phase3-access.md` sur la branche `feat/phase3-security`.

## Décisions prises en cours de route

| Décision | Pourquoi | Coût si c'était le mauvais choix |
|---|---|---|
| Limites visiteur par défaut : 300 tâches créées par heure et 200 actives (la spec dit 100 et 30) ; clés : 200 par heure et 100 actives (la spec dit 50) | Un lot de 12 liens et deux playlists de 50 doivent passer sans refus | Plus de tâches en file pour une IP abusive (une tâche en file ne coûte aucun processus) |
| Le site attend et réessaie seul après un refus 429 (jusqu'à 3 fois, attente de 120 s au plus) | Ne jamais laisser une carte en erreur à cause d'une limite | Le bouton « Download All » reste occupé pendant l'attente |
| Nouveaux codes d'erreur `unauthorized` (401) et `forbidden` (403) | La liste de la spec n'a aucun code pour une clé invalide ou un droit manquant | Une valeur de plus à gérer pour les clients |
| Seul `Authorization: Bearer …` compte comme une clé ; `Basic` et les autres schémas sont traités comme un visiteur | Les proxys protégés par mot de passe (Traefik, Dokploy, Caddy, nginx) transmettent cet en-tête | Un client qui envoie une clé sans `Bearer` est traité comme un visiteur |
| L'adresse publique déduite automatiquement ne sert qu'aux liens signés, jamais au contrôle `Origin` ; elle n'est apprise qu'après une requête acceptée ; un avertissement invite à fixer `RECLIP_PUBLIC_URL` ; `reclip-core url show|reset` | Sur une installation neuve, la première requête pourrait imposer une fausse adresse | Liens du futur bot faux tant que l'adresse n'est pas fixée |
| Le contrôle `Origin` compare aussi le schéma (https derrière un proxy https) et ne dispense que GET, HEAD et OPTIONS | Fermer les contournements par une page http ou une méthode inattendue | Un proxy qui n'envoie pas `X-Forwarded-Proto` doit fixer `RECLIP_PUBLIC_URL` |
| Quotas IPv6 comptés par bloc /64 | Un client contrôle tout son /64 et pourrait changer d'adresse à volonté | Des voisins sur le même /64 partagent leurs quotas |
| Au plus 16 résolutions DNS simultanées pour la protection SSRF, et une cible refusée ne consomme pas de quota | Des noms lents ne doivent pas saturer le serveur | Les analyses attendent si le DNS est lent |
| `X-Forwarded-For` : lecture de toutes les lignes, de droite à gauche, arrêt à la première entrée illisible | Une entrée forgée par le client ne doit ni choisir l'IP ni regrouper tout le monde sous l'IP du proxy | Aucun |
| Délégation Telegram : identifiant `tg:` sans zéro initial | Un seul propriétaire par utilisateur Telegram | Le bot doit envoyer l'identifiant tel quel |
| Liens signés valables 6 h par défaut (`RECLIP_LINK_TTL=21600`), jamais au-delà de l'expiration du fichier | La spec ne donne pas de valeur | Liens plus courts ou plus longs que souhaité |
| Durée d'une clé entre 1 et 36 500 jours | Au-delà, la date dépasse l'an 9999 et la clé paraît expirée | Aucun |
| Pas de plafond de flux SSE par visiteur | Un flux ne coûte presque rien et la création est limitée | Un abuseur peut garder beaucoup de flux ouverts |
| CSP sans `'unsafe-inline'` ; les attributs `style` du frontend sont passés en classes CSS | Spec §4.5 | Aucun |
| Les tests de bout en bout utilisent des URL à IP littérale (`https://93.184.216.34/slow`) | Le vrai binaire résout le DNS, et les faux noms sont refusés par la protection SSRF | Ces tests n'exercent plus la résolution de noms |
| Jeton admin généré affiché deux fois (log WARN et sortie d'erreur) | La copie sur la sortie d'erreur survit à `RUST_LOG=off` | Une ligne en double dans les logs |

## À faire plus tard

- Le compteur d'utilisation des clés n'est écrit qu'à l'utilisation suivante : au repos, `keys list` peut montrer des valeurs en retard, et jusqu'à 60 s d'utilisations sont perdues au redémarrage. Ajouter une écriture périodique ou à l'arrêt.
- Un `RECLIP_PUBLIC_URL` refusé est recopié tel quel dans l'avertissement : masquer la partie `user:motdepasse@` si quelqu'un en met une.
- Pendant une attente après un 429, le texte « Waiting N s » ne décompte pas.
- Le plafond de tâches actives n'est pas atomique avec l'insertion : des créations parallèles peuvent le dépasser de quelques unités.
- Une adresse publique déduite par erreur reste en mémoire jusqu'au redémarrage, même après `url reset`.
- Tests manquants : jeton `/d` falsifié ou expiré en intégration ; `Secure` posé seulement derrière un proxy de confiance ; repli HTTP/2 sur `:authority`.
- Fichiers proches de la limite de 300 lignes : `tests/access.rs` (313), `access/caller.rs`, `config.rs`. Les modules `cookie.rs` et `caller.rs` s'importent l'un l'autre : un `access/visitor.rs` clarifierait.
- Toutes les clés qui ont le droit `delegate` voient toutes les tâches `tg:*` (documenté) : à revoir si plusieurs bots coexistent.
