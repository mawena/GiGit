# GiGit — Document de conception

Date : 2026-10-26
Statut : en attente de validation

## 1. Intention et pourquoi

L'utilisateur se sert d'ungit (installé en version 1.5.24 dans `/usr/local/lib/node_modules/ungit`) pour visualiser et manipuler ses dépôts git. Aujourd'hui, regarder un projet demande trois étapes : aller dans le dossier, lancer `ungit`, puis ouvrir l'interface dans un navigateur. C'est trop long pour un simple coup d'œil, surtout quand on passe d'un projet à l'autre.

GiGit est une **application de bureau native en Rust**. Elle reprend **toutes les fonctionnalités d'ungit**, dans une belle interface conviviale, et ajoute ce qui manque : **une liste de dépôts persistante sur le côté**. On y ajoute et on y retire des dossiers, et un clic suffit pour passer d'un dépôt à l'autre, sans terminal ni navigateur.

L'application est réussie si l'utilisateur ouvre GiGit au lieu d'ungit pour son travail git quotidien, que ce soit pour regarder ou pour agir.

## 2. Objectifs

- Parité fonctionnelle avec ungit (liste complète en §5).
- Gestion multi-dépôts : ajouter et retirer des dossiers dans une barre latérale qui persiste entre les lancements.
- Une interface belle, fluide et conviviale, au thème **« Sombre chaleureux »** choisi par l'utilisateur (§6).
- Démarrage rapide, distribuée sous forme d'un seul binaire.
- Une interface qui ne se fige jamais : les opérations git tournent hors du thread d'interface.
- Livraison sous forme de paquet **`.deb`**.

## 3. Hors périmètre

- Le système de plugins d'ungit et son mode serveur web, inutiles dans une application de bureau.
- Windows et macOS ne sont pas visés pour cette version : la cible est Linux (Ubuntu, X11 et Wayland). Le code ne doit toutefois rien faire qui empêche un portage plus tard.
- Une intégration à des plateformes d'hébergement (GitHub ou GitLab : PR, issues).
- Un éditeur de texte complet. Seul le `.gitignore` est éditable dans l'application.

## 4. Anti-objectifs (échec même si « ça marche »)

- Une interface qui se fige pendant un fetch, un push ou le chargement d'un gros historique.
- Une action destructrice (reset hard, force push, suppression de branche, discard) exécutée sans confirmation.
- Une erreur git avalée ou incompréhensible : l'erreur doit s'afficher en clair, avec la sortie de git.
- Retirer un dépôt de la liste qui toucherait au disque. Cela ne doit **jamais** supprimer de fichiers.
- Ne pas respecter la configuration git de l'utilisateur (identifiants SSH, credential helpers, hooks, `user.name`, éditeur ou outil de merge).
- Une interface « fonctionnelle mais moche » ou chargée : la beauté et la clarté font partie de la demande.

## 5. Fonctionnalités (parité ungit + ajouts)

Liste établie à partir des routes de `ungit/source/git-api.js` et des composants d'ungit.

