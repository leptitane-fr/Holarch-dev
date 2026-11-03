# Le navigateur Web

Un navigateur écrit de zéro en Rust, sans bibliothèque, pour tourner dans
un holon du système : WebAssembly, entiers seulement, une fenêtre et
rien d'autre. Le système est dans le dépôt `Holarch-system-alpha-2`.

## Ce qu'il y a ici

| Endroit | Rôle |
|---|---|
| `crates/type` (`hln-type`) | les polices Manrope et JetBrains Mono, lissées d'avance à chaque taille |
| `crates/web` (`hln-web`) | le moteur : adresses, HTML, CSS, mise en page, dessin, formulaires |
| `crates/browser` (`hln-browser`) | le navigateur : onglets, barre d'adresse, historique, favoris, recherche dans la page, menu, ses propres pages (`about:`) |
| `holons/navigateur` (`hln-navigateur`) | le holon : la boucle d'événements, la mémoire, le réseau du système |
| `xtask` | les outils de développement |

## Les commandes

- `cargo test` : les essais ;
- `cargo xtask holon` : construit le holon (`build/holon/navigateur.wasm`,
  avec l'import `hln.fetch`, et `navigateur-hors-ligne.wasm` sans) et vérifie ce que
  vérifie le système : les imports permis, aucune instruction flottante ;
- `cargo xtask bench [adresse…] [--size LxH]` : fait tourner tout le
  navigateur et enregistre une image après chaque étape (`build/bench/`) ;
- `cargo xtask page <fichier | adresse> [image.png]` : le moteur seul,
  une page en image ;
- `cargo xtask test` : `holon` puis `bench`, comme la vérification
  automatique.

## Ce qu'il attend du système

Le système le reçoit comme une application qui demande le droit `network`
(`rights = network`) : il importe `hln.fetch`, décrit dans
`holons/navigateur/src/lib.rs`. La fenêtre prend toute taille (et le plein
écran), la molette et Ctrl, Alt, Maj arrivent avec les touches. Encore
attendus côté système : l'envoi de formulaires (POST), le type de contenu
et l'adresse finale, et un chargement qui ne bloque pas la fenêtre. Voir
`docs/JOURNAL.md`.
