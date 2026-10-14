# Journal de passation

À lire en premier par la session suivante. Le plus récent en haut.

## 2026-10-14 (fin de matinée) — Le holon, la fusion de deux sessions, la PR

Deux sessions ont travaillé en même temps sur la branche
`claude/navigateur-web-sydt7q` : l'une a poussé l'enveloppe du navigateur
(entrée suivante, « L'enveloppe du navigateur et son banc »), l'autre le
holon. Fusionnées : **l'enveloppe gardée est celle déjà poussée**
(`crates/browser`, `xtask bench`) ; de l'autre session restent le holon, la
vérification du module, la CI et les documents. Avant de travailler ici,
vérifier qu'aucune autre session n'écrit sur la branche (`git fetch`).

Stef a accepté (carte du 14/10, « Main vide ») une branche `main` réduite
à un README, base de la PR brouillon du navigateur.

### Fait
- `holons/navigateur` (`hln-navigateur`) : holon `cdylib` `no_std`, tas
  par classes de puissances de deux (`heap.rs`, jamais émietté), boucle
  d'événements (les mots de `hln.input` traduits en `Event`), dessin
  seulement quand quelque chose a changé, un pas de chargement quand rien
  n'attend. Option `network` : `Net` sur l'import proposé
  `hln.fetch(url, url_len, body, capacity) -> i64`, au format du mot
  `hln_user::fetch::Answer` d'alpha-2 (longueur, statut, raison ; raisons
  dites en français). Sans l'option, `Offline` : seules les pages
  `about:` s'ouvrent, l'erreur dit pourquoi.
- `cargo xtask holon` : construit `build/holon/navigateur.wasm` et
  `navigateur-reseau.wasm`, vérifie les imports permis et l'absence de
  toute instruction flottante (lecture du module, testée). ~1,6 Mio.
  `cargo xtask test` : `holon` puis `bench --offline`.
- **Essayé dans l'interpréteur d'alpha-2** (`crates/wasm` copié dans le
  bac à sable, rien changé dans alpha-2) : le holon démarre, affiche
  l'accueil, ouvre pypi.org (copie locale servie par un faux `hln.fetch`)
  avec ses quatre feuilles, défile, ouvre le menu. ~2 s pour pypi.org
  dans l'interpréteur sur l'hôte ; mémoire jusqu'à ~300 pages (19 Mio,
  dont 8 Mio de tampon de réception).
