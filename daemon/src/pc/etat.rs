//! Etat du chiot publie aux programmes du PC (SPEC-002) et synchro des rappels (SPEC-003).
//! Pur : entrees = lignes montantes de la board, lien, jauge `sage`, reglages, tic, commandes
//! clients ; sorties = ligne d etat, reponses, lignes a ecrire a la board.

use super::rappels::{Rappels, PORTEES};
use super::Heure;
use crate::config::Reglages;
use crate::sources::activite::EmetteurJauge;
use serde_json::{json, Value};

const MAX_LIGNE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EtatRappel {
    Reclame,
    Reporte,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnCours {
    pub id: String,
    pub categorie: String,
    pub heure: String,
    pub etat: EtatRappel,
    /// Fin du report (epoch), si `Reporte`.
    reporte_jusqu: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct Geste {
    geste: String,
    source: String,
    heure: String,
}

/// Commande accepter / reporter prise pendant que le lien etait coupe.
#[derive(Clone, Debug, PartialEq)]
struct EnAttente {
    cmd: &'static str,
    id: String,
}

pub struct EtatPc {
    rappels: Rappels,
    report_min: i64,
    connecte: bool,
    /// Premiere ligne d etat de la board apres un retour de lien : la commande en attente
    /// est rejouee si la board reclame encore ce rappel.
    premiere_ligne: bool,
    etat: String,
    raison: String,
    /// Pose de reclamation de la board (SPEC-012) ; `-` si absente.
    pose: String,
    rappel: String,
    promenade_debut: String,
    pile: i64,
    besoins: Option<(i64, i64, i64)>,
    sage: i64,
    en_cours: Option<EnCours>,
    en_attente: Option<EnAttente>,
    dernier_geste: Option<Geste>,
    /// Reglage de la jauge (SPEC-009) ; None : le daemon ne la calcule pas.
    jauge: Option<EmetteurJauge>,
}

fn hhmm_de_id(id: &str) -> String {
    match id.split_once(',') {
        Some((_, h)) if h.len() == 4 => format!("{}:{}", &h[..2], &h[2..]),
        _ => "-".into(),
    }
}

/// Commande `quitter` (menu « Quitter Osampo ») : le daemon repond `ok` puis s arrete.
pub fn demande_l_arret(ligne: &str) -> bool {
    serde_json::from_str::<Value>(ligne)
        .ok()
        .is_some_and(|v| v.get("cmd").and_then(Value::as_str) == Some("quitter"))
}

fn reponse(nom: &str, erreur: Option<&str>) -> String {
    match erreur {
        None => json!({"reponse": nom, "ok": true}),
        Some(e) => json!({"reponse": nom, "ok": false, "erreur": e}),
    }
    .to_string()
}

impl EtatPc {
    pub fn new(rappels: Rappels, reglages: &Reglages) -> EtatPc {
        EtatPc {
            rappels,
            report_min: reglages.report_min,
            connecte: false,
            premiere_ligne: false,
            etat: "-".into(),
            raison: "-".into(),
            pose: "-".into(),
            rappel: "-".into(),
            promenade_debut: "-".into(),
            pile: -1,
            besoins: None,
            sage: 100,
            en_cours: None,
            en_attente: None,
            dernier_geste: None,
            jauge: None,
        }
    }

    pub fn jauge_reglable(&mut self, j: Option<EmetteurJauge>) {
        self.jauge = j;
    }

    pub fn reglages(&mut self, r: &Reglages) {
        self.report_min = r.report_min;
        self.rappels.reglages(r);
    }

    pub fn sage(&mut self, v: i64) {
        self.sage = v.clamp(0, 100);
    }

    #[cfg(test)]
    pub fn en_cours(&self) -> Option<&EnCours> {
        self.en_cours.as_ref()
    }

    /// Lien vu connecte (et abonne) ou coupe.
    pub fn lien(&mut self, connecte: bool) {
        if connecte == self.connecte {
            return;
        }
        self.connecte = connecte;
        if connecte {
            self.premiere_ligne = true;
        } else {
            self.pile = -1;
            self.premiere_ligne = false;
            // la reclamation de la jauge ne survit pas a la coupure ; un rappel, si.
            if self.en_cours.as_ref().is_some_and(|e| e.categorie == "promenade") {
                self.en_cours = None;
            }
        }
    }

    /// Une ligne montante de la board (ligne d etat ou evenement). Rend les lignes a ecrire
    /// a la board (commande rejouee au retour du lien).
    pub fn ligne_board(&mut self, ligne: &str, now: Heure) -> Vec<String> {
        if ligne.len() > MAX_LIGNE {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(ligne) else {
            return vec![];
        };
        if v.get("etat").is_some() {
            return self.ligne_etat(&v, now);
        }
        if let Some(evt) = v.get("evt").and_then(Value::as_str) {
            self.evenement(evt, &v, now);
        }
        vec![]
    }

    fn ligne_etat(&mut self, v: &Value, now: Heure) -> Vec<String> {
        let texte = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("-").to_string();
        self.etat = texte("etat");
        self.raison = texte("raison");
        self.pose = texte("pose");
        self.rappel = texte("rappel");
        self.promenade_debut = texte("promenade_debut");
        if let Some(p) = v.get("pile").and_then(Value::as_i64) {
            self.pile = p;
        }
        if let Some(b) = v.get("besoins") {
            let n = |k: &str| b.get(k).and_then(Value::as_i64);
            if let (Some(a), Some(j), Some(d)) = (n("bouger"), n("jouer"), n("dormir")) {
                self.besoins = Some((a, j, d));
            }
        }
        let reclame = self.etat == "reclame";
        let mut a_envoyer = vec![];
        let mut rejoue = false;
        if std::mem::take(&mut self.premiere_ligne) {
            if let Some(att) = self.en_attente.take() {
                let meme = reclame
                    && if att.id == "promenade" {
                        self.raison == "promenade"
                    } else {
                        self.rappel == att.id
                    };
                if meme {
                    a_envoyer.push(json!({"cmd": att.cmd, "rappel": att.id}).to_string());
                    rejoue = true;
                }
            }
        }
        if rejoue {
            return a_envoyer; // le rappel reste tel que le PC l a laisse
        }
        if reclame && self.raison == "rappel" {
            let id = self.rappel.clone();
            let categorie = id.split(',').next().unwrap_or("").to_string();
            if PORTEES.contains(&categorie.as_str()) {
                let deja = self.en_cours.as_ref().is_some_and(|e| e.id == id);
                if deja {
                    if let Some(e) = &mut self.en_cours {
                        e.etat = EtatRappel::Reclame;
                    }
                } else {
                    self.rappels.marquer(&id, self.jour_de(&id, now));
                    self.en_cours = Some(EnCours {
                        heure: hhmm_de_id(&id),
                        id,
                        categorie,
                        etat: EtatRappel::Reclame,
                        reporte_jusqu: 0,
                    });
                }
            } else {
                self.en_cours = None; // repas : ni sortie ni chien du PC
            }
        } else if reclame && self.raison == "promenade" {
            let libre = match &self.en_cours {
                None => true,
                Some(e) => e.categorie == "promenade" && e.etat == EtatRappel::Reporte,
            };
            if libre {
                self.en_cours = Some(EnCours {
                    id: "promenade".into(),
                    categorie: "promenade".into(),
                    heure: now.hhmm(),
                    etat: EtatRappel::Reclame,
                    reporte_jusqu: 0,
                });
            }
        } else if self
            .en_cours
            .as_ref()
            .is_some_and(|e| e.categorie == "promenade" && e.etat == EtatRappel::Reclame)
        {
            self.en_cours = None;
        }
        a_envoyer
    }

    /// Jour de programmation d un rappel adopte : aujourd hui, ou la veille si la minute du
    /// rappel est dans la soiree et qu on est juste apres minuit.
    fn jour_de(&self, id: &str, now: Heure) -> i64 {
        match self.rappels.rappel(id) {
            Some(r) if r.minute > now.minute + 720 => now.jour - 1,
            _ => now.jour,
        }
    }

    fn evenement(&mut self, evt: &str, v: &Value, now: Heure) {
        match evt {
            "os" | "chatouille" | "bain" => {
                let source = v.get("source").and_then(Value::as_str).unwrap_or("board");
                self.dernier_geste = Some(Geste {
                    geste: evt.into(),
                    source: source.into(),
                    heure: now.hhmm(),
                });
            }
            "accepte" | "reporte" | "abandonne" => {
                let id = v.get("rappel").and_then(Value::as_str).unwrap_or("");
                if self.en_cours.as_ref().is_none_or(|e| e.id != id) {
                    return;
                }
                match evt {
                    "reporte" => self.reporter(now),
                    _ => self.en_cours = None,
                }
            }
            _ => {}
        }
    }

    fn reporter(&mut self, now: Heure) {
        let jusqu = now.epoch_s + self.report_min * 60;
        if let Some(e) = &mut self.en_cours {
            e.etat = EtatRappel::Reporte;
            e.reporte_jusqu = jusqu;
        }
    }

    /// Tic d une seconde : fin d un report, declenchement d un rappel du.
    pub fn tic(&mut self, now: Heure) {
        if let Some(e) = &mut self.en_cours {
            if e.etat == EtatRappel::Reporte && now.epoch_s >= e.reporte_jusqu {
                if e.categorie == "promenade" {
                    self.en_cours = None;
                } else {
                    e.etat = EtatRappel::Reclame;
                }
            }
        }
        // un seul rappel a la fois ; il remplace une reclamation de promenade.
        let rappel_en_cours = self.en_cours.as_ref().is_some_and(|e| e.categorie != "promenade");
        if rappel_en_cours {
            return;
        }
        if let Some(d) = self.rappels.du(now) {
            self.rappels.marquer(&d.rappel.id, d.jour);
            self.en_cours = Some(EnCours {
                heure: d.rappel.heure(),
                categorie: d.rappel.categorie,
                id: d.rappel.id,
                etat: EtatRappel::Reclame,
                reporte_jusqu: 0,
            });
        }
    }

    /// Commande d un client : (reponse sur une ligne, lignes a ecrire a la board).
    pub fn commande(&mut self, ligne: &str, now: Heure) -> (String, Vec<String>) {
        let inconnue = || (reponse("?", Some("commande inconnue")), vec![]);
        let Ok(v) = serde_json::from_str::<Value>(ligne) else {
            return inconnue();
        };
        match v.get("cmd").and_then(Value::as_str) {
            Some(c @ ("chatouille" | "os" | "bain")) => {
                let nom = match c {
                    "os" => "os",
                    "bain" => "bain",
                    _ => "chatouille",
                };
                if !self.connecte {
                    return (reponse(nom, Some("board hors ligne")), vec![]);
                }
                if matches!(self.etat.as_str(), "dort" | "reclame" | "promenade") {
                    return (reponse(nom, Some("le chiot ne peut pas jouer maintenant")), vec![]);
                }
                self.dernier_geste = Some(Geste {
                    geste: nom.into(),
                    source: "pc".into(),
                    heure: now.hhmm(),
                });
                (reponse(nom, None), vec![json!({"cmd": nom}).to_string()])
            }
            Some("jauge") => {
                let Some(j) = &self.jauge else {
                    return (reponse("jauge", Some("jauge indisponible")), vec![]);
                };
                // as_i64 refuse 42.5, "42", null et l absence de la cle.
                match v.get("valeur").and_then(Value::as_i64).filter(|n| (0..=100).contains(n)) {
                    Some(n) if j.regler(n) => (reponse("jauge", None), vec![]),
                    Some(_) => (reponse("jauge", Some("jauge indisponible")), vec![]),
                    None => (reponse("jauge", Some("valeur hors de 0..100")), vec![]),
                }
            }
            Some(c @ ("accepter" | "reporter")) => {
                let nom = c;
                let cmd: &'static str = if c == "accepter" { "accepter" } else { "reporter" };
                let Some(id) = v.get("rappel").and_then(Value::as_str) else {
                    return inconnue();
                };
                if self.en_cours.as_ref().is_none_or(|e| e.id != id) {
                    return (reponse(nom, Some(&format!("aucun rappel {id} en cours"))), vec![]);
                }
                if cmd == "accepter" {
                    self.en_cours = None;
                } else {
                    self.reporter(now);
                }
                if self.connecte {
                    (reponse(nom, None), vec![json!({"cmd": cmd, "rappel": id}).to_string()])
                } else {
                    self.en_attente = Some(EnAttente { cmd, id: id.to_string() });
                    (reponse(nom, None), vec![])
                }
            }
            Some("quitter") => (reponse("quitter", None), vec![]),
            _ => inconnue(),
        }
    }

    /// Pose de reclamation de la board : celle de la ligne d etat, sinon recalculee depuis
    /// `raison` et la categorie de `rappel` (SPEC-012). Raison ou categorie inconnue : `aboie`
    /// (comportement d avant les poses).
    fn pose_reclamee(&self) -> &'static str {
        match self.pose.as_str() {
            "patte" => return "patte",
            "laisse" => return "laisse",
            "aboie" => return "aboie",
            _ => {}
        }
        match self.raison.as_str() {
            "claude" => "patte",
            "promenade" => "laisse",
            "rappel" => match self.rappel.split(',').next().unwrap_or("") {
                "sortie" | "sport" | "dodo" => "laisse",
                _ => "aboie",
            },
            _ => "aboie",
        }
    }

    /// Icone (SPEC-002, « Choix de l'icone »).
    pub fn icone(&self) -> &'static str {
        if self.connecte && self.etat == "reclame" {
            return match self.pose_reclamee() {
                "patte" => "patte",
                "laisse" => "reclame_laisse",
                _ => "aboie",
            };
        }
        if self.en_cours.as_ref().is_some_and(|e| e.etat == EtatRappel::Reclame) {
            "aboie"
        } else if self.etat == "dort" {
            "niche"
        } else if self.etat == "promenade" {
            "laisse"
        } else {
            "assis"
        }
    }

    /// La ligne `{"pc":{...}}` (sans `\n`).
    pub fn publier(&self, now: Heure) -> String {
        let besoins = match self.besoins {
            Some((b, j, d)) => json!({"bouger": b, "jouer": j, "dormir": d}),
            None => Value::Null,
        };
        let en_cours = match &self.en_cours {
            Some(e) => json!({
                "id": e.id, "categorie": e.categorie, "heure": e.heure,
                "etat": if e.etat == EtatRappel::Reclame { "reclame" } else { "reporte" },
            }),
            None => Value::Null,
        };
        let geste = match &self.dernier_geste {
            Some(g) => json!({"geste": g.geste, "source": g.source, "heure": g.heure}),
            None => Value::Null,
        };
        json!({"pc": {
            "etat": self.etat,
            "icone": self.icone(),
            "sage": self.sage,
            "prochain_rappel": self.rappels.prochain(now, self.en_cours.as_ref().map(|e| e.id.as_str())),
            "promenade_debut": self.promenade_debut,
            "board": if self.connecte { "connectee" } else { "hors_ligne" },
            "pile": if self.connecte { self.pile } else { -1 },
            "besoins": besoins,
            "rappel_en_cours": en_cours,
            "calme": self.rappels.calme(now.minute),
            "dernier_geste": geste,
        }})
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::super::rappels::tests::{dim, jour_plus, reglages};
    use super::*;

    fn pc(rappels: &str) -> EtatPc {
        let g = reglages(rappels);
        EtatPc::new(Rappels::new(None, &g), &g)
    }

    fn pub_(p: &EtatPc, now: Heure) -> Value {
        serde_json::from_str::<Value>(&p.publier(now)).unwrap()["pc"].clone()
    }

    fn etat(e: &str, raison: &str, rappel: &str) -> String {
        format!(
            r#"{{"etat":"{e}","raison":"{raison}","rappel":"{rappel}","promenade_debut":"-","pile":83,"besoins":{{"bouger":70,"jouer":60,"dormir":50}}}}"#
        )
    }

    #[test]
    fn etat_initial() {
        let p = pc("sortie,1230,127");
        let v = pub_(&p, dim(10, 0));
        assert_eq!(v["etat"], "-");
        assert_eq!(v["icone"], "assis");
        assert_eq!(v["board"], "hors_ligne");
        assert_eq!(v["pile"], -1);
        assert!(v["besoins"].is_null() && v["rappel_en_cours"].is_null() && v["dernier_geste"].is_null());
        assert_eq!(v["prochain_rappel"], "sortie à 12:30");
        assert_eq!(v["calme"], false);
        assert_eq!(v["sage"], 100);
    }

    #[test]
    fn lignes_de_la_board_et_icone() {
        let mut p = pc("sortie,1230,127");
        let n = dim(10, 0);
        p.lien(true);
        p.ligne_board(&etat("dort", "-", "-"), n);
        let v = pub_(&p, n);
        assert_eq!((v["icone"].as_str(), v["board"].as_str(), v["pile"].as_i64()), (Some("niche"), Some("connectee"), Some(83)));
        assert_eq!(v["besoins"]["jouer"], 60);
        p.ligne_board(&etat("promenade", "-", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "laisse");
        p.ligne_board(&etat("reclame", "claude", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "patte");
        // lien coupe : reclame garde -> assis, etat et besoins gardes
        p.lien(false);
        let v = pub_(&p, n);
        assert_eq!((v["icone"].as_str(), v["etat"].as_str(), v["pile"].as_i64()), (Some("assis"), Some("reclame"), Some(-1)));
        assert_eq!(v["besoins"]["bouger"], 70);
        // promenade emportee hors ligne : laisse
        p.lien(true);
        p.ligne_board(&etat("promenade", "-", "-"), n);
        p.lien(false);
        assert_eq!(pub_(&p, n)["icone"], "laisse");
        // ligne illisible ou trop longue : ignoree
        p.ligne_board("pas du json", n);
        p.ligne_board(&"x".repeat(5000), n);
    }

    fn etat_pose(raison: &str, pose: &str, rappel: &str) -> String {
        format!(
            r#"{{"etat":"reclame","raison":"{raison}","pose":"{pose}","rappel":"{rappel}","promenade_debut":"-","pile":83}}"#
        )
    }

    #[test]
    fn icone_copie_la_pose_de_la_board() {
        let mut p = pc("sortie,1230,127");
        let n = dim(12, 29);
        p.lien(true);
        // patte puis aboie (escalade de Claude)
        p.ligne_board(&etat_pose("claude", "patte", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "patte");
        p.ligne_board(&etat_pose("claude", "aboie", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "aboie");
        // jauge vide : laisse dans la gueule
        p.ligne_board(&etat_pose("promenade", "laisse", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "reclame_laisse");
        // repas : aboie
        p.ligne_board(&etat_pose("rappel", "aboie", "repas,1200"), n);
        assert_eq!(pub_(&p, n)["icone"], "aboie");
        // sortie reclamee par la board connectee : reclame_laisse, pas aboie
        p.ligne_board(&etat_pose("rappel", "laisse", "sortie,1230"), n);
        assert_eq!(p.en_cours().unwrap().id, "sortie,1230");
        assert_eq!(pub_(&p, n)["icone"], "reclame_laisse");
        // le daemon declenche le meme rappel : la board reclame deja, toujours reclame_laisse
        p.tic(dim(12, 30));
        assert_eq!(pub_(&p, n)["icone"], "reclame_laisse");
        // board au repos pendant le rappel : aboie
        p.ligne_board(&etat("repos", "-", "-"), n);
        assert_eq!(pub_(&p, n)["icone"], "aboie");
        // lien coupe pendant le rappel : aboie
        p.ligne_board(&etat_pose("rappel", "laisse", "sortie,1230"), n);
        p.lien(false);
        assert_eq!(pub_(&p, n)["icone"], "aboie");
    }

    #[test]
    fn relance_en_laisse_aboie_puis_reprend_la_laisse() {
        // SPEC-012, « Suivi par le PC » : chaque relance en laisse passe par `aboie`
        // et revient a `reclame_laisse`, sans quitter la reclamation de sortie.
        for (raison, rappel) in [("rappel", "sortie,1230"), ("promenade", "-")] {
            let mut p = pc("sortie,1230,127");
            let n = dim(12, 30);
            p.lien(true);
            if raison == "rappel" {
                p.tic(n); // le daemon declenche le rappel de sortie
            }
            for (pose, attendu) in [
                ("laisse", "reclame_laisse"),
                ("aboie", "aboie"),
                ("laisse", "reclame_laisse"),
                ("aboie", "aboie"),
                ("laisse", "reclame_laisse"),
            ] {
                p.ligne_board(&etat_pose(raison, pose, rappel), n);
                let v = pub_(&p, n);
                assert_eq!(v["icone"], attendu, "{raison} {pose}");
                if raison == "rappel" {
                    // la fenetre de sortie suit rappel_en_cours : elle reste en reclamation
                    assert_eq!(v["rappel_en_cours"]["etat"], "reclame", "{raison} {pose}");
                    assert_eq!(p.en_cours().unwrap().id, "sortie,1230");
                }
            }
        }
    }

    #[test]
    fn icone_sans_pose_recalculee() {
        let mut p = pc("");
        let n = dim(12, 30);
        p.lien(true);
        for (ligne, attendu) in [
            (etat("reclame", "claude", "-"), "patte"),
            (etat("reclame", "promenade", "-"), "reclame_laisse"),
            (etat("reclame", "rappel", "sortie,1230"), "reclame_laisse"),
            (etat("reclame", "rappel", "dodo,2100"), "reclame_laisse"),
            (etat("reclame", "rappel", "repas,1200"), "aboie"),
            // pose inconnue ou `-` : meme recalcul
            (etat_pose("rappel", "danse", "sport,1800"), "reclame_laisse"),
            (etat_pose("claude", "-", "-"), "patte"),
        ] {
            p.ligne_board(&ligne, n);
            assert_eq!(pub_(&p, n)["icone"], attendu, "{ligne}");
        }
    }

    #[test]
    fn icone_reclame_garde_apres_coupure() {
        let mut p = pc("");
        let n = dim(12, 30);
        p.lien(true);
        for pose in ["patte", "laisse", "aboie"] {
            p.ligne_board(&etat_pose("claude", pose, "-"), n);
            p.lien(false);
            assert_eq!(pub_(&p, n)["icone"], "assis", "{pose}");
            p.lien(true);
        }
    }

    #[test]
    fn gestes_et_commandes_jeu() {
        let mut p = pc("sortie,1230,127");
        let n = dim(14, 12);
        let (r, l) = p.commande(r#"{"cmd":"chatouille"}"#, n);
        assert_eq!(r, r#"{"erreur":"board hors ligne","ok":false,"reponse":"chatouille"}"#);
        assert!(l.is_empty());
        p.lien(true);
        p.ligne_board(&etat("dort", "-", "-"), n);
        let (r, _) = p.commande(r#"{"cmd":"os"}"#, n);
        assert!(r.contains("le chiot ne peut pas jouer maintenant"), "{r}");
        p.ligne_board(&etat("repos", "-", "-"), n);
        let (r, l) = p.commande(r#"{"cmd":"os"}"#, n);
        assert_eq!(r, r#"{"ok":true,"reponse":"os"}"#);
        assert_eq!(l, vec![r#"{"cmd":"os"}"#]);
        assert_eq!(pub_(&p, n)["dernier_geste"], json!({"geste": "os", "source": "pc", "heure": "14:12"}));
        p.ligne_board(r#"{"evt":"chatouille","source":"board"}"#, dim(14, 20));
        assert_eq!(pub_(&p, n)["dernier_geste"], json!({"geste": "chatouille", "source": "board", "heure": "14:20"}));
        let (r, l) = p.commande(r#"{"cmd":"bain"}"#, dim(14, 21));
        assert_eq!(r, r#"{"ok":true,"reponse":"bain"}"#);
        assert_eq!(l, vec![r#"{"cmd":"bain"}"#]);
        assert_eq!(pub_(&p, n)["dernier_geste"], json!({"geste": "bain", "source": "pc", "heure": "14:21"}));
        p.ligne_board(r#"{"evt":"bain","source":"board"}"#, dim(14, 22));
        assert_eq!(pub_(&p, n)["dernier_geste"], json!({"geste": "bain", "source": "board", "heure": "14:22"}));
        p.ligne_board(&etat("promenade", "-", "-"), n);
        let (r, _) = p.commande(r#"{"cmd":"bain"}"#, n);
        assert!(r.contains("le chiot ne peut pas jouer maintenant"), "{r}");
        for l in ["n importe quoi", r#"{"cmd":"danse"}"#, r#"{"cmd":"accepter"}"#] {
            let (r, l) = p.commande(l, n);
            assert_eq!(r, r#"{"erreur":"commande inconnue","ok":false,"reponse":"?"}"#);
            assert!(l.is_empty());
        }
    }

    #[test]
    fn rappel_du_daemon_accepter_reporter() {
        let mut p = pc("sortie,1230,127");
        p.tic(dim(12, 29));
        assert!(p.en_cours().is_none());
        p.tic(dim(12, 30));
        let v = pub_(&p, dim(12, 30));
        assert_eq!(v["rappel_en_cours"], json!({"id":"sortie,1230","categorie":"sortie","heure":"12:30","etat":"reclame"}));
        assert_eq!(v["icone"], "aboie");
        assert_eq!(v["prochain_rappel"], "sortie demain à 12:30");
        let (r, _) = p.commande(r#"{"cmd":"accepter","rappel":"sortie,1300"}"#, dim(12, 31));
        assert!(r.contains("aucun rappel sortie,1300 en cours"), "{r}");
        // report 10 min, board hors ligne : rien a envoyer
        let t = dim(12, 31);
        let (r, l) = p.commande(r#"{"cmd":"reporter","rappel":"sortie,1230"}"#, t);
        assert_eq!(r, r#"{"ok":true,"reponse":"reporter"}"#);
        assert!(l.is_empty());
        assert_eq!(pub_(&p, t)["rappel_en_cours"]["etat"], "reporte");
        assert_eq!(pub_(&p, t)["icone"], "assis");
        p.tic(dim(12, 40));
        assert_eq!(p.en_cours().unwrap().etat, EtatRappel::Reporte);
        p.tic(dim(12, 41));
        assert_eq!(p.en_cours().unwrap().etat, EtatRappel::Reclame);
        p.commande(r#"{"cmd":"accepter","rappel":"sortie,1230"}"#, dim(12, 42));
        assert!(p.en_cours().is_none());
        p.tic(dim(12, 43)); // une fois par jour
        assert!(p.en_cours().is_none());
    }

    #[test]
    fn evenements_de_la_board_et_adoption() {
        let mut p = pc("sortie,1230,127;sport,1800,127");
        let n = dim(12, 30);
        p.lien(true);
        // la board reclame sport alors que le daemon n a rien declenche : adoption
        p.ligne_board(&etat("reclame", "rappel", "sport,1800"), n);
        assert_eq!(p.en_cours().unwrap().id, "sport,1800");
        assert_eq!(p.en_cours().unwrap().heure, "18:00");
        // ne le redeclenche pas
        p.tic(dim(18, 0));
        assert_eq!(p.en_cours().unwrap().id, "sport,1800");
        // la board en reclame un autre : il remplace
        p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), n);
        assert_eq!(p.en_cours().unwrap().id, "sortie,1230");
        p.ligne_board(r#"{"evt":"reporte","rappel":"sortie,1230"}"#, n);
        assert_eq!(p.en_cours().unwrap().etat, EtatRappel::Reporte);
        p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), dim(12, 41));
        assert_eq!(p.en_cours().unwrap().etat, EtatRappel::Reclame);
        p.ligne_board(r#"{"evt":"abandonne","rappel":"sortie,1230"}"#, n);
        assert!(p.en_cours().is_none());
        // evenement pour un autre id : ignore
        p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), n);
        p.ligne_board(r#"{"evt":"accepte","rappel":"sport,1800"}"#, n);
        assert!(p.en_cours().is_some());
        p.ligne_board(r#"{"evt":"accepte","rappel":"sortie,1230"}"#, n);
        assert!(p.en_cours().is_none());
        // repas reclame, lien connecte : null
        p.ligne_board(&etat("reclame", "rappel", "repas,1200"), n);
        assert!(p.en_cours().is_none());
        p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), n);
        p.ligne_board(&etat("reclame", "rappel", "repas,1200"), n);
        assert!(p.en_cours().is_none());
    }

    #[test]
    fn reclamation_de_promenade() {
        let mut p = pc("sortie,1230,127");
        p.lien(true);
        let n = dim(14, 20);
        p.ligne_board(&etat("reclame", "promenade", "-"), n);
        let v = pub_(&p, n);
        assert_eq!(v["rappel_en_cours"], json!({"id":"promenade","categorie":"promenade","heure":"14:20","etat":"reclame"}));
        // la jauge remonte, sans evenement : null
        p.ligne_board(&etat("repos", "-", "-"), n);
        assert!(p.en_cours().is_none());
        // reporter depuis le PC : relayee, reporte, puis null apres report_min si la board ne redemande pas
        p.ligne_board(&etat("reclame", "promenade", "-"), n);
        let (r, l) = p.commande(r#"{"cmd":"reporter","rappel":"promenade"}"#, n);
        assert_eq!(r, r#"{"ok":true,"reponse":"reporter"}"#);
        assert_eq!(l, vec![r#"{"cmd":"reporter","rappel":"promenade"}"#]);
        assert_eq!(p.en_cours().unwrap().etat, EtatRappel::Reporte);
        p.ligne_board(&etat("repos", "-", "-"), n); // pas en null : c est un report
        assert!(p.en_cours().is_some());
        p.tic(dim(14, 31));
        assert!(p.en_cours().is_none());
        // reporte puis la board redemande : de nouveau reclame, heure du retour
        p.ligne_board(&etat("reclame", "promenade", "-"), n);
        p.ligne_board(r#"{"evt":"reporte","rappel":"promenade"}"#, n);
        p.ligne_board(&etat("reclame", "promenade", "-"), dim(14, 35));
        let e = p.en_cours().unwrap();
        assert_eq!((e.etat, e.heure.as_str()), (EtatRappel::Reclame, "14:35"));
        // accepte par la board
        p.ligne_board(r#"{"evt":"accepte","rappel":"promenade"}"#, n);
        assert!(p.en_cours().is_none());
        // perte du lien : null
        p.ligne_board(&etat("reclame", "promenade", "-"), n);
        p.lien(false);
        assert!(p.en_cours().is_none());
    }

    #[test]
    fn rappel_du_remplace_la_promenade() {
        let mut p = pc("sortie,1230,127");
        p.lien(true);
        p.ligne_board(&etat("reclame", "promenade", "-"), dim(12, 29));
        p.tic(dim(12, 30));
        assert_eq!(p.en_cours().unwrap().id, "sortie,1230");
    }

    #[test]
    fn coupure_rappel_garde_et_commande_rejouee() {
        let mut p = pc("sortie,1230,127");
        p.lien(true);
        p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), dim(12, 30));
        p.lien(false);
        assert!(p.en_cours().is_some()); // un rappel survit a la coupure
        assert_eq!(pub_(&p, dim(12, 31))["icone"], "aboie");
        let (_, l) = p.commande(r#"{"cmd":"accepter","rappel":"sortie,1230"}"#, dim(12, 32));
        assert!(l.is_empty());
        assert!(p.en_cours().is_none());
        // retour du lien : la board reclame encore -> commande rejouee, rien de re-adopte
        p.lien(true);
        let l = p.ligne_board(&etat("reclame", "rappel", "sortie,1230"), dim(12, 33));
        assert_eq!(l, vec![r#"{"cmd":"accepter","rappel":"sortie,1230"}"#]);
        assert!(p.en_cours().is_none());
        // la board ne reclame plus : commande abandonnee
        let mut q = pc("sortie,1230,127");
        q.lien(true);
        q.ligne_board(&etat("reclame", "rappel", "sortie,1230"), dim(12, 30));
        q.lien(false);
        q.commande(r#"{"cmd":"reporter","rappel":"sortie,1230"}"#, dim(12, 32));
        q.lien(true);
        assert!(q.ligne_board(&etat("repos", "-", "-"), dim(12, 33)).is_empty());
        q.ligne_board(&etat("reclame", "rappel", "sortie,1230"), dim(12, 34));
        // seule la premiere ligne rejoue
        assert!(q.en_cours().is_some());
    }

    #[test]
    fn rappel_du_hors_ligne_puis_retour() {
        let mut p = pc("dodo,2100,127");
        p.tic(dim(21, 0));
        let v = pub_(&p, dim(21, 0));
        assert_eq!(v["icone"], "aboie");
        assert_eq!(v["board"], "hors_ligne");
        p.commande(r#"{"cmd":"accepter","rappel":"dodo,2100"}"#, dim(21, 1));
        assert_eq!(pub_(&p, dim(21, 1))["icone"], "assis");
        // calme : tic de 22:30 ne declenche rien (et prochain_rappel evite les heures calmes)
        let mut q = pc("dodo,2145,127;sortie,0730,127");
        q.tic(jour_plus(1, 7, 30));
        assert_eq!(q.en_cours().unwrap().id, "sortie,0730");
        assert_eq!(pub_(&q, dim(23, 0))["calme"], true);
    }

    /// SPEC-009 : validation de `{"cmd":"jauge","valeur":N}` et passage a la source.
    #[test]
    fn commande_jauge() {
        use crate::sources::activite::canal_jauge;
        let mut p = pc("");
        let now = dim(10, 0);
        let rep = |p: &mut EtatPc, ligne: &str| -> (Value, Vec<String>) {
            let (r, a_ecrire) = p.commande(ligne, now);
            (serde_json::from_str(&r).unwrap(), a_ecrire)
        };
        // source `activite` non lancee
        let (r, _) = rep(&mut p, r#"{"cmd":"jauge","valeur":42}"#);
        assert_eq!(r, json!({"reponse": "jauge", "ok": false, "erreur": "jauge indisponible"}));

        let (emetteur, mut recepteur) = canal_jauge();
        p.jauge_reglable(Some(emetteur));
        for (ligne, attendu) in [
            (r#"{"cmd":"jauge","valeur":0}"#, 0),
            (r#"{"cmd":"jauge","valeur":100}"#, 100),
            (r#"{"cmd":"jauge","valeur":42}"#, 42),
        ] {
            let (r, a_ecrire) = rep(&mut p, ligne);
            assert_eq!(r, json!({"reponse": "jauge", "ok": true}));
            assert!(a_ecrire.is_empty(), "rien n'est envoye a la board");
            assert_eq!(recepteur.recu(), Some(attendu));
        }
        for ligne in [
            r#"{"cmd":"jauge"}"#,
            r#"{"cmd":"jauge","valeur":42.5}"#,
            r#"{"cmd":"jauge","valeur":"42"}"#,
            r#"{"cmd":"jauge","valeur":null}"#,
            r#"{"cmd":"jauge","valeur":-1}"#,
            r#"{"cmd":"jauge","valeur":101}"#,
        ] {
            let (r, a_ecrire) = rep(&mut p, ligne);
            assert_eq!(
                r,
                json!({"reponse": "jauge", "ok": false, "erreur": "valeur hors de 0..100"}),
                "{ligne}"
            );
            assert!(a_ecrire.is_empty());
            assert_eq!(recepteur.recu(), None, "{ligne}: la jauge ne change pas");
        }
        // lien connecte ou non : meme reponse (le daemon n'a pas besoin de la board)
        p.lien(true);
        let (r, _) = rep(&mut p, r#"{"cmd":"jauge","valeur":7}"#);
        assert_eq!(r["ok"], true);
    }

    /// Menu « Quitter Osampo » : `quitter` repond `ok`, sans rien pour la board, et
    /// seule cette commande demande l arret.
    #[test]
    fn commande_quitter() {
        let mut p = pc("");
        let (r, a_ecrire) = p.commande(r#"{"cmd":"quitter"}"#, dim(10, 0));
        assert_eq!(serde_json::from_str::<Value>(&r).unwrap(), json!({"reponse": "quitter", "ok": true}));
        assert!(a_ecrire.is_empty());
        assert!(demande_l_arret(r#"{"cmd":"quitter"}"#));
        for l in [r#"{"cmd":"os"}"#, r#"{"quitter":1}"#, "quitter", ""] {
            assert!(!demande_l_arret(l), "{l}");
        }
    }
}
