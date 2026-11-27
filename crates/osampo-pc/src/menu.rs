//! Menu `com.canonical.dbusmenu` de l'icône (SPEC-004) : statuts non cliquables, séparateur,
//! pendant un rappel notifié `Accepter le rappel`, `Reporter N min` et un séparateur, puis
//! `Chatouiller`, `Donner un os`, `Donner un bain`, `Remplir la jauge`, `Vider la jauge`, `Réglages…`,
//! `Quitter Osampo`.
//! La liste d'entrées est neutre ([`entrees`]) : le menu du chien (SPEC-011) la reprend.

use ksni::menu::StandardItem;
use ksni::MenuItem;

use crate::notification;
use crate::sni::Icone;
use crate::statuts::{self, Vue};

/// Ce qu'un clic de menu (ou l'`Activate` de l'icône) demande à la boucle principale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Chatouille,
    Os,
    Bain,
    Remplir,
    Vider,
    Reglages,
    /// Comme les boutons de la notification du rappel (SPEC-005), si elle a disparu.
    Accepter,
    Reporter,
    /// Arrête le daemon puis `osampo-pc` : plus rien n'alimente le chiot jusqu'à la
    /// prochaine session.
    Quitter,
}

/// Une ligne du menu, indépendante du dessin (ksni pour l'icône, X11 pour le chien).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entree {
    pub texte: String,
    /// `None` pour un statut ou un séparateur.
    pub action: Option<Action>,
    pub actif: bool,
    pub separateur: bool,
}

impl Entree {
    fn statut(texte: String) -> Self {
        Entree { texte, action: None, actif: false, separateur: false }
    }

    fn geste(texte: &str, action: Action, actif: bool) -> Self {
        Entree { texte: texte.into(), action: Some(action), actif, separateur: false }
    }

    fn separateur() -> Self {
        Entree { texte: String::new(), action: None, actif: false, separateur: true }
    }
}

/// Les entrées dans l'ordre : statuts non cliquables, séparateur, `Chatouiller`,
/// `Donner un os`, `Donner un bain`, `Remplir la jauge`, `Vider la jauge`, `Réglages…`, `Quitter Osampo`.
pub fn entrees(vue: &Vue) -> Vec<Entree> {
    let mut m: Vec<Entree> = statuts::lignes(vue).into_iter().map(Entree::statut).collect();
    m.push(Entree::separateur());
    let rappel = vue.as_ref().and_then(|e| e.rappel_en_cours.as_ref());
    if rappel.is_some_and(|r| r.reclame() && notification::notifiable(r)) {
        m.push(Entree::geste("Accepter le rappel", Action::Accepter, true));
        let reporter = format!("Reporter {} min", notification::report_min());
        m.push(Entree { texte: reporter, action: Some(Action::Reporter), actif: true, separateur: false });
        m.push(Entree::separateur());
    }
    let gestes = statuts::gestes_possibles(vue);
    m.push(Entree::geste("Chatouiller", Action::Chatouille, gestes));
    m.push(Entree::geste("Donner un os", Action::Os, gestes));
    m.push(Entree::geste("Donner un bain", Action::Bain, gestes));
    // Jauge : actifs dès que le daemon est joignable, quel que soit l'état du chiot (SPEC-009).
    let joignable = vue.is_some();
    m.push(Entree::geste("Remplir la jauge", Action::Remplir, joignable));
    m.push(Entree::geste("Vider la jauge", Action::Vider, joignable));
    m.push(Entree::geste("Réglages…", Action::Reglages, true));
    m.push(Entree::geste("Quitter Osampo", Action::Quitter, true));
    m
}

fn item(e: Entree) -> MenuItem<Icone> {
    if e.separateur {
        return MenuItem::Separator;
    }
    match e.action {
        None => StandardItem { label: e.texte, enabled: false, ..Default::default() }.into(),
        Some(action) => StandardItem {
            label: e.texte,
            enabled: e.actif,
            activate: Box::new(move |t: &mut Icone| {
                let _ = t.actions.send(action);
            }),
            ..Default::default()
        }
        .into(),
    }
}

