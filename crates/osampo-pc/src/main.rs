//! `osampo-pc` : icône du chien dans la zone de notification, notification du rappel,
//! fenêtre du chien et sons (SPEC-004 et suivantes). Il parle au daemon par son socket
//! (SPEC-002).

mod bureau;
mod client;
mod fenetre;
mod menu;
mod menu_chien;
mod notification;
mod position;
mod ressources;
mod sni;
mod sons;
mod sprites;
mod statuts;

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use client::{ligne_commande, ligne_jauge, Evt, Reponse};
use fenetre::{Fenetre, Humeur, Reaction};
use menu::Action;
use notification::Notification;
use statuts::{Etat, Rappel, Vue};

/// Taille du chien quand `osampo.toml` n'a pas la clé `[pc] chien_px` (SPEC-011).
const CHIEN_PX_DEFAUT: i64 = 64;
/// Intervalle de relecture de `osampo.toml`.
const RELECTURE: Duration = Duration::from_secs(2);

/// `[pc] chien_px` d'après la lecture de `osampo.toml` : fichier absent → 64 ; fichier
/// refusé (illisible, TOML invalide, valeur hors bornes) → `None`, la taille en cours est gardée.
fn chien_px_de(lecture: std::io::Result<String>) -> Option<i64> {
    match lecture {
        Ok(texte) => osampo_config::lire(&texte).ok().map(|r| r.chien_px),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(CHIEN_PX_DEFAUT),
        Err(_) => None,
    }
}

fn lire_chien_px() -> Option<i64> {
    chien_px_de(std::fs::read_to_string(osampo_config::chemin_par_defaut()))
}

/// Réaction du chien à un geste fait sur la board : `dernier_geste` a changé et sa `source`
/// est `board`. Sans état précédent (première ligne après une connexion au daemon), rien : elle
/// porte un geste passé.
fn reaction_board(avant: &Vue, apres: &Etat) -> Option<Reaction> {
    let avant = avant.as_ref()?;
    let geste = apres.dernier_geste.as_ref().filter(|g| avant.dernier_geste.as_ref() != Some(*g))?;
    if geste.source != "board" {
        return None;
    }
    match geste.geste.as_str() {
        "chatouille" => Some(Reaction::Content),
        "os" => Some(Reaction::Mange),
        _ => None,
    }
}

/// Une action venue du chien suit le même chemin que celle du menu de l'icône, mais seulement
/// si l'entrée correspondante est cliquable dans l'état courant (grisée ou daemon injoignable :
/// rien).
fn permise(vue: &Vue, action: Action) -> bool {
    menu::entrees(vue).iter().any(|e| e.action == Some(action) && e.actif)
}

/// Les geste du menu : commande envoyée au daemon, réaction du chien, texte d'échec.
struct Geste {
    cmd: &'static str,
    reaction: Reaction,
    non_transmis: &'static str,
}

const CHATOUILLE: Geste = Geste {
    cmd: "chatouille",
    reaction: Reaction::Content,
    non_transmis: "Board hors ligne : chatouille non transmise",
};
const OS: Geste = Geste {
    cmd: "os",
    reaction: Reaction::Mange,
    non_transmis: "Board hors ligne : os non transmis",
};

/// Suite d'une commande de geste : à faire une fois la réponse du daemon reçue.
#[derive(Debug, PartialEq, Eq)]
enum Suite {
    Reaction,
    ReactionEtNotification,
    Notification(String),
}

fn suite_de(rep: &Reponse) -> Suite {
    match rep {
        Ok(()) => Suite::Reaction,
        Err(e) if e == "board hors ligne" => Suite::ReactionEtNotification,
        Err(e) => Suite::Notification(e.clone()),
    }
}

fn lire_socket() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--socket" {
            match args.next() {
                Some(p) => return PathBuf::from(p),
                None => {
                    eprintln!("osampo-pc : --socket demande un chemin");
                    std::process::exit(2);
                }
            }
        } else if let Some(p) = a.strip_prefix("--socket=") {
            return PathBuf::from(p);
        } else {
            eprintln!("osampo-pc : option inconnue {a} (usage : osampo-pc [--socket <chemin>])");
            std::process::exit(2);
        }
    }
    client::socket_par_defaut()
}

