//! The browser's own pages (`about:`), and the pages it shows when one
//! cannot be opened. All in French, none naming the product.

use crate::net::NetError;
use alloc::format;
use alloc::string::{String, ToString};
use hln_web::url::Url;

/// What every page of the browser shares: dark, like the browser around it.
const STYLE: &str = "<style>
body { margin: 0; background: #1d2027; color: #e4e6ec; font-family: Manrope, sans-serif; font-size: 15px; line-height: 1.5 }
main { max-width: 720px; margin: 0 auto; padding: 40px 28px }
h1 { font-size: 28px; margin: 0 0 6px 0; color: #ffffff }
h2 { font-size: 18px; margin: 28px 0 8px 0; color: #ffffff }
p { margin: 8px 0 }
a { color: #8fb4ff; text-decoration: none }
.doux { color: #9aa1b0 }
.carte { background: #272b34; border: 1px solid #343946; border-radius: 12px; padding: 14px 18px; margin: 12px 0 }
ul.liste { list-style: none; padding: 0; margin: 0 }
ul.liste li { padding: 8px 0; border-bottom: 1px solid #2f3440 }
.adresse { color: #9aa1b0; font-size: 13px }
.tuiles { display: flex; flex-wrap: wrap; gap: 12px; margin-top: 16px }
.tuile { flex: 0 0 150px; background: #272b34; border: 1px solid #343946; border-radius: 12px; padding: 12px 14px }
.tuile b { display: block; color: #ffffff }
form { display: flex; gap: 8px; margin: 24px 0 }
input { flex: none; width: 520px; font-size: 16px; padding: 10px 14px; border-radius: 22px; border: 1px solid #3d4352; background: #14161b; color: #e4e6ec }
button { font-size: 15px; padding: 10px 18px; border-radius: 22px; border: 0; background: #3d6fe0; color: #ffffff }
kbd { font-family: monospace; background: #2f3440; border-radius: 4px; padding: 1px 6px; font-size: 13px }
table { border-collapse: collapse }
td { padding: 4px 16px 4px 0; vertical-align: top }
</style>";

/// Escapes text for HTML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=fr><head><meta charset=utf-8><title>{}</title>{STYLE}</head><body><main>{body}</main></body></html>",
        escape(title)
    )
}

/// A saved address: a bookmark or a line of history.
#[derive(Clone)]
pub struct Saved {
    pub title: String,
    pub url: Url,
}

impl Saved {
    /// What a list shows of it: its title, else its address.
    pub fn label(&self) -> String {
        if self.title.trim().is_empty() { self.url.pretty() } else { String::from(self.title.trim()) }
    }
}

fn links(items: &[Saved], empty: &str) -> String {
    if items.is_empty() {
        return format!("<p class=doux>{empty}</p>");
    }
    let mut out = String::from("<ul class=liste>");
    for s in items {
        let href = escape(&s.url.to_string());
        out += &format!(
            "<li><a href=\"{href}\">{}</a><br><span class=adresse>{}</span></li>",
            escape(&s.label()),
            escape(&s.url.pretty())
        );
    }
    out + "</ul>"
}

/// The browser's own page `about:<name>`, if it has one.
pub fn about(name: &str, search: &str, bookmarks: &[Saved], history: &[Saved]) -> Option<String> {
    Some(match name {
        "accueil" | "blank" | "" => {
            let mut tiles = String::new();
            for s in bookmarks.iter().take(8) {
                tiles += &format!(
                    "<a class=tuile href=\"{}\"><b>{}</b><span class=adresse>{}</span></a>",
                    escape(&s.url.to_string()),
                    escape(&s.label()),
                    escape(&s.url.authority())
                );
            }
            if tiles.is_empty() {
                tiles = String::from(
                    "<a class=tuile href=\"about:essai\"><b>Page d'essai</b><span class=adresse>ce que le moteur sait dessiner</span></a>\
                     <a class=tuile href=\"about:aide\"><b>Aide</b><span class=adresse>les touches et les boutons</span></a>",
                );
            }
            let action = escape(search.split('?').next().unwrap_or(search));
            let field = search.split('?').nth(1).and_then(|q| q.split('=').next()).unwrap_or("q");
            document(
                "Nouvel onglet",
                &format!(
                    "<h1>Où aller ?</h1><p class=doux>Tapez une adresse dans la barre du haut, ou cherchez ici.</p>\
                     <form action=\"{action}\"><input name=\"{}\" placeholder=\"Rechercher sur le Web\" autofocus><button>Chercher</button></form>\
                     <h2>Favoris</h2><div class=tuiles>{tiles}</div>",
                    escape(field)
                ),
            )
        }
        "favoris" => document(
            "Favoris",
            &format!(
                "<h1>Favoris</h1><p class=doux>L'étoile de la barre d'adresse ajoute la page ouverte, ou l'enlève.</p>{}",
                links(bookmarks, "Aucun favori pour l'instant.")
            ),
        ),
        "historique" => document(
            "Historique",
            &format!(
                "<h1>Historique</h1><p class=doux>Les pages ouvertes depuis le démarrage, la plus récente en premier. Rien n'est gardé à l'arrêt.</p>{}",
                links(history, "Aucune page ouverte pour l'instant.")
            ),
        ),
        "aide" => document(
            "Aide",
            "<h1>Aide</h1>\
             <h2>Les boutons</h2><table>\
             <tr><td>← →</td><td>page précédente, page suivante</td></tr>\
             <tr><td>⟳</td><td>recharger la page (pendant un chargement : l'arrêter)</td></tr>\
             <tr><td>⌂</td><td>la page d'accueil</td></tr>\
             <tr><td>☆</td><td>ajouter la page aux favoris, ou l'en retirer</td></tr>\
             <tr><td>≡</td><td>le menu : onglets, favoris, historique, recherche dans la page</td></tr></table>\
             <h2>Les touches</h2><table>\
             <tr><td><kbd>Ctrl</kbd> <kbd>L</kbd></td><td>aller à la barre d'adresse</td></tr>\
             <tr><td><kbd>Ctrl</kbd> <kbd>T</kbd> / <kbd>W</kbd></td><td>ouvrir / fermer un onglet</td></tr>\
             <tr><td><kbd>Ctrl</kbd> <kbd>Tab</kbd></td><td>l'onglet suivant</td></tr>\
             <tr><td><kbd>Ctrl</kbd> <kbd>F</kbd></td><td>rechercher dans la page (<kbd>Entrée</kbd> : l'endroit suivant)</td></tr>\
             <tr><td><kbd>Ctrl</kbd> <kbd>D</kbd></td><td>ajouter aux favoris</td></tr>\
             <tr><td><kbd>Ctrl</kbd> <kbd>R</kbd></td><td>recharger</td></tr>\
             <tr><td><kbd>Alt</kbd> <kbd>←</kbd> / <kbd>→</kbd></td><td>page précédente / suivante</td></tr>\
             <tr><td><kbd>Tab</kbd></td><td>le lien ou le champ suivant ; <kbd>Entrée</kbd> l'ouvre</td></tr>\
             <tr><td><kbd>↑</kbd> <kbd>↓</kbd> <kbd>Espace</kbd></td><td>faire défiler la page</td></tr>\
             <tr><td><kbd>Échap</kbd></td><td>quitter un champ, la recherche ou le menu</td></tr></table>",
        ),
        "a-propos" => document(
            "À propos",
            "<h1>À propos du navigateur</h1>\
             <p>Un navigateur écrit de zéro : il lit le HTML et le CSS, place les blocs, les lignes, les tableaux et les boîtes flexibles, puis dessine la page lui-même, sans aucun calcul en virgule flottante.</p>\
             <div class=carte><b>Ce qu'il ne fait pas encore</b><p class=doux>Les images (leur place seulement), la grille CSS, le JavaScript. Les sites qui en dépendent s'affichent incomplets.</p></div>\
             <div class=carte><b>Ce qu'il ne fera jamais</b><p class=doux>Envoyer quoi que ce soit sans que vous l'ayez demandé. Chaque site restera enfermé dans son propre holon.</p></div>\
             <p class=doux>Polices : Manrope et JetBrains Mono, sous licence SIL Open Font License 1.1.</p>",
        ),
        "essai" => String::from(include_str!("../../../docs/essais/page.html")),
        _ => return None,
    })
}

/// The page shown when `url` could not be opened.
pub fn error(url: &Url, error: &NetError) -> String {
    let address = escape(&url.pretty());
    let again = escape(&url.to_string());
    match error {
        NetError::NoRight => document(
            "Pas de réseau",
            &format!(
                "<h1>Pas encore de réseau</h1>\
                 <p>Le navigateur n'a pas pu ouvrir <b>{address}</b> : le système ne lui a pas donné le droit d'aller sur le réseau.</p>\
                 <div class=carte><p class=doux>Chaque holon ne reçoit que les droits qu'on lui accorde. Celui-ci n'a pas encore le droit « réseau » : il ne peut montrer que ses propres pages. Rien n'a été envoyé.</p></div>\
                 <p><a href=\"about:accueil\">Accueil</a> · <a href=\"about:essai\">Page d'essai</a> · <a href=\"about:aide\">Aide</a></p>"
            ),
        ),
        NetError::Unsupported => document(
            "Adresse non prise en charge",
            &format!(
                "<h1>Adresse non prise en charge</h1>\
                 <p>Le navigateur n'ouvre que les pages du Web (<b>http</b> et <b>https</b>) et les siennes (<b>about:</b>). <b>{address}</b> n'en est pas une.</p>\
                 <p><a href=\"about:accueil\">Accueil</a></p>"
            ),
        ),
        NetError::Failed(why) => document(
            "Page introuvable",
            &format!(
                "<h1>Impossible d'ouvrir la page</h1>\
                 <p><b>{address}</b> n'a pas répondu.</p>\
                 <div class=carte><p class=doux>{}</p></div>\
                 <p><a href=\"{again}\">Réessayer</a> · <a href=\"about:accueil\">Accueil</a></p>",
                escape(why)
            ),
        ),
    }
}

/// The page shown for an `about:` address the browser does not have.
pub fn unknown(name: &str) -> String {
    document(
        "Page inconnue",
        &format!(
            "<h1>Page inconnue</h1><p>Le navigateur n'a pas de page <b>about:{}</b>. Les siennes : \
             <a href=\"about:accueil\">accueil</a>, <a href=\"about:favoris\">favoris</a>, <a href=\"about:historique\">historique</a>, \
             <a href=\"about:aide\">aide</a>, <a href=\"about:a-propos\">a-propos</a>, <a href=\"about:essai\">essai</a>.</p>",
            escape(name)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_exist_and_escape() {
        let s = Saved { title: String::from("<b>x</b>"), url: Url::parse("https://exemple.fr/").unwrap() };
        let page = about("favoris", "https://html.duckduckgo.com/html/?q=", &[s], &[]).unwrap();
        assert!(page.contains("&lt;b&gt;x&lt;/b&gt;") && !page.contains("<b>x</b>"));
        for name in ["accueil", "historique", "aide", "a-propos", "essai"] {
            assert!(about(name, "https://e.fr/?q=", &[], &[]).is_some(), "{name}");
        }
        assert!(about("rien", "", &[], &[]).is_none());
        let accueil = about("accueil", "https://html.duckduckgo.com/html/?q=", &[], &[]).unwrap();
        assert!(accueil.contains("action=\"https://html.duckduckgo.com/html/\"") && accueil.contains("name=\"q\""));
    }
}
