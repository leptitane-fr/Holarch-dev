# Journal de passation

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
