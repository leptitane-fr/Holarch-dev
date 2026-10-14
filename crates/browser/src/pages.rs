//! The browser's own pages (`about:`), written in HTML and drawn by the
//! engine like any other: the home page, bookmarks, history, help, what
//! the browser can do, and the pages that say why a page did not come.
//! Everything here is in French: the person reads it.

use crate::net::NetError;
use alloc::format;
use alloc::string::{String, ToString};
use hln_web::url::Url;

/// A page remembered: its title and its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub title: String,
    pub url: Url,
}

/// What the browser's pages show of the browser.
pub struct Book<'a> {
    pub bookmarks: &'a [Mark],
    /// Pages visited, the oldest first.
    pub visited: &'a [Mark],
    /// Where searches go (the words are appended).
    pub search: &'a str,
}

/// Escapes text for HTML.
pub fn esc(text: &str) -> String {
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

const STYLE: &str = "
body { margin: 0; background: #12141a; color: #e3e6ec; font-family: Manrope, sans-serif; font-size: 15px; line-height: 1.5 }
main { max-width: 720px; margin: 0 auto; padding: 32px 24px 48px }
h1 { font-size: 28px; margin: 0 0 6px; color: #f4f6fa }
h2 { font-size: 18px; margin: 28px 0 10px; color: #c9d2e3 }
p.sous { color: #8d96a8; margin: 0 0 24px }
a { color: #8fb4ff; text-decoration: none }
code, kbd { font-family: 'JetBrains Mono', monospace; font-size: 13px; background: #222632; border-radius: 4px; padding: 1px 5px }
.carte { background: #1b1e27; border: 1px solid #2a2f3c; border-radius: 12px; padding: 16px 20px; margin: 12px 0 }
.tuiles { display: flex; flex-wrap: wrap; gap: 10px }
.tuile { flex: 0 0 160px; background: #1b1e27; border: 1px solid #2a2f3c; border-radius: 10px; padding: 10px 12px; overflow: hidden }
.tuile b { display: block; color: #e3e6ec; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.tuile span { display: block; color: #7d8699; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
form.cherche { display: flex; gap: 8px; margin: 8px 0 }
form.cherche input { flex: 1 1 auto; font-size: 16px; padding: 9px 14px; border-radius: 22px; border: 1px solid #3a4152; background: #1b1e27; color: #e3e6ec }
form.cherche button { font-size: 15px; padding: 9px 18px; border-radius: 22px; border: 0; background: #7aa2f7; color: #0d1220; font-weight: bold }
ul.liste { list-style: none; padding: 0; margin: 0 }
ul.liste li { padding: 6px 0; border-bottom: 1px solid #232733 }
ul.liste li span { color: #7d8699; font-size: 12px; margin-left: 8px }
table { border-collapse: collapse; width: 100% }
td { padding: 5px 8px; border-bottom: 1px solid #232733; vertical-align: top }
td:first-child { white-space: nowrap; width: 38% }
.vide { color: #7d8699; font-style: italic }
.erreur h1 { color: #ffb4a8 }
";

fn shell(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=fr><head><meta charset=utf-8><title>{}</title><style>{STYLE}</style></head>\
         <body><main>{body}</main></body></html>",
        esc(title)
    )
}

/// The search box, sending its words to `search`.
fn search_form(search: &str) -> String {
    let (action, name) = match search.split_once('?') {
        Some((a, q)) => (a, q.trim_end_matches('=')),
        None => (search, "q"),
    };
    format!(
        "<form class=cherche action='{}'><input name='{}' autofocus placeholder='Rechercher sur le Web'>\
         <button>Rechercher</button></form>",
        esc(action),
        esc(name)
    )
}

fn tiles(marks: &[Mark]) -> String {
    let mut out = String::from("<div class=tuiles>");
    for m in marks {
        let title = if m.title.is_empty() { m.url.pretty() } else { m.title.clone() };
        out.push_str(&format!(
            "<a class=tuile href='{}'><b>{}</b><span>{}</span></a>",
            esc(&m.url.to_string()),
            esc(&title),
            esc(&m.url.pretty())
        ));
    }
    out.push_str("</div>");
    out
}

fn list(marks: &[Mark], newest_first: bool) -> String {
    if marks.is_empty() {
        return String::from("<p class=vide>Rien pour l'instant.</p>");
    }
    let mut out = String::from("<ul class=liste>");
    let mut push = |m: &Mark| {
        let title = if m.title.is_empty() { m.url.pretty() } else { m.title.clone() };
        out.push_str(&format!(
            "<li><a href='{}'>{}</a><span>{}</span></li>",
            esc(&m.url.to_string()),
            esc(&title),
            esc(&m.url.pretty())
        ));
    };
    if newest_first {
        marks.iter().rev().for_each(&mut push);
    } else {
        marks.iter().for_each(&mut push);
    }
    out.push_str("</ul>");
    out
}

// Le P'tit Âne trotte jusqu'aux pages du navigateur sans réseau.

/// The page `about:<name>`, if the browser has one by that name.
pub fn about(name: &str, book: &Book) -> Option<String> {
    let name = name.trim_end_matches('/');
    Some(match name {
        "" | "accueil" | "blank" | "home" | "newtab" => {
            let mut recent: alloc::vec::Vec<Mark> = alloc::vec::Vec::new();
            for m in book.visited.iter().rev() {
                if !recent.iter().any(|r| r.url == m.url) {
                    recent.push(m.clone());
                }
                if recent.len() == 6 {
                    break;
                }
            }
            let mut body =
                format!("<h1>Nouvel onglet</h1><p class=sous>Où allons-nous ?</p>{}", search_form(book.search));
            body.push_str("<h2>Favoris</h2>");
            if book.bookmarks.is_empty() {
                body.push_str("<p class=vide>Aucun favori : l'étoile de la barre d'adresse en ajoute un.</p>");
            } else {
                body.push_str(&tiles(book.bookmarks));
            }
            if !recent.is_empty() {
                body.push_str("<h2>Récemment</h2>");
                body.push_str(&tiles(&recent));
            }
            body.push_str(
                "<p class=sous style='margin-top:28px'><a href='about:aide'>Aide et raccourcis</a> · \
                 <a href='about:a-propos'>Ce que sait faire ce navigateur</a> · <a href='about:essai'>Page d'essai</a></p>",
            );
            shell("Nouvel onglet", &body)
        }
        "favoris" => shell("Favoris", &format!("<h1>Favoris</h1>{}", list(book.bookmarks, false))),
        "historique" => shell(
            "Historique",
            &format!(
                "<h1>Historique</h1><p class=sous>Les pages de cette session, la plus récente en haut. \
                 Rien n'est gardé après la fermeture.</p>{}",
                list(book.visited, true)
            ),
        ),
        "aide" => shell("Aide", HELP),
        "a-propos" => shell("À propos du navigateur", ABOUT),
        "essai" => String::from(include_str!("../../../docs/essais/page.html")),
        _ => return None,
    })
}

const HELP: &str = "<h1>Aide</h1><p class=sous>Le clavier et la souris dans le navigateur.</p>
<div class=carte><table>
<tr><td><kbd>Échap</kbd></td><td>aller à la barre d'adresse (ou en sortir)</td></tr>
<tr><td><kbd>Entrée</kbd> dans la barre</td><td>ouvrir l'adresse, ou chercher les mots tapés</td></tr>
<tr><td><kbd>↑</kbd> <kbd>↓</kbd> dans la barre</td><td>choisir une suggestion (favoris, historique)</td></tr>
<tr><td><kbd>↑</kbd> <kbd>↓</kbd> <kbd>Espace</kbd></td><td>faire défiler la page</td></tr>
<tr><td><kbd>Début</kbd> <kbd>Fin</kbd></td><td>haut et bas de la page</td></tr>
<tr><td><kbd>Tab</kbd></td><td>passer d'un lien ou d'un champ au suivant ; <kbd>Entrée</kbd> l'ouvre</td></tr>
<tr><td><kbd>Retour arrière</kbd></td><td>page précédente</td></tr>
<tr><td><kbd>/</kbd></td><td>rechercher dans la page ; <kbd>Entrée</kbd> et <kbd>↑</kbd> passent d'un résultat à l'autre</td></tr>
<tr><td><kbd>Ctrl</kbd> + <kbd>T</kbd>, <kbd>W</kbd>, <kbd>L</kbd>, <kbd>F</kbd>, <kbd>R</kbd>, <kbd>D</kbd></td>
<td>nouvel onglet, fermer l'onglet, adresse, rechercher, recharger, ajouter aux favoris (quand le système transmettra la touche Ctrl)</td></tr>
</table></div>
<div class=carte><table>
<tr><td>Clic sur un lien</td><td>l'ouvrir</td></tr>
<tr><td>Clic droit sur un lien</td><td>l'ouvrir dans un nouvel onglet</td></tr>
<tr><td>Molette, barre de droite</td><td>faire défiler</td></tr>
<tr><td>Menu ≡</td><td>onglets, favoris, historique, code source, aide</td></tr>
</table></div>";

const ABOUT: &str = "<h1>À propos du navigateur</h1>
<p class=sous>Un navigateur écrit de zéro, sans bibliothèque, pour tourner dans un holon.</p>
<h2>Ce qu'il sait faire</h2>
<div class=carte><ul>
<li>lire le HTML comme les grands navigateurs (balises implicites, tableaux, entités, jeux de caractères) ;</li>
<li>appliquer le CSS : cascade et spécificité, sélecteurs complets, <code>@media</code>, <code>@import</code>,
<code>var()</code>, <code>calc()</code>, couleurs <code>rgb</code> et <code>hsl</code> ;</li>
<li>mettre en page : blocs, lignes, flottants, tableaux (fusion de cellules), flexbox, positions absolue et fixe ;</li>
<li>dessiner le texte lissé (Manrope, JetBrains Mono), les bordures arrondies, les soulignements ;</li>
<li>onglets, historique, favoris, formulaires (GET et POST), recherche dans la page, code source.</li>
</ul></div>
<h2>Ce qu'il ne sait pas encore faire</h2>
<div class=carte><ul>
<li>le JavaScript : les pages qui en dépendent restent incomplètes ;</li>
<li>les images (une place leur est gardée) et la grille CSS (rendue en blocs) ;</li>
<li>le réseau dépend du système : le holon attend le droit <code>network</code>.</li>
</ul></div>";

/// The page saying why `url` did not come.
pub fn error(url: &Url, err: &NetError) -> String {
    let (title, text) = match err {
        NetError::NoRight => (
            "Pas encore d'accès au réseau",
            String::from(
                "<p>Ce holon n'a pas encore le droit d'aller sur le réseau : le système ne le lui donne pas \
                 aujourd'hui. Les pages du navigateur (<a href='about:accueil'>accueil</a>, \
                 <a href='about:aide'>aide</a>, <a href='about:essai'>page d'essai</a>) marchent sans lui.</p>\
                 <p>Dès que le système accordera le droit <code>network</code>, avec votre accord à \
                 l'installation, les pages du Web s'ouvriront ici.</p>",
            ),
        ),
        NetError::Unsupported => (
            "Adresse non prise en charge",
            format!("<p>Ce navigateur n'ouvre pas les adresses <code>{}:</code>.</p>", esc(&url.scheme)),
        ),
        NetError::Failed(why) => (
            "La page n'a pas pu être chargée",
            format!(
                "<p>{}</p><p>Vérifiez l'adresse, ou réessayez avec <kbd>Ctrl</kbd> + <kbd>R</kbd> ou le bouton ⟳.</p>",
                esc(why)
            ),
        ),
    };
    let body = format!(
        "<div class=erreur><h1>{title}</h1><p class=sous><code>{}</code></p>{text}</div>",
        esc(&url.to_string())
    );
    shell(title, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn pages_escape_what_they_show() {
        let marks = vec![Mark { title: String::from("<b>Âne</b>"), url: Url::parse("https://exemple.fr/").unwrap() }];
        let book = Book { bookmarks: &marks, visited: &marks, search: "https://cherche.fr/?q=" };
        let home = about("accueil", &book).unwrap();
        assert!(home.contains("&lt;b&gt;Âne") && !home.contains("<b>Âne"));
        assert!(home.contains("action='https://cherche.fr/'") && home.contains("name='q'"));
        assert!(about("inconnue", &book).is_none());
        let e = error(&Url::parse("ftp://x.fr/").unwrap(), &NetError::Unsupported);
        assert!(e.contains("<code>ftp:</code>"));
    }
}
