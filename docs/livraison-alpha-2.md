# Livrer le navigateur au système (pour la session qui tient alpha-2)

Le holon : `navigateur-0.1.0.wasm` (fichiers du projet, `navigateur/` ; ou
`cargo xtask holon` → `build/holon/navigateur.wasm`), 1 667 959 octets,
SHA-256 `93301dcfdaa208fe182d73937ad8074b15e4fbf334afa10880fa706905dc9658`.
Il est construit depuis `leptitane-fr/Holarch-dev` (branche
`claude/navigateur-web-sydt7q`, PR #2) par `cargo xtask holon`, avec
l'option `network`, qui est maintenant celle par défaut.

## Ce qu'il est

- Il importe `hln.log`, `hln.present`, `hln.input` et `hln.fetch`.
- Il ne contient aucune instruction flottante (vérifié par `cargo xtask holon`).
- Il suit le contrat de `docs/delivering-a-holon.md` de la PR #72 d'alpha-2 :
  - `RESIZED` : il dessine à la taille donnée ;
  - `WHEEL` : la molette fait défiler la page ;
  - Maj, Ctrl, Alt (bits 36 à 38) : Ctrl+L, Ctrl+T, Ctrl+F, Alt+← et les autres raccourcis ;
  - Page précédente et Page suivante.
- Il ouvre les adresses `http` en `https`, puisque le système n'ouvre que celles-là.
- Il a été essayé dans `crates/wasm` d'alpha-2, copié sur l'hôte, avec un
  faux `hln.fetch` : fenêtre de 960 × 600 puis de 640 × 400, Ctrl+L,
  `http://pypi.org` ouvert en `https` avec ses feuilles, molette, Page
  suivante, Ctrl+F.

Sans la PR #72, il marche aussi : il reste à 480 × 320 et les raccourcis
avec Ctrl ne lui arrivent pas.

## La ligne à ajouter

Dans `WASM_HOLONS` (ou `DEPOT_ONLY` s'il ne doit pas aller sur la clé), avec
le droit `network` :

```
("navigateur", "hln_navigateur", "0.1.0", "Navigateur Web écrit de zéro : onglets, favoris, recherche dans la page", "network"),
```

Le paquet `hln-navigateur` vit dans Holarch-dev. Deux façons de l'avoir :
- copier `crates/type`, `crates/web`, `crates/browser` et `holons/navigateur` dans alpha-2 ;
- ou, pour ce premier essai, sceller directement le `.wasm` ci-dessus.

## Encore attendu côté système

- L'envoi de formulaires (POST) : la recherche de l'accueil passe par GET et marche déjà.
- Le type de contenu et l'adresse finale après redirection.
- Un `fetch` qui ne bloque pas la fenêtre pendant un chargement.
