# Livrer le navigateur au système (pour la session qui tient alpha-2)

Le holon : `navigateur-0.1.0.wasm` (fichiers du projet, `navigateur/` ; ou `cargo xtask holon` → `build/holon/navigateur.wasm`), SHA-256 `06e9bc75f981e572fd3ef4ac5c3dabe26edc88e52e44c808e7d966b01c16234e`.
Il est construit depuis `leptitane-fr/Holarch-dev` (`main`) par `cargo xtask holon`, sans l'option `network`.

## Ce qu'il est

- Il importe seulement `hln.log`, `hln.present` et `hln.input`.
- Il ne contient aucune instruction flottante (vérifié par `cargo xtask holon`).
- Il a été essayé dans `crates/wasm` d'alpha-2, copié sur l'hôte :
  - il démarre, dessine et lit les touches et la souris ;
  - avec un faux `hln.fetch`, il ouvre une page et ses feuilles de style.

## Ce qui l'empêche de s'ouvrir aujourd'hui (`programs/wasm/src/main.rs`)

| Limite | Aujourd'hui | Le navigateur |
|---|---|---|
| `MODULE_MAX` | 64 Kio | **1,6 Mio** (1 647 000 octets environ, dont 1,1 Mio de polices) |
| `MAX_PAGES` | 16 (1 Mio) | mémoire initiale déclarée **34 pages** ; jusqu'à ~300 pages (19 Mio) sur une grosse page |

Proposition :
- `MODULE_MAX` à 4 Mio ;
- `MAX_PAGES` à 512 (32 Mio) ;
- et ce qui en découle : la projection du module (`MODULE_ADDRESS`, le trou jusqu'à `SURFACE_ADDRESS` fait 16 Mio, assez) et la taille des capsules reçues par la Bibliothèque et le coffre.

## La ligne à ajouter

Dans `DEPOT_ONLY`, ou dans `WASM_HOLONS` s'il doit aussi aller sur la clé :

```
("navigateur", "hln_navigateur", "0.1.0", "Navigateur Web écrit de zéro : onglets, favoris, recherche dans la page", ""),
```

Le paquet `hln-navigateur` vit dans Holarch-dev. Deux façons de l'avoir :
- copier `crates/type`, `crates/web`, `crates/browser` et `holons/navigateur` dans alpha-2 ;
- ou, pour ce premier essai, sceller directement le `.wasm` ci-dessus.

## Plus tard

- Droit `network` et import `hln.fetch(url, url_len, body, capacity) -> i64`, au format du mot `hln_user::fetch::Answer` ; le holon est prêt avec l'option `network`.
- Fenêtre plus grande que 480 × 320.
- Molette, Ctrl et Alt, Page précédente et Page suivante.