/// Lance `osampo-reglages` (à côté de ce binaire, sinon dans le PATH), sauf s'il tourne
/// déjà depuis ce programme.
fn lancer_reglages(en_cours: &mut Option<Child>) {
    if let Some(c) = en_cours {
        if matches!(c.try_wait(), Ok(None)) {
            return;
        }
    }
    let voisin = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("osampo-reglages")))
        .filter(|p| p.exists());
    let prog = voisin.unwrap_or_else(|| PathBuf::from("osampo-reglages"));
    match Command::new(&prog).spawn() {
        Ok(c) => *en_cours = Some(c),
        Err(e) => eprintln!("lancement de {} impossible : {e}", prog.display()),
    }
}

#[tokio::main]
async fn main() {
    let socket = lire_socket();
    let (evt_tx, mut evt_rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (vue_tx, vue_rx) = mpsc::unbounded_channel();
    let (act_tx, mut act_rx) = mpsc::unbounded_channel();
    let (chien_tx, mut chien_rx) = mpsc::unbounded_channel();
    let (suite_tx, mut suite_rx) = mpsc::unbounded_channel::<(Reaction, &'static str, Reponse)>();
    tokio::spawn(client::boucle(socket, evt_tx, cmd_rx));
    tokio::spawn(sni::superviser(vue_rx, act_tx));
    let (mut notif, mut notif_rx) = Notification::new();
    let mut chien_px = lire_chien_px().unwrap_or(CHIEN_PX_DEFAUT);
    let mut fenetre = Fenetre::new(chien_px as u16, chien_tx);
    let mut releve = tokio::time::interval(RELECTURE);

    let mut vue: Vue = None;
    let mut rappel: Option<Rappel> = None;
    let mut reglages: Option<Child> = None;

    loop {
        tokio::select! {
            e = evt_rx.recv() => {
                let Some(e) = e else { break };
                let nouvelle: Vue = match e {
                    Evt::Etat(e) => Some(*e),
                    Evt::Injoignable => None,
                };
                let nouveau_rappel = nouvelle.as_ref().and_then(|e| e.rappel_en_cours.clone());
                if nouveau_rappel != rappel {
                    let calme = nouvelle.as_ref().is_some_and(|e| e.calme);
                    notif.rappel_change(nouveau_rappel.as_ref(), calme);
                    rappel = nouveau_rappel;
                }
                if let Some(r) = nouvelle.as_ref().and_then(|n| reaction_board(&vue, n)) {
                    fenetre.reaction(r); // geste de la board : sans son, la board joue le sien
                }
                if nouvelle != vue {
                    vue = nouvelle;
                    fenetre.humeur(Humeur::de_icone(vue.as_ref().map_or("assis", |e| e.icone.as_str())));
                    fenetre.menu(menu::entrees(&vue));
                    let _ = vue_tx.send(vue.clone());
                }
            }
            a = act_rx.recv() => match a {
                Some(Action::Quitter) => break quitter(&cmd_tx).await,
                Some(a) => executer(a, rappel.as_ref(), &cmd_tx, &suite_tx, &mut reglages),
                None => break,
            },
            Some(a) = chien_rx.recv() => {
                if a == Action::Quitter {
                    break quitter(&cmd_tx).await;
                }
                if permise(&vue, a) {
                    executer(a, rappel.as_ref(), &cmd_tx, &suite_tx, &mut reglages);
                }
            }
            _ = releve.tick() => {
                if let Some(px) = lire_chien_px() {
                    if px != chien_px {
                        chien_px = px;
                        fenetre.cote(px as u16);
                    }
                }
            }
            s = suite_rx.recv() => {
                let Some((reaction, non_transmis, rep)) = s else { break };
                let calme = vue.as_ref().is_some_and(|e| e.calme);
                let occupee = rappel.as_ref().is_some_and(Rappel::reclame);
                let jouer = |fenetre: &mut Fenetre| {
                    if !occupee {
                        fenetre.reaction(reaction);
                        sons::jouer("jappe", calme);
                    }
                };
                match suite_de(&rep) {
                    Suite::Reaction => jouer(&mut fenetre),
                    Suite::ReactionEtNotification => {
                        jouer(&mut fenetre);
                        tokio::spawn(async move { bureau::notifier(non_transmis, "").await });
                    }
                    Suite::Notification(erreur) => {
                        tokio::spawn(async move { bureau::notifier("Osampo", &erreur).await });
                    }
                }
            }
            n = notif_rx.recv() => {
                if let Some(n) = n {
                    let (tx, _) = oneshot::channel();
                    let _ = cmd_tx.send((ligne_commande(n.cmd, Some(&n.rappel)), tx));
                }
            }
        }
    }
}

/// Ce que fait une action du menu (de l'icône ou du chien) ou le clic sur le chien.
fn executer(
    action: Action,
    rappel: Option<&Rappel>,
    cmd_tx: &mpsc::UnboundedSender<client::Commande>,
    suite_tx: &mpsc::UnboundedSender<(Reaction, &'static str, Reponse)>,
    reglages: &mut Option<Child>,
) {
    match action {
        Action::Reglages => lancer_reglages(reglages),
        Action::Chatouille => envoyer_geste(&CHATOUILLE, cmd_tx, suite_tx),
        Action::Os => envoyer_geste(&OS, cmd_tx, suite_tx),
        Action::Remplir => regler_jauge(100, cmd_tx),
        Action::Vider => regler_jauge(0, cmd_tx),
        Action::Quitter => {} // traité par la boucle principale : il faut en sortir
        // Même commande que les boutons de la notification ; le daemon retire ensuite le rappel.
        Action::Accepter | Action::Reporter => {
            let Some(r) = rappel else { return };
            let cmd = if action == Action::Accepter { "accepter" } else { "reporter" };
            let (tx, _) = oneshot::channel();
            let _ = cmd_tx.send((ligne_commande(cmd, Some(&r.id)), tx));
        }
    }
}

/// Délai laissé au daemon pour répondre à `quitter` ; injoignable, on quitte quand même.
const ATTENTE_QUITTER: Duration = Duration::from_secs(2);

/// Menu « Quitter Osampo » : demande au daemon de s'arrêter, puis rend la main pour que
/// `osampo-pc` sorte.
async fn quitter(cmd_tx: &mpsc::UnboundedSender<client::Commande>) {
    let (tx, rx) = oneshot::channel();
    let _ = cmd_tx.send((ligne_commande("quitter", None), tx));
    match tokio::time::timeout(ATTENTE_QUITTER, rx).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(e))) => eprintln!("osampo-pc : arrêt du daemon : {e}"),
        _ => eprintln!("osampo-pc : daemon sans réponse à quitter"),
    }
}

