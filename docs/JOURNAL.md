# Journal de passation

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

Branche : `claude/navigateur-web-sydt7q`. Le dépôt était vide (aucun commit,
pas de `main`) : ce commit est le premier. Il faudra une branche `main`
(commit minimal) pour ouvrir la PR brouillon ; **demander l'accord de Stef
avant de pousser sur `main`**.

### Choix de pile (annoncé)
Rust 2024 (`rust-toolchain.toml` copié d'alpha-2), `no_std`, écrit de zéro,
**entiers seulement** (millièmes de pixel : l'interpréteur WebAssembly du
système refuse les flottants), livré comme holon WebAssembly
(`wasm32-unknown-unknown`, imports `hln.log`, `hln.present`, `hln.input`).
Polices Manrope et JetBrains Mono pré-tramées en 4 bits/pixel
(`docs/TIERS.md` : licences OFL). Aucune caisse externe hors `xtask`.

### Fait (tout compile, 33 tests du moteur passent)
- `crates/type` (`hln-type`) : faces pré-tramées (`cargo xtask faces`,
  polices récupérées sur fonts.gstatic.com), `face(mono, gras, taille)`.
- `crates/web` (`hln-web`), le moteur : `url`, `html` (tokeniseur + arbre,
  entités, jeux de caractères), `css` (cascade, @media, @import, sélecteurs,
  spécificité), `style` (calc, var, couleurs, feuille de l'agent),
  `layout` (blocs, lignes, flottants, tableaux avec rowspan/colspan, flex,
  positionnement absolu/fixe), `paint` (canevas, texte lissé, clips),
  `page` (liens, ancres, formulaires GET/POST, recherche dans la page,
  meta refresh).
- `xtask` : `cargo xtask page <fichier|url> [sortie.png] [--size WxH]
  [--full] [--dump]` rend une page en PNG (curl pour le réseau).
  Rendus vérifiés : `docs/essais/page.html` et pypi.org (copie locale).

### Commencé, hors de l'espace de travail pour l'instant
`crates/browser` (`hln-browser`) : `net.rs` (trait `Net`, `Response`,
`NetError`, `Offline`), `edit.rs` (ligne éditable, testée),
`Cargo.toml`. Ces caisses ne sont **pas** dans `members` du `Cargo.toml`
racine : les y remettre (`crates/browser`, `holons/navigateur`) et rajouter
`hln-browser` aux dépendances de `xtask`.

### Reste à faire (dans l'ordre)
1. `hln-browser` : `lib.rs` (Browser, onglets, historique par onglet,
   focus Page/Adresse/Champ/Recherche, chargement par étapes : document
   puis feuilles et @import, ancre, autofocus, meta refresh), `chrome.rs`
   (thème sombre : onglets 34 px, barre 40 px ← → ⟳ ⌂, adresse avec
   cadenas, ☆, menu ≡ ; barre de progression, bulle d'état, barre de
   défilement, barre de recherche, suggestions), `pages.rs` (about:accueil,
   favoris, historique, aide, a-propos, essai, pages d'erreur en français,
   sans nom de produit ; `NoRight` expliqué honnêtement).
2. `xtask bench` : rendre le navigateur entier en PNG.
3. Holon `holons/navigateur` : cdylib wasm, allocateur, boucle
   d'événements, `hln.fetch` derrière une option `network` ; vérifier la
   taille et l'absence d'instructions flottantes.
4. `CLAUDE.md`, `README.md`, `.github/workflows/check.yml` ; fmt, clippy,
   tests ; PR brouillon.

### À demander côté système (alpha-2)
Droit `network` + import `hln.fetch` ; modules et mémoire bien plus grands
(les faces pèsent ~1,1 Mo, la limite est 64 Kio / 1 Mio) ; fenêtre plus
grande que 480×320 ; molette et modificateurs (Ctrl).

### Lacunes connues du moteur
Pas d'images (emplacements seulement), CSS grid rendu en blocs, pas de
fusion des marges parent-enfant, pas de JavaScript.

### Environnement
Le proxy du bac à sable refuse la plupart des sites (403) ; pypi.org,
fonts.googleapis.com, fonts.gstatic.com et index.crates.io passent.
