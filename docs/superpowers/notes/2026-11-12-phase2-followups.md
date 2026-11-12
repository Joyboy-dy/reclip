# Phase 2 : décisions prises et suites à donner

Bilan de l'exécution du plan `docs/superpowers/plans/2026-11-11-phase2-lifecycle.md` sur la branche `feat/phase2-lifecycle`.

## Décisions prises en cours de route

| Décision | Pourquoi | Coût si c'était le mauvais choix |
|---|---|---|
| Cookie visiteur et propriété des tâches avancés de la phase 3 à la phase 2 | Retrouver « ses » tâches après un rafraîchissement l'exige | Aucun |
| `Secure` suit `X-Forwarded-Proto` sans liste de proxys de confiance | La liste arrive en phase 3 | Au pire, un visiteur en HTTP simple perd la reprise après rafraîchissement |
| La progression affichée ne recule jamais et couvre vidéo + audio (2 flux en MP4) | Sinon la barre fait deux fois 0 → 100 % | La barre peut sauter une fois |
| Vitesse en Mo calculés sur 1024² | Cohérent avec l'exemple du plan | 5 % d'écart d'affichage |
| Au plus 3 flux SSE ouverts par page, les autres tâches sont interrogées toutes les 1,5 s | Les navigateurs limitent à 6 connexions par site en HTTP/1.1 | Mises à jour un peu moins vives au-delà de 3 tâches |
| Les tâches échouées ou annulées expirent aussi après `RECLIP_FILE_TTL` | Sinon la base grossit sans fin | Les cartes d'erreur disparaissent au bout de 24 h |
| Le nettoyage au démarrage ne supprime que des dossiers au nom de tâche (32 caractères hexadécimaux) | Ne jamais toucher un dossier de l'utilisateur | Aucun |
| yt-dlp tourne avec la priorité `nice 10` (unix) | Spec §3.3 : le site reste réactif pendant les téléchargements | Aucun |
| Contrôle de l'en-tête `Origin` sur les requêtes avec cookie reporté en phase 3 | `SameSite=Lax` bloque déjà les POST/DELETE d'un autre site | Faible exposition CSRF jusqu'à la phase 3 |

## À faire plus tard

- **Phase 3** : liste des proxys de confiance (`Secure` et vraie IP), contrôle `Origin`, limites par visiteur (nombre de tâches, de flux SSE), en-têtes de sécurité.
- Les tâches échouées ou annulées créées avant les correctifs de fin de phase gardent `expires_at` vide (seules les `done` ont été rattrapées par la migration 0003).
- Après expiration, une carte échouée revient sans son message d'erreur.
- `expire_due` ouvre une transaction différée : sous forte concurrence d'écriture, SQLite peut répondre `BUSY` (le cycle suivant rattrape).
- Tests manquants : forcer `Lagged` sur le flux SSE ; libération du flux à la déconnexion du client ; double clic sur Retry.
- Le job e2e de la CI recompile ce que les jobs `rust` et `web` ont déjà construit (plus lent, sans risque).