pub fn construire(vue: &Vue) -> Vec<MenuItem<Icone>> {
    entrees(vue).into_iter().map(item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ksni::menu::StandardItem as S;

    fn libelles(vue: &Vue) -> Vec<(String, bool)> {
        construire(vue)
            .into_iter()
            .filter_map(|i| match i {
                MenuItem::Standard(S { label, enabled, .. }) => Some((label, enabled)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn entrees_neutres_dans_l_ordre() {
        let e = entrees(&None);
        let n = e.len();
        assert!(e[n - 8].separateur);
        let fin: Vec<_> = e[n - 7..].iter().map(|x| (x.texte.as_str(), x.action, x.actif)).collect();
        assert_eq!(
            fin,
            vec![
                ("Chatouiller", Some(Action::Chatouille), false),
                ("Donner un os", Some(Action::Os), false),
                ("Donner un bain", Some(Action::Bain), false),
                ("Remplir la jauge", Some(Action::Remplir), false),
                ("Vider la jauge", Some(Action::Vider), false),
                ("Réglages…", Some(Action::Reglages), true),
                ("Quitter Osampo", Some(Action::Quitter), true),
            ]
        );
        assert!(e[..n - 8].iter().all(|x| x.action.is_none() && !x.actif && !x.separateur));
    }

    fn avec_rappel(categorie: &str, etat: &str) -> Vue {
        let ligne = format!(
            r#"{{"pc":{{"etat":"reclame","icone":"reclame_laisse","board":"connectee","rappel_en_cours":{{"id":"{categorie},2145","categorie":"{categorie}","heure":"21:45","etat":"{etat}"}}}}}}"#
        );
        Some(crate::statuts::Etat::lire(&ligne).expect("ligne d'état"))
    }

    #[test]
    fn accepter_et_reporter_pendant_un_rappel_en_laisse() {
        let e = entrees(&avec_rappel("dodo", "reclame"));
        let i = e.iter().position(|x| x.separateur).unwrap();
        assert_eq!(e[i + 1].action, Some(Action::Accepter));
        assert_eq!(e[i + 1].texte, "Accepter le rappel");
        assert!(e[i + 1].actif);
        assert_eq!(e[i + 2].action, Some(Action::Reporter));
        assert!(e[i + 2].texte.starts_with("Reporter "), "{}", e[i + 2].texte);
        assert!(e[i + 2].actif);
        assert!(e[i + 3].separateur);
        assert_eq!(e[i + 4].action, Some(Action::Chatouille));
    }

    #[test]
    fn ni_accepter_ni_reporter_sans_rappel_reclame() {
        for vue in [None, avec_rappel("dodo", "reporte"), avec_rappel("repas", "reclame")] {
            let e = entrees(&vue);
            assert!(
                e.iter().all(|x| !matches!(x.action, Some(Action::Accepter | Action::Reporter))),
                "{vue:?}"
            );
        }
    }

    #[test]
    fn jauge_grisee_seulement_daemon_injoignable() {
        let e = libelles(&None);
        let pos = |l: &str| e.iter().position(|(n, _)| n == l).unwrap();
        assert_eq!(pos("Donner un bain"), pos("Donner un os") + 1);
        assert_eq!(pos("Remplir la jauge"), pos("Donner un bain") + 1);
        assert_eq!(pos("Vider la jauge"), pos("Remplir la jauge") + 1);
        assert_eq!(pos("Réglages…"), pos("Vider la jauge") + 1);
        assert_eq!(pos("Quitter Osampo"), pos("Réglages…") + 1);
        assert!(e[pos("Quitter Osampo")].1, "Quitter reste actif, daemon injoignable");
        assert!(!e[pos("Remplir la jauge")].1 && !e[pos("Vider la jauge")].1);
    }
}
