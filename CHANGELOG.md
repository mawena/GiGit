# Journal des modifications

Toutes les modifications notables de GiGit sont listées ici.
Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et le projet utilise le [versionnage sémantique](https://semver.org/lang/fr/).

## [Non publié]

### Ajouté

- Avatars des auteurs dans le graphe et le détail des commits, comme dans ungit : Gravatar, ou avatar GitHub pour les adresses `noreply`. Les images sont gardées en cache sur le disque, avec les initiales en repli. Les avatars peuvent être désactivés dans les réglages.
- Thème **Ardoise**, inspiré du mode sombre de Vuexy. Il devient le thème par défaut et le thème sombre du mode « Système ».

### Modifié

- Dans le graphe, les branches distantes et les tags ont chacun leur couleur dans tous les thèmes, comme dans ungit : bleu ou cyan pour les distantes, jaune pour les tags.

## [0.1.0] - 2026-10-26

Première version publique.

### Ajouté

- **Liste de dépôts** persistante :
  - ajout d'un dossier (sélecteur ou `gigit <dossier>`), retrait de la liste (le disque n'est jamais touché) ;
  - réordonnancement par glisser-déposer ;
  - pour chaque dépôt : branche courante, indicateur de modifications, commits en avance ou en retard.
- **Graphe des commits façon ungit** :
  - la ligne principale part de HEAD, avec une carte par commit ;
  - les autres branches ont chacune leur colonne ;
  - les noms des branches et des tags sont affichés à côté des nœuds ;
  - l'historique se charge par pages.
- **Détail d'un commit** : message, auteur, date, parents, fichiers et diff.
- **Indexation par fichier, par bloc ou par ligne**, dans le vrai index git.
- **Commit** (avec `Ctrl+Entrée`) et **amend**.
- **Annulation de modifications** par fichier, bloc ou ligne, avec confirmation.
- **Ajout au `.gitignore`.**
- **Fetch, pull, push et push forcé.** Le push forcé passe par `--force-with-lease --force-if-includes` et demande une confirmation.
- **Fetch automatique**, réglable.
- **Rafraîchissement automatique** quand des fichiers changent. Les fichiers ignorés par `.gitignore` ne déclenchent rien.
- **Interface en français et en anglais.**
- **Thèmes** : Sombre chaleureux, Nuit, Clair, Système.
- **Instance unique** : `gigit <dossier>` envoie le dossier à la fenêtre déjà ouverte.
- **Paquet Debian**, avec une icône et une entrée dans le menu des applications.

### Sécurité des données

- Les patchs d'indexation partielle sont reconstruits à l'octet près : CRLF et fichiers non UTF-8 sont pris en charge.
- Les chemins sont toujours traités littéralement : `f[1].txt` ne désigne jamais `f1.txt`.
- Les opérations confirmées ne s'exécutent que sur le dépôt pour lequel elles ont été confirmées.
- Les commandes lancées en arrière-plan n'ouvrent jamais de fenêtre de mot de passe.

[0.1.0]: https://github.com/mawena/GiGit/releases/tag/v0.1.0