### 5.1 Dépôts (ajout GiGit)
- Barre latérale listant les dépôts enregistrés : nom du dossier, branche courante, badges (modifications non commitées ; commits en avance et en retard sur l'upstream).
- **Ajouter un dossier** avec un sélecteur natif. Si le dossier n'est pas un dépôt git, proposer `git init` (route ungit `/init`) ou un clone (route `/clone`) vers ce dossier.
- **Retirer** un dépôt de la liste (clic droit), sans aucune action sur le disque.
- Réordonner la liste par glisser-déposer (choix d’implémentation).
- Au lancement, rouvrir le dernier dépôt affiché.
- La CLI `gigit [chemin]` ouvre le chemin donné et l'ajoute à la liste s'il n'y est pas déjà.
- Signaler un dépôt dont le dossier a disparu (grisé, retirable), sans planter.

### 5.2 Graphe des commits
- Graphe de l'historique avec des couloirs colorés par branche, les étiquettes de branches locales et distantes, les tags et HEAD.
- Chargement progressif ou paginé (comme `/gitlog` avec limite et skip) pour rester fluide sur les gros dépôts.
- Clic sur un commit : panneau de détail (hash, auteur, date, message complet, parents, fichiers modifiés, diff par fichier). Équivalent de `/show` et du composant `commitdiff`.
- Option « afficher les commits non atteignables par une ref » : hors périmètre, sauf si c'est simple.

### 5.3 Zone de travail (staging et commit)
- Liste des fichiers modifiés, nouveaux, supprimés, renommés et en conflit (`/status`).
- Diff texte (`/diff`), avec un choix entre vue unifiée et vue côte à côte (choix d’implémentation), et diff d'images, avant et après (`/diff/image`).
- Cocher ou décocher des fichiers pour le commit. **Staging partiel par hunk**, comme ungit qui permet de sélectionner des lignes ou des hunks.
- Annuler les modifications d'un fichier (`/discardchanges`), avec confirmation.
- Ajouter un fichier au `.gitignore` (`/ignorefile`).
- Commit avec message, et option **amend** (`/commit`).
- Éditer le `.gitignore` (`/gitignore` GET et PUT).

### 5.4 Branches, tags et HEAD
- Lister les branches locales et distantes et les tags (`/branches`, `/remote/branches`, `/tags`, `/remote/tags`, `/refs`).
- Créer une branche ou un tag sur n'importe quel commit. Supprimer une branche ou un tag, locaux ou distants, avec confirmation.
- Checkout d'une branche, d'un tag ou d'un commit (`/checkout`).
- Reset soft, mixed ou hard d'une branche vers un commit (`/reset`), avec confirmation pour hard.
- Déplacer une branche par **glisser-déposer** sur un commit, avec un menu « Déplacer ici / Rebase ici / Merge ici », comme ungit.

### 5.5 Fusion et réécriture
- Merge (`/merge`), avec abort et continue (`/merge/abort`, `/merge/continue`).
- Rebase (`/rebase`), avec abort et continue (`/rebase/abort`, `/rebase/continue`).
- Squash (`/squash`).
- Cherry-pick (`/cherrypick`).
- Revert (`/revert`).
- **Conflits** : bandeau d'état (merge, rebase ou cherry-pick en cours), liste des fichiers en conflit, bouton « marquer comme résolu » (`/resolveconflicts`), bouton pour lancer l'outil de merge configuré (`/launchmergetool`).

### 5.6 Distants
- Fetch (`/fetch`), pull (fetch puis merge ou rebase selon la config git de l'utilisateur), push (`/push`), et **force push** avec confirmation.
- Lister, ajouter et supprimer des remotes (`/remotes`, `/remotes/:name`).
- Clone d'une URL vers un dossier (`/clone`). Le nouveau dépôt est ajouté à la liste.
- L'authentification passe par la configuration git de l'utilisateur (agent SSH, credential helper). Si git demande un identifiant ou un mot de passe en HTTPS, une boîte de dialogue le demande dans l'application, comme le composant `login` d'ungit.

### 5.7 Stash
- Lister, créer (avec message optionnel), appliquer, pop et supprimer (`/stashes`, `/stashes/:id`). Voir le diff d'un stash.

### 5.8 Sous-modules
- Lister (`/submodules`), ajouter (`/submodules/add`), mettre à jour (`/submodules/update`). Ouvrir un sous-module comme un dépôt dans GiGit.

### 5.9 Transverse
- **Rafraîchissement automatique** : surveiller le dossier du dépôt (et `.git`) et recharger l'état ou le graphe après une courte pause. Un bouton de rafraîchissement manuel existe aussi.
- Panneau d'erreurs git (équivalent de `gitErrors`) : message lisible, avec la commande et la sortie complète repliables.
- Indicateur d'activité pendant une opération longue.
- Raccourcis clavier pour les actions courantes : commit, fetch, push, rafraîchir, changer de dépôt (choix d’implémentation).

## 6. Interface

### 6.1 Disposition (validée sur maquette)
Trois colonnes :
- **Gauche** : la liste des dépôts, avec le bouton « + Ajouter un dossier » en bas.
- **Centre** : une barre d'outils (nom du dépôt, branche, Fetch, Pull, Push avec le nombre de commits à pousser), puis le graphe des commits avec les étiquettes de refs, le message et la date relative.
- **Droite** : les fichiers modifiés avec leur pastille d'état, le message de commit et le bouton Commit. Quand un commit est sélectionné dans le graphe, le panneau de droite (ou une zone basse, au choix du builder) montre son détail et son diff.

Les menus contextuels (clic droit) sur les commits, les branches, les fichiers et les dépôts donnent accès aux actions de la §5.

Maquette de référence : `.superpowers/brainstorm/*/content/style-visuel.html` (option C).

### 6.2 Thème « Sombre chaleureux »
Inspiré de la palette Catppuccin Mocha :
- fond `#1e1e2e`, barre latérale `#181825`, panneaux `#24273a`, bordures `#313244`, sélection `#313244` ;
- texte `#cdd6f4`, texte secondaire `#9399b2` ;
- accent principal pêche `#f5a97f`. Couleurs des couloirs du graphe : pêche, lavande `#cba6f7`, vert `#a6e3a1`, puis d'autres teintes Catppuccin (bleu, jaune, rose, ciel…) en rotation ;
- états des fichiers : modifié jaune, ajouté vert, supprimé rouge, en conflit rouge vif ou orange.

Coins arrondis, espacements généreux, typographie sans-serif moderne (Inter ou équivalent embarqué).

### 6.3 Thèmes au choix (décidé par l'utilisateur)
Le thème se choisit dans les réglages et s'applique immédiatement, sans redémarrage :
- **Sombre chaleureux** (par défaut), décrit en §6.2 ;
- **Nuit** : sombre bleuté (fond `#0f1117`, accent `#7c9cff`, couloirs bleu, rose `#f78fb3`, vert `#5fd3a6`). C'est l'option A de la maquette ;
- **Clair** : fond `#f7f8fa`, panneaux blancs, accent `#3b6cf6`. C'est l'option B de la maquette ;
- **Système** : suit le réglage clair ou sombre du bureau, et prend « Clair » ou « Sombre chaleureux ».

Les thèmes sont définis comme des jeux de couleurs nommés (tokens). En ajouter un plus tard ne doit demander qu'une nouvelle palette (« etc. » dans la demande de l'utilisateur).

### 6.4 Réglages
Une fenêtre ou un panneau de réglages regroupe : la langue, le thème, la taille de police de l'interface, l'intervalle du fetch automatique (ou sa désactivation) et la confirmation des actions dangereuses (toujours active pour reset hard, force push et suppression). Ces réglages sont enregistrés dans `config.toml`.

## 7. Choix techniques (validés par l’utilisateur)

- **Langage** : Rust stable (1.99 installé).
- **Interface** : `egui` / `eframe`. Natif, rapide, binaire unique, et adapté pour dessiner le graphe sur mesure.
- **Moteur git** : exécution de la **CLI `git`** (`std::process::Command`) et analyse de sa sortie, comme ungit (`git-parser.js` sert de référence). Cela garantit le respect de la config, des hooks et des identifiants de l'utilisateur. Formats de sortie stables : `--porcelain=v2 -z` pour status, format `log` personnalisé avec séparateurs NUL, etc.
- **Concurrence** : les commandes git tournent sur des threads de travail. L'interface communique avec eux par des canaux et ne bloque jamais. Il faut éviter de lancer deux opérations d'écriture git en même temps sur le même dépôt.
- **Surveillance des fichiers** : crate `notify`, avec debounce.
- **Sélecteur de dossier** : crate `rfd`.
- **Configuration** : un fichier TOML dans le dossier de config utilisateur (`~/.config/gigit/config.toml`, via la crate `directories`). Il contient la liste des dépôts, leur ordre, le dernier dépôt ouvert et la taille de la fenêtre.
- **Version minimale de git** : celle installée (2.43). Ne pas dépendre de fonctionnalités plus récentes.

## 8. Façon de travailler (décidée par l'utilisateur)

- **Git flow** dans le dépôt GiGit, avec git-flow AVH 1.12.3 déjà initialisé. Branches : `main` (production) et `develop` (intégration). Préfixes : `feature/`, `release/`, `hotfix/`. Préfixe des tags de version : `v`.
- Chaque bloc fonctionnel est développé sur une branche `feature/<nom>` partant de `develop`, puis fusionné dans `develop`.
- **Releases** : `git flow release start X.Y.Z`, mise à jour de la version dans `Cargo.toml`, puis `git flow release finish` qui crée le tag `vX.Y.Z` sur `main`. Un **paquet `.deb`** est produit pour chaque release.
- **Paquet `.deb`** : construit avec `cargo-deb`. Il contient le binaire `gigit` dans `/usr/bin`, un fichier `.desktop` (pour le menu des applications) et une icône. Les dépendances déclarées incluent `git`. Le `.deb` produit est rangé dans `target/debian/`.

## 8bis. Précisions issues de la relecture (choix d’implémentation, sauf mention contraire)

- **Staging = le vrai index git.** Contrairement à ungit, qui ne stage qu'au moment du commit, GiGit coche ou décoche en faisant `git add` / `git restore --staged` (ou `git apply --cached` pour un hunk ou une ligne) immédiatement. Ce que l'utilisateur a stagé dans un terminal apparaît donc tel quel, et l'état survit au rafraîchissement et au changement de dépôt. Le panneau de droite montre deux groupes : « Indexé » et « Non indexé ».
- **Staging partiel par hunk et par ligne** (parité ungit). Désindexer un hunk ou une ligne est inclus. Annuler un hunk (discard) est inclus aussi, avec confirmation. Cas limites à couvrir : fin de fichier sans saut de ligne, CRLF, fichiers binaires (fichier entier uniquement).
- **Fetch en arrière-plan** uniquement pour le dépôt ouvert : à l'ouverture, puis toutes les 5 minutes. Ces fetchs sont lancés avec `GIT_TERMINAL_PROMPT=0` et l'askpass désactivé : en cas d'échec d'authentification, une simple icône discrète s'affiche, jamais de boîte de dialogue. Les autres dépôts de la liste ne sont **pas** surveillés. Leurs badges sont calculés à partir des données locales, au lancement et au passage sur le dépôt.
- **Instance unique** : `gigit [chemin]` envoie le chemin à l'instance déjà ouverte par un socket Unix dans `$XDG_RUNTIME_DIR`, puis quitte. Une seule fenêtre, un seul écrivain de `config.toml`.
- **Pas d'auto-stash silencieux.** Si un checkout, un reset ou un rebase est refusé à cause de modifications locales, une boîte de dialogue propose « Mettre de côté (stash), faire l'action, puis réappliquer », ou Annuler. En cas de conflit au moment de réappliquer, le stash est conservé et un message le signale.
- **Secrets** : GiGit sert lui-même de `GIT_ASKPASS` et `SSH_ASKPASS` (avec `SSH_ASKPASS_REQUIRE=force`). Il relaie à l'interface les demandes d'identifiant, de mot de passe et de passphrase SSH, ainsi que la confirmation d'une clé d'hôte inconnue (`yes/no`). Les identifiants HTTPS saisis ne sont pas mémorisés par GiGit : le credential helper configuré par l'utilisateur les enregistre s'il en a un. La signature GPG repose sur le pinentry graphique de `gpg-agent`.
- **Langue de l'interface (décidé par l'utilisateur)** : **français et anglais**, au choix dans les réglages. Par défaut, la langue du système (`LANG`), avec l'anglais en repli. Tous les textes passent par un système de traduction dès le départ, sans aucune chaîne en dur dans l'interface. Les messages bruts de git restent tels que git les renvoie.

## 9. Laissé au builder

- Le découpage en modules et crates, et la structure du code.
- L'algorithme de placement des couloirs du graphe (il doit rester stable et lisible avec beaucoup de branches).
- Le détail visuel au-delà de la §6 : icônes, animations discrètes, tailles.
- La stratégie de tests. Il faut au minimum des tests sur l'analyse des sorties git et des tests d'intégration sur de vrais dépôts temporaires pour les opérations.
- L'ordre des livraisons. Je suggère une première release utilisable (dépôts, graphe, staging, commit, fetch, pull, push), puis les releases suivantes jusqu'à la parité complète.
- L'icône de l'application.