/// Envoie la commande du geste ; la réponse revient par `suite_tx`.
fn envoyer_geste(
    geste: &Geste,
    cmd_tx: &mpsc::UnboundedSender<client::Commande>,
    suite_tx: &mpsc::UnboundedSender<(Reaction, &'static str, Reponse)>,
) {
    let (tx, rx) = oneshot::channel();
    let _ = cmd_tx.send((ligne_commande(geste.cmd, None), tx));
    let suite_tx = suite_tx.clone();
    let (reaction, non_transmis) = (geste.reaction, geste.non_transmis);
    tokio::spawn(async move {
        let rep = rx.await.unwrap_or_else(|_| Err("daemon injoignable".into()));
        let _ = suite_tx.send((reaction, non_transmis, rep));
    });
}

/// Règle la jauge sage du daemon (SPEC-009) : réponse `ok` sans réaction du chien ; en erreur,
/// notification de bureau avec le texte de l'erreur.
fn regler_jauge(valeur: u8, cmd_tx: &mpsc::UnboundedSender<client::Commande>) {
    let (tx, rx) = oneshot::channel();
    let _ = cmd_tx.send((ligne_jauge(valeur), tx));
    tokio::spawn(async move {
        let rep = rx.await.unwrap_or_else(|_| Err("daemon injoignable".into()));
        if let Err(erreur) = rep {
            bureau::notifier("Osampo", &erreur).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_des_commandes() {
        assert_eq!(suite_de(&Ok(())), Suite::Reaction);
        assert_eq!(suite_de(&Err("board hors ligne".into())), Suite::ReactionEtNotification);
        assert_eq!(
            suite_de(&Err("le chiot ne peut pas jouer maintenant".into())),
            Suite::Notification("le chiot ne peut pas jouer maintenant".into())
        );
    }

    fn etat(icone: &str, etat: &str, geste: Option<(&str, &str, &str)>) -> Etat {
        let g = geste
            .map(|(g, s, h)| format!(r#"{{"geste":"{g}","source":"{s}","heure":"{h}"}}"#))
            .unwrap_or_else(|| "null".into());
        let ligne = format!(
            r#"{{"pc":{{"etat":"{etat}","icone":"{icone}","board":"connectee","dernier_geste":{g}}}}}"#
        );
        Etat::lire(&ligne).expect("ligne d'état")
    }

    #[test]
    fn geste_de_la_board_declenche_une_reaction() {
        let avant = Some(etat("assis", "repos", Some(("os", "board", "14:12"))));
        let apres = |g: Option<(&str, &str, &str)>| etat("assis", "repos", g);
        // même geste : rien
        assert_eq!(reaction_board(&avant, &apres(Some(("os", "board", "14:12")))), None);
        // nouveau geste de la board
        assert_eq!(
            reaction_board(&avant, &apres(Some(("chatouille", "board", "14:13")))),
            Some(Reaction::Content)
        );
        assert_eq!(reaction_board(&avant, &apres(Some(("os", "board", "14:14")))), Some(Reaction::Mange));
        // geste du PC : déjà joué (SPEC-006)
        assert_eq!(reaction_board(&avant, &apres(Some(("os", "pc", "14:15")))), None);
        // geste inconnu, aucun geste
        assert_eq!(reaction_board(&avant, &apres(Some(("saut", "board", "14:16")))), None);
        assert_eq!(reaction_board(&avant, &apres(None)), None);
    }

    #[test]
    fn premiere_ligne_apres_connexion_sans_reaction() {
        let nouvelle = etat("assis", "repos", Some(("os", "board", "09:00")));
        assert_eq!(reaction_board(&None, &nouvelle), None);
        // un premier geste après une ligne sans geste compte
        let avant = Some(etat("assis", "repos", None));
        assert_eq!(reaction_board(&avant, &nouvelle), Some(Reaction::Mange));
    }

    #[test]
    fn taille_du_chien_depuis_le_fichier() {
        use std::io::{Error, ErrorKind};
        assert_eq!(chien_px_de(Ok("[pc]\nchien_px = 100\n".into())), Some(100));
        assert_eq!(chien_px_de(Ok(String::new())), Some(64));
        // fichier absent : 64
        assert_eq!(chien_px_de(Err(Error::from(ErrorKind::NotFound))), Some(64));
        // fichier refusé : la taille en cours est gardée
        assert_eq!(chien_px_de(Ok("[pc]\nchien_px = 300\n".into())), None);
        assert_eq!(chien_px_de(Ok("pas du toml [".into())), None);
        assert_eq!(chien_px_de(Err(Error::from(ErrorKind::PermissionDenied))), None);
    }

    #[test]
    fn actions_du_chien_selon_l_etat() {
        // daemon injoignable : rien, sauf les réglages
        assert!(!permise(&None, Action::Chatouille));
        assert!(!permise(&None, Action::Remplir));
        assert!(permise(&None, Action::Reglages));
        let calme = Some(etat("assis", "repos", None));
        assert!(permise(&calme, Action::Chatouille));
        assert!(permise(&calme, Action::Vider));
        // il dort, board connectée : les gestes sont grisés
        let dort = Some(etat("niche", "dort", None));
        assert!(!permise(&dort, Action::Os));
    }
}
