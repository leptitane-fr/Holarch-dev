# Journal de passation

À lire en premier par la session suivante. Le plus récent en haut.

## Pour reprendre (état au 14/10/2026, fin de matinée)

Branche : `claude/navigateur-web-sydt7q` (le dépôt n'a pas encore de
`main` : question posée à Stef, carte du 14/10, « créer une branche main
vide ? » ; la PR brouillon attend sa réponse).

### Choix de pile (annoncé à Stef)
Rust 2024, `no_std`, écrit de zéro, **entiers seulement** (millièmes de
pixel : l'interpréteur WebAssembly du système refuse les flottants), livré
en holon WebAssembly (`wasm32-unknown-unknown`, imports `hln.log`,
`hln.present`, `hln.input`, plus `hln.fetch` proposé). Polices Manrope et
JetBrains Mono pré-tramées en 4 bits/pixel (`docs/TIERS.md` : OFL).
Aucune caisse externe hors `xtask`.

### Fait
- `crates/type` (`hln-type`) : faces pré-tramées (`cargo xtask faces`).
- `crates/web` (`hln-web`), le moteur : adresses, HTML (balises implicites,
  tableaux, entités, jeux de caractères), CSS (cascade, @media, @import,
  sélecteurs, var, calc), mise en page (blocs, lignes, flottants,
  tableaux, flex, positions absolue et fixe), dessin, formulaires.
- `crates/browser` (`hln-browser`), le navigateur : onglets avec leur
  historique, barre d'adresse (adresse, nom de site ou recherche ;
  suggestions tirées des favoris et de l'historique), favoris (étoile),
  recherche dans la page, menu ≡, code source, pages à lui en français
  (`about:accueil`, `favoris`, `historique`, `aide`, `a-propos`, `essai`,
  pages d'erreur ; `NoRight` dit honnêtement que le réseau manque). Le
  chargement se fait pas à pas (document, puis feuilles et `@import`,
  ancre, autofocus, meta refresh), à travers le trait `Net`.
- `holons/navigateur` (`hln-navigateur`) : le holon, tas par classes de
  puissances de deux (`heap.rs`), boucle d'événements ; option `network`
  qui importe `hln.fetch` au format de `hln_user::fetch::Answer`
  d'alpha-2 (longueur, statut, raison).
- `xtask` : `page` (le moteur seul en PNG), `bench` (tout le navigateur
  piloté, une image par étape), `holon` (construit les deux modules et
  vérifie imports et absence d'instructions flottantes), `test`.
- **Essayé dans l'interpréteur d'alpha-2** (`crates/wasm` copié dans le
  bac à sable, sans rien changer à alpha-2) : le holon démarre, affiche
  l'accueil, ouvre pypi.org (copie locale servie par un faux
  `hln.fetch`) avec ses feuilles, défile, ouvre le menu. Environ 1,6 s
  pour charger et mettre en page pypi.org dans l'interpréteur (sur
  l'hôte), mémoire montée à 297 pages (19 Mio, dont 8 Mio de tampon de
  réception).
- CI : `.github/workflows/check.yml` (fmt, clippy hôte et wasm, tests,
  `cargo xtask test`, images en artefact).

### Reste à faire
1. La PR brouillon dès que `main` existe (accord de Stef).
2. Livraison au système, selon `docs/delivering-a-holon.md` d'alpha-2 :
   crate `holons/navigateur` dans alpha-2, ligne dans `WASM_HOLONS`
   (`navigateur`, `hln-navigateur`, version, résumé en français, droits
   `network`) ; c'est la session qui tient alpha-2 qui le fait.
3. Côté navigateur : images (PNG, JPEG, puis GIF), grille CSS, fusion des
   marges parent-enfant, bordures en pointillés, `<details>`, presse-papiers
   quand le système le donnera, POST quand `hln.fetch` le prendra, type de
   contenu et adresse finale renvoyés par `hln.fetch`.

### À demander côté système (alpha-2)
- Droit `network` + import `hln.fetch(url, url_len, body, capacity) -> i64`
  (mot `Answer`), servi par le holon réseau ; idéalement aussi le type de
  contenu, l'adresse finale après redirections, et POST.
- Modules ≥ 2 Mio (le holon fait ~1,6 Mio, surtout les polices) et mémoire
  ≥ 32 Mio (la mémoire initiale déclarée fait déjà ~35 pages : données +
  pile de 1 Mio).
- Fenêtre plus grande que 480 × 320 (le navigateur suit `Event::Resize`).
- La molette (`Event::Wheel`) et Ctrl (`Event::Ctrl` : T, W, L, F, R, D…).
- Les touches Page précédente / Page suivante.

### Environnement
Le proxy du bac à sable refuse la plupart des sites (403) ; pypi.org,
fonts.googleapis.com, fonts.gstatic.com et index.crates.io passent.