- Moteur : un élément remplacé (champ, bouton) d'une ligne flex garde la
  largeur que la ligne lui donne (le champ de recherche de l'accueil
  s'étire enfin).
- `.github/workflows/check.yml`, `CLAUDE.md`, `README.md`.

### Reste à faire
1. Livraison au système, selon `docs/delivering-a-holon.md` d'alpha-2
   (crate dans `holons/`, ligne dans `WASM_HOLONS`, droit `network`) : à
   faire par la session qui tient alpha-2, quand ses limites le
   permettront.
2. Navigateur : images (PNG, JPEG, GIF), grille CSS, fusion des marges
   parent-enfant, sélection et presse-papiers, favoris gardés (quand le
   holon aura un stockage), clic droit (le holon ne transmet que le
   bouton gauche à l'enveloppe actuelle).

### À demander côté système (alpha-2, en lecture seule pour cette session)
- Droit `network` + `hln.fetch` ci-dessus, servi par le holon réseau ;
  idéalement aussi le type de contenu, l'adresse finale et POST.
- Modules ≥ 2 Mio et mémoire ≥ 32 Mio (la mémoire initiale déclarée fait
  déjà ~35 pages) ; une fenêtre plus grande que 480 × 320 ; la molette ;
  Ctrl, Alt, Maj ; Page précédente et Page suivante.

## 2026-10-14 (matin) — L'enveloppe du navigateur et son banc

Reprise de la session interrompue, même branche `claude/navigateur-web-sydt7q`.
Toujours pas de `main` ni de PR : **attendre l'accord de Stef** avant de
pousser sur `main`.

### Fait (tout compile, y compris `hln-browser` pour `wasm32-unknown-unknown` ; 44 tests ; clippy `-D warnings` propre)
- `crates/browser` (`hln-browser`) remis dans l'espace de travail :
  - `lib.rs` : `Browser` (onglets, historique par onglet avec la position
    de défilement, historique de la session, favoris), `Event`/`Key`/`Mods`
    (indépendants d'alpha-2 : le holon traduira les mots de `hln.input`),
    focus Page/Adresse/Champ/Recherche, chargement par étapes
    (`step(&mut dyn Net)` : le document, puis les feuilles, puis leurs
    `@import`, une requête à la fois ; la page affichée reste jusqu'à ce que
    la nouvelle soit complète), ancres sans rechargement, `autofocus`,
    `meta refresh` (5 au plus), formulaires GET/POST, cases, boutons radio,
    listes (un clic : le choix suivant), `view-source:`, Tab entre liens et
    champs, raccourcis Ctrl L/T/W/F/D/R/H/Tab et Alt ←/→.
  - `chrome.rs` : thème sombre ; onglets 34 px (titre, ×, +, anneau de
    chargement), barre 40 px (← → ⟳/× ⌂ dessinés au trait, adresse avec
    cadenas pour les pages venues en https, ☆ plein si favori, menu ≡),
    barre de progression, suggestions sous l'adresse (favoris puis
    historique, ↑ ↓), menu, barre de recherche (« 2 sur 3 », ↑ ↓ ×), barre
    de défilement (piste et pouce à tirer), bulle « Chargement de … » ou
    adresse du lien survolé, anneau autour du lien ou du contrôle du
    clavier. `regions()` sert au dessin et au pointeur.
  - `pages.rs` : about:accueil (recherche DuckDuckGo HTML, tuiles de
    favoris), favoris, historique, aide, a-propos, essai
    (`docs/essais/page.html`), page inconnue, pages d'erreur en français
    (`NoRight` expliqué : le holon n'a pas le droit « réseau », rien n'a
    été envoyé ; `Unsupported` ; `Failed`).
- `cargo xtask bench [adresse|fichier] [sortie.png] [--size WxH] [--offline]
  [--type T] [--key ctrl+f] [--click X,Y] [--hover X,Y] [--wheel N]
  [--open A]` : le navigateur entier rendu en PNG (curl et fichiers ; les
  formulaires POST passent par `curl --data`). Vérifié sur about:accueil,
  about:essai (menu ouvert, recherche, anneau, bulle), pypi.org, et la
  page « Pas encore de réseau ».
- Avertissements clippy du moteur corrigés (aucun changement de rendu).

### Nouveau côté alpha-2 (à lire avant l'étape holon)
alpha-2 a maintenant le réseau (PR #63 à #66) : dans Windows, le lanceur
prête les connexions du PC au niveau des sockets ; seul le holon réseau
les reçoit (`Object::Network`, appels système 16 et 17) ; `crates/net`
(`hln-net`) fait TLS (rustls, racines Mozilla) et HTTP/1.1. Les holons
WebAssembly n'ont toujours que `hln.log`, `hln.present`, `hln.input`, une
mémoire de 1 Mio et un module de 64 Kio, une fenêtre de 480×320. Le
`hln.fetch` du navigateur devra donc passer par le holon réseau.

### Reste à faire (dans l'ordre)
1. Holon `holons/navigateur` : cdylib wasm, allocateur, boucle
   d'événements (traduire les mots de `hln.input` en `Event`), `Net` sur
   `hln.fetch` derrière une option `network` (sinon `Offline`) ; vérifier
   la taille et l'absence d'instructions flottantes.
2. `CLAUDE.md`, `README.md`, `.github/workflows/check.yml` ; PR brouillon
   (une fois `main` créée avec l'accord de Stef).
3. Demandes côté alpha-2 (inchangées) : droit `network` + import
   `hln.fetch` relayé par le holon réseau ; modules et mémoire bien plus
   grands (les faces pèsent ~1,1 Mo) ; fenêtre plus grande que 480×320 ;
   molette et modificateurs (Ctrl, Alt, Maj).

### Lacunes connues
Moteur : pas d'images, grid rendu en blocs, pas de fusion des marges
parent-enfant, pas de JavaScript. Navigateur : favoris et historique non
gardés à l'arrêt (le holon n'a pas de stockage), pas de sélection de
texte ni de presse-papiers, une liste `<select>` ne s'ouvre pas (un clic
passe au choix suivant).

## 2026-10-14 — Navigateur Web, première étape (interrompue : quota atteint)

