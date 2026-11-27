mod config;
mod config_vivante;
mod emit;
mod manifest;
mod model;
mod pc;
mod scheduler;
mod source;
mod sources;

use anyhow::Result;
use manifest::Manifest;
use model::{Frame, Keys};
use scheduler::Scheduler;
use source::{FakeSource, Source};
use sources::activite::Surcharge;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

/// Capacite du broadcast vers les connexions. Chaque connexion le vide aussitot
/// dans sa propre file : cette capacite n est pas la borne par client.
const CAPACITE_DIFFUSION: usize = emit::socket::FILE_PAR_CONNEXION;

/// Capacite du canal des sources vers la boucle d emission, vide a chaque tic.
const CAPACITE_SOURCES: usize = 64;

struct Options {
    widgets_dir: PathBuf,
    socket: PathBuf,
    scan_ble: bool,
    ble: bool,
    /// Board simulee : socket Unix a la place de BlueZ (SPEC-001).
    faux_ble: Option<PathBuf>,
    unite_s: u64,
    serial: Option<PathBuf>,
    echo_serie: bool,
    /// Fichier de reglages du widget osampo (C2).
    config: PathBuf,
}

const USAGE: &str = "usage: daemon [--widgets-dir <dir>] [--socket <path>] [--scan-ble] \
                     [--ble | --faux-ble <path>] [--unite-s <s>] [--serial <path>] [--echo-serie] \
                     [--config <path>]";

/// Seul le widget osampo a un fichier de reglages et une ligne cfg (C15).
const WIDGET_A_REGLAGES: &str = "osampo";

/// La valeur numerique d une option, ou l usage et un code 2.
fn nombre<T: std::str::FromStr>(option: &str, v: Option<String>) -> T {
    match v.as_deref().map(str::parse::<T>) {
        Some(Ok(n)) => n,
        _ => {
            eprintln!("{option} attend un nombre");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

fn lire_les_options() -> Options {
    let mut widgets_dir = PathBuf::from("widgets");
    let mut socket = emit::socket::socket_path();
    let mut scan_ble = false;
    let mut ble = false;
    let mut faux_ble = None;
    let mut unite_s: u64 = 60;
    let mut serial = None;
    let mut echo_serie = false;
    let mut config = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--widgets-dir" => {
                widgets_dir = PathBuf::from(args.next().expect("--widgets-dir attend un chemin"))
            }
            "--socket" => socket = PathBuf::from(args.next().expect("--socket attend un chemin")),
            "--scan-ble" => scan_ble = true,
            "--ble" => ble = true,
            "--faux-ble" => {
                faux_ble = Some(PathBuf::from(args.next().expect("--faux-ble attend un chemin")))
            }
            "--active-max" => {
                eprintln!("--active-max n existe plus (SP5h) : regler [promenade] vide_apres_min dans osampo.toml");
                std::process::exit(2);
            }
            "--unite-s" => unite_s = nombre("--unite-s", args.next()),
            "--serial" => {
                serial = Some(PathBuf::from(args.next().expect("--serial attend un chemin")))
            }
            "--echo-serie" => echo_serie = true,
            "--config" => {
                config = Some(PathBuf::from(args.next().expect("--config attend un chemin")))
            }
            autre => {
                eprintln!("option inconnue: {autre}");
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        }
    }
    if unite_s == 0 {
        eprintln!("--unite-s doit etre au moins 1");
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    if ble && faux_ble.is_some() {
        eprintln!("--ble et --faux-ble s excluent : le transport est l un ou l autre");
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let config = config.unwrap_or_else(config::chemin_par_defaut);
    Options { widgets_dir, socket, scan_ble, ble, faux_ble, unite_s, serial, echo_serie, config }
}

/// Tous les manifestes sous dir, par ordre alphabetique d identifiant. Un
/// manifeste illisible bloque le demarrage : mieux vaut une erreur ici qu un
/// widget refuse plus tard sans explication.
fn lire_les_manifestes(dir: &Path) -> Result<Vec<Manifest>> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("repertoire des widgets {}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("widget.toml").exists())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    ids.sort();
    if ids.is_empty() {
        anyhow::bail!("aucun widget sous {}", dir.display());
    }
    ids.iter().map(|id| Manifest::load(dir, id)).collect()
}

// ponytail: la cadence d emission est celle du premier widget par ordre alphabetique, un seul
// widget cadence ; plafond : un widget a refresh_ms different recoit des trames a la cadence d un
// autre, et un widget a 5 s pres d un widget a 500 ms est servi trop vite ; chemin : une boucle
// d emission par cadence distincte, chaque connexion abonnee a celle de son manifeste.
//
// ponytail: le planificateur ne connait que les cles du manifeste de cadence ; plafond : un autre
// widget ne peut declarer que des cles de celui-ci, sinon le daemon refuse de demarrer (mieux qu une
// cle annoncee dans l ack et jamais emise) ; chemin : un Scheduler sur l union des manifestes (cles
// et values fusionnees), alimenter filtrant ensuite par connexion.
fn manifeste_de_cadence(dir: &Path) -> Result<Manifest> {
    let tous = lire_les_manifestes(dir)?;
    let premier = tous[0].clone();
    let connues = premier.key_names();
    for m in &tous[1..] {
        if let Some(cle) = m.key_names().into_iter().find(|k| !connues.contains(k)) {
            anyhow::bail!(
                "le manifeste {:?} declare la cle {cle:?}, absente du manifeste de cadence {:?} : \
                 elle ne serait jamais emise (un seul planificateur, sur les cles de {:?})",
                m.id, premier.id, premier.id
            );
        }
        if m.refresh_ms != premier.refresh_ms {
            eprintln!(
                "avertissement: {:?} demande refresh_ms = {}, mais la cadence est celle de {:?} ({} ms)",
                m.id, m.refresh_ms, premier.id, premier.refresh_ms
            );
        }
    }
    Ok(premier)
}

/// D10 : une source est retenue si au moins une de ses cles est demandee par le
/// manifeste de cadence. FakeSource ne sert donc que les manifestes de demo.
fn sources_pour(
    manifeste: &Manifest,
    reglages: watch::Receiver<config::Reglages>,
    surcharge: Surcharge,
    jauge: sources::activite::RecepteurJauge,
) -> Vec<Box<dyn Source>> {
    let demandees = manifeste.key_names();
    let candidates: Vec<Box<dyn Source>> = vec![
        Box::new(FakeSource::new()),
        Box::new(sources::horloge::SourceHorloge),
        Box::new(sources::activite::SourceActivite::new(reglages, surcharge, jauge)),
        Box::new(sources::claude::SourceClaude::new()),
    ];
    candidates
        .into_iter()
        .filter(|s| s.keys().iter().any(|k| demandees.iter().any(|d| d == k)))
        .collect()
}

/// Controle croise des sources contre CHAQUE manifeste, pas seulement celui de
/// cadence : une cle que personne ne produit doit bloquer le demarrage, quel
/// que soit le widget qui la declare.
fn controler_les_sources(dir: &Path, sources: &[Box<dyn Source>]) -> Result<()> {
    for m in lire_les_manifestes(dir)? {
        for avertissement in Scheduler::new(m).check_sources(sources)? {
            eprintln!("avertissement: {avertissement}");
        }
    }
    Ok(())
}

/// Les reglages et la ligne cfg, surveilles si le widget de cadence est
/// osampo (C15). Sinon : les defauts, aucune ligne cfg, aucun fichier touche.
fn preparer_les_reglages(
    manifeste: &Manifest,
    chemin: &Path,
) -> (watch::Receiver<config::Reglages>, watch::Receiver<Option<String>>) {
    if manifeste.id != WIDGET_A_REGLAGES {
        let (_rt, rr) = watch::channel(config::Reglages::default());
        let (_ct, cr) = watch::channel(None);
        return (rr, cr);
    }
    // Fichier present : l empreinte avant la lecture, pour qu une ecriture
    // entre les deux soit relue. Fichier absent : apres sa creation, sinon le
    // modele tout juste ecrit serait relu au premier regard.
    let existait = chemin.exists();
    let avant = existait.then(|| config_vivante::Surveillant::new(chemin.to_path_buf()));
    let reglages = match config::charger_ou_creer(chemin) {
        Ok(r) => {
            let etat = if existait { "lue" } else { "creee" };
            eprintln!("config: {} ({etat})", chemin.display());
            r
        }
        Err(e) => {
            // C5 : un fichier refuse au demarrage donne les defauts.
            eprintln!("config: {} (refusee: {})", chemin.display(), e.trim_end());
            config::Reglages::default()
        }
    };
    let surveillant =
        avant.unwrap_or_else(|| config_vivante::Surveillant::new(chemin.to_path_buf()));
    let ligne = reglages.ligne_cfg();
    let (rt, rr) = watch::channel(reglages);
    let (ct, cr) = watch::channel(Some(ligne));
    config_vivante::surveiller(surveillant, rt, ct);
    (rr, cr)
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = lire_les_options();

    if opts.scan_ble {
        // Exerce le chemin BLE aussi loin qu on peut sans board.
        let vus = emit::ble::scan(std::time::Duration::from_secs(3)).await?;
        println!("scan BLE: {} peripherique(s)", vus.len());
        for n in vus {
            println!("  {n}");
        }
        return Ok(());
    }

    let manifeste = manifeste_de_cadence(&opts.widgets_dir)?;
    let cadence = Duration::from_millis(manifeste.refresh_ms);
    eprintln!("cadence {} ms, d apres le manifeste {:?}", manifeste.refresh_ms, manifeste.id);

    let a_reglages = manifeste.id == WIDGET_A_REGLAGES;
    let (reglages, cfg) = preparer_les_reglages(&manifeste, &opts.config);
    let surcharge = Surcharge { unite_s: opts.unite_s };
    let (vide, courte, longue) = {
        let r = reglages.borrow();
        (r.vide_apres_min, r.courte_min, r.longue_min)
    };
    let reglages_pc = reglages.clone();
    let (jauge_pc, jauge_rx) = sources::activite::canal_jauge();
    let sources = sources_pour(&manifeste, reglages, surcharge, jauge_rx);
    let noms: Vec<&str> = sources.iter().map(|s| s.id()).collect();
    // SPEC-009 : le PC ne regle la jauge que si la source qui la calcule est lancee.
    let jauge_pc = noms.contains(&"activite").then_some(jauge_pc);
    eprintln!(
        "sources: {} (jauge videe en {} min, promenade courte {} min, longue {} min, unite {} s)",
        noms.join(", "),
        vide,
        courte,
        longue,
        opts.unite_s
    );
    controler_les_sources(&opts.widgets_dir, &sources)?;
    let planificateur = Scheduler::new(manifeste);

    let (tx, rx) = broadcast::channel::<Frame>(CAPACITE_DIFFUSION);

    // Lie avant d installer le nettoyage : le fichier supprime a l arret est
    // alors forcement le notre, jamais celui d un autre daemon.
    let listener = emit::socket::bind(&opts.socket)?;
    let dir = opts.widgets_dir.clone();
    // L emetteur serie a son propre abonnement : ni lui ni le socket ne
    // ralentissent l autre, et aucun des deux ne bloque la boucle d emission.
    if let Some(chemin) = opts.serial.clone() {
        tokio::spawn(emit::serial::servir(chemin, tx.subscribe(), cfg.clone(), opts.echo_serie));
    }
    // Bluetooth (SP5d2) : la board osampo, a cote de l USB si les deux sont
    // demandes ; meme abonnement propre, meme ligne cfg.
    // Le lien avec la board (SPEC-001) : BlueZ ou board simulee, l un ou l autre ; la tache
    // « pc » en tient l autre moitie. Sans transport, elle garde la moitie du transport pour
    // que le lien reste « coupe » plutot que « disparu ».
    let (lien, cote) = emit::lien_board::creer();
    let mut garde = None;
    if let Some(chemin) = opts.faux_ble.clone() {
        let (rx_faux, cfg_faux) = (tx.subscribe(), cfg.clone());
        tokio::spawn(async move {
            if let Err(e) = emit::faux_ble::servir(chemin, rx_faux, cfg_faux, cote).await {
                eprintln!("faux-ble: {e:#}");
            }
        });
    } else if opts.ble {
        tokio::spawn(emit::ble::servir_lien("osampo".into(), tx.subscribe(), cfg.clone(), cote));
    } else {
        garde = Some(cote);
    }
    let (etat_tx, etat_rx) = watch::channel(String::new());
    let (cmd_tx, cmd_rx) = mpsc::channel(CAPACITE_COMMANDES_PC);
    let (quitter_tx, quitter_rx) = oneshot::channel();
    tokio::spawn(tache_pc(
        lien,
        tx.subscribe(),
        reglages_pc,
        a_reglages,
        cmd_rx,
        etat_tx,
        garde,
        jauge_pc,
        quitter_tx,
    ));
    let lien_pc = emit::socket::LienPc { etat: etat_rx, commandes: cmd_tx };
    tokio::spawn(emit::socket::serve(listener, dir, rx, cfg, Some(lien_pc)));

    // SIGINT au clavier, SIGTERM pour pkill, commande `quitter` d un client (menu
    // « Quitter Osampo ») : tous suppriment le socket. Sortie en code 0 : l unite systemd
    // (Restart=on-failure) ne relance pas le daemon.
    let mut terme = signal(SignalKind::terminate())?;
    let arret = async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terme.recv() => {}
            Ok(()) = quitter_rx => {}
        }
    };
    emettre(planificateur, sources, cadence, tx, arret).await;

    let _ = std::fs::remove_file(&opts.socket);
    if let Some(f) = &opts.faux_ble {
        let _ = std::fs::remove_file(f);
    }
    eprintln!("arret, socket supprime: {}", opts.socket.display());
    Ok(())
}

/// Capacite de la file des commandes des clients `osampo-pc` vers la tache « pc ».
const CAPACITE_COMMANDES_PC: usize = 16;

/// Premier `await` d un recepteur qui peut avoir perdu son emetteur : une fois ferme,
/// la branche cesse de se declarer prete (sinon la boucle tournerait a vide).
async fn ou_jamais<T>(ouvert: bool, f: impl std::future::Future<Output = T>) -> T {
    if ouvert {
        f.await
    } else {
        std::future::pending().await
    }
}

fn maintenant() -> pc::Heure {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    pc::Heure::locale(s)
}

/// La tache « pc » (SPEC-002, SPEC-003) : relie le lien avec la board, la jauge `sage` lue
/// dans les trames, les reglages et les clients du socket a la logique de `pc::etat`. Un tic
/// d une seconde pour les rappels ; la ligne d etat est republiee des qu elle change.
/// Les rappels du daemon ne servent que si le widget de cadence est osampo (sinon pas de
/// reglages du chiot : aucun rappel, aucun fichier d etat touche, comme pour la ligne cfg).
#[allow(clippy::too_many_arguments)]
async fn tache_pc(
    mut lien: emit::lien_board::LienBoard,
    mut trames: broadcast::Receiver<Frame>,
    mut reglages: watch::Receiver<config::Reglages>,
    a_reglages: bool,
    mut commandes: mpsc::Receiver<emit::socket::CommandePc>,
    publie: watch::Sender<String>,
    _garde: Option<emit::lien_board::CoteTransport>,
    jauge_pc: Option<sources::activite::EmetteurJauge>,
    quitter: oneshot::Sender<()>,
) {
    let mut quitter = Some(quitter);
    let pour_pc = |r: &config::Reglages| {
        let mut r = r.clone();
        if !a_reglages {
            r.rappels = String::new();
        }
        r
    };
    let initiaux = pour_pc(&reglages.borrow());
    let dossier = a_reglages.then(sources::activite::dossier_etat);
    let mut etat = pc::etat::EtatPc::new(pc::rappels::Rappels::new(dossier, &initiaux), &initiaux);
    etat.jauge_reglable(jauge_pc);
    etat.lien(*lien.connecte.borrow());
    let mut tic = tokio::time::interval(Duration::from_secs(1));
    tic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (mut lignes_ok, mut lien_ok, mut trames_ok, mut reglages_ok) = (true, true, true, true);
    publie.send_replace(etat.publier(maintenant()));
    loop {
        let now;
        tokio::select! {
            l = ou_jamais(lignes_ok, lien.lignes.recv()) => {
                now = maintenant();
                match l {
                    Some(l) => {
                        // Un changement de lien en attente passe avant la ligne : la board
                        // ecrit des la connexion, et select! peut servir la ligne d abord
                        // (la commande en attente ne serait alors jamais rejouee).
                        if lien_ok && lien.connecte.has_changed().unwrap_or(false) {
                            etat.lien(*lien.connecte.borrow_and_update());
                        }
                        for c in etat.ligne_board(&l, now) {
                            let _ = lien.commandes.try_send(c);
                        }
                    }
                    None => lignes_ok = false,
                }
            }
            r = ou_jamais(lien_ok, lien.connecte.changed()) => {
                now = maintenant();
                if r.is_err() { lien_ok = false } else { etat.lien(*lien.connecte.borrow_and_update()) }
            }
            t = ou_jamais(trames_ok, trames.recv()) => {
                now = maintenant();
                match t {
                    Ok(Frame(k)) => {
                        if let Some(model::Value::Int(v)) = k.get("sage") {
                            etat.sage(*v);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => trames_ok = false,
                }
            }
            r = ou_jamais(reglages_ok, reglages.changed()) => {
                now = maintenant();
                if r.is_err() {
                    reglages_ok = false;
                } else {
                    let r = pour_pc(&reglages.borrow_and_update());
                    etat.reglages(&r);
                }
            }
            c = commandes.recv() => {
                now = maintenant();
                let Some((ligne, retour)) = c else { return };
                let (reponse, a_ecrire) = etat.commande(&ligne, now);
                for c in a_ecrire {
                    let _ = lien.commandes.try_send(c);
                }
                let _ = retour.send(reponse);
                if pc::etat::demande_l_arret(&ligne) {
                    // ponytail: la reponse part avant l arret sans garantie d ecriture ;
                    // osampo-pc quitte de toute facon.
                    if let Some(q) = quitter.take() {
                        let _ = q.send(());
                    }
                }
            }
            _ = tic.tick() => {
                now = maintenant();
                etat.tic(now);
            }
        }
        let ligne = etat.publier(now);
        publie.send_if_modified(|v| {
            if *v == ligne {
                false
            } else {
                *v = ligne;
                true
            }
        });
    }
}

/// Une tache par source, a son propre rythme. Chaque resultat part avec
/// l instant de sa reception : c est l age reel de la donnee, pas celui du tic
/// qui la fusionne.
fn lancer_les_sources(sources: Vec<Box<dyn Source>>) -> mpsc::Receiver<(Keys, Instant)> {
    let (tx, rx) = mpsc::channel(CAPACITE_SOURCES);
    for mut s in sources {
        let tx = tx.clone();
        let reveil = s.reveil();
        tokio::spawn(async move {
            let mut tic = tokio::time::interval(s.interval());
            // une source lente ne rattrape pas ses tics manques en rafale
            tic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                // Un reveil (reglage de la jauge) poll hors intervalle ; le tic reste
                // ce qu il est (un `notify_one` pose pendant un poll n est pas perdu).
                match &reveil {
                    Some(n) => tokio::select! {
                        _ = tic.tick() => {}
                        _ = n.notified() => {}
                    },
                    None => {
                        tic.tick().await;
                    }
                }
                match s.poll().await {
                    Ok(k) => {
                        if tx.send((k, Instant::now())).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => eprintln!("source {:?} en echec: {e}", s.id()),
                }
            }
        });
    }
    rx
}

/// La boucle d emission. Le tic ne fait que fusionner ce que les sources ont
/// deja rendu, puis emettre : il n attend jamais une source, si lente soit-elle.
async fn emettre(
    mut planificateur: Scheduler,
    sources: Vec<Box<dyn Source>>,
    cadence: Duration,
    tx: broadcast::Sender<Frame>,
    arret: impl std::future::Future<Output = ()>,
) {
    let mut recues = lancer_les_sources(sources);
    tokio::pin!(arret);
    // Decale d une demi-cadence : les sources demarrent leur intervalle au
    // meme instant, et une source de meme periode que le tic (FakeSource et
    // demo3, 500 ms) arriverait a la course avec lui, trame doublee puis
    // sautee. Le tic tombe ainsi entre deux resultats.
    let mut tic = tokio::time::interval_at(tokio::time::Instant::now() + cadence / 2, cadence);
    // pas de rafale apres une mise en veille : des milliers de trames d un coup feraient
    // depasser 64 pertes a un client sain, coupe alors comme trop lent
    tic.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = &mut arret => return,
            _ = tic.tick() => {}
        }
        while let Ok((k, instant)) = recues.try_recv() {
            planificateur.merge(k, instant);
        }
        if let Some(frame) = planificateur.snapshot(Instant::now()) {
            // send ne rend Err que sans aucun recepteur ; le rx d origine vit dans serve, donc jamais.
            let _ = tx.send(frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::KeySpec;
    use crate::model::Value;
    use std::sync::Arc;

    /// Une source qui numerote ses trames, a l intervalle donne.
    struct Rapide(u64, Duration);

    #[async_trait::async_trait]
    impl Source for Rapide {
        fn id(&self) -> &str { "rapide" }
        fn keys(&self) -> &'static [&'static str] { &["value", "label"] }
        fn interval(&self) -> Duration { self.1 }
        async fn poll(&mut self) -> Result<Keys> {
            self.0 += 1;
            let mut k = Keys::new();
            k.insert("value".into(), Value::Float((self.0 % 100) as f64));
            k.insert("label".into(), Value::Text(format!("trame {}", self.0)));
            Ok(k)
        }
    }

    /// Repond une fois, puis chaque poll dure une heure : un appel reseau pendu.
    struct Pendue(bool);

    #[async_trait::async_trait]
    impl Source for Pendue {
        fn id(&self) -> &str { "pendue" }
        fn keys(&self) -> &'static [&'static str] { &["lent"] }
        fn interval(&self) -> Duration { Duration::from_millis(10) }
        async fn poll(&mut self) -> Result<Keys> {
            if self.0 {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            self.0 = true;
            let mut k = Keys::new();
            k.insert("lent".into(), Value::Text("une fois".into()));
            Ok(k)
        }
    }

    /// Intervalle d'une heure, mais reveillable (SPEC-009).
    struct Reveillable(u64, Arc<tokio::sync::Notify>);

    #[async_trait::async_trait]
    impl Source for Reveillable {
        fn id(&self) -> &str { "reveillable" }
        fn keys(&self) -> &'static [&'static str] { &["n"] }
        fn interval(&self) -> Duration { Duration::from_secs(3600) }
        fn reveil(&self) -> Option<Arc<tokio::sync::Notify>> { Some(self.1.clone()) }
        async fn poll(&mut self) -> Result<Keys> {
            self.0 += 1;
            let mut k = Keys::new();
            k.insert("n".into(), Value::Int(self.0 as i64));
            Ok(k)
        }
    }

    #[tokio::test]
    async fn un_reveil_fait_poller_hors_intervalle() {
        let n = Arc::new(tokio::sync::Notify::new());
        let mut rx = lancer_les_sources(vec![Box::new(Reveillable(0, n.clone()))]);
        let un = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
        assert_eq!(un.0.get("n"), Some(&Value::Int(1)));
        n.notify_one();
        let deux = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap();
        assert_eq!(deux.0.get("n"), Some(&Value::Int(2)));
    }

    fn cle(nom: &str, ty: &str) -> KeySpec {
        KeySpec { name: nom.into(), ty: ty.into(), values: vec![] }
    }

    fn pour(m: &Manifest) -> Vec<Box<dyn Source>> {
        let (_t, r) = watch::channel(config::Reglages::default());
        sources_pour(m, r, Surcharge { unite_s: 60 }, sources::activite::canal_jauge().1)
    }

    fn ids(sources: &[Box<dyn Source>]) -> Vec<String> {
        let mut v: Vec<String> = sources.iter().map(|s| s.id().to_string()).collect();
        v.sort();
        v
    }

    #[test]
    fn le_manifeste_osampo_prend_les_trois_sources_reelles() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../widgets");
        let m = Manifest::load(&dir, "osampo").unwrap();
        assert_eq!(ids(&pour(&m)), vec!["activite", "claude", "horloge"]);
    }

    #[test]
    fn un_manifeste_de_demo_prend_la_fausse_source() {
        let m = manifeste(vec![cle("value", "float"), cle("label", "string"), cle("state", "string")]);
        assert_eq!(ids(&pour(&m)), vec!["fake"]);
        let m = manifeste(vec![cle("value", "float")]);
        assert_eq!(ids(&pour(&m)), vec!["fake"]);
    }

    #[test]
    fn une_seule_cle_suffit_a_retenir_sa_source() {
        let m = manifeste(vec![cle("tz_s", "int")]);
        assert_eq!(ids(&pour(&m)), vec!["horloge"]);
        let m = manifeste(vec![cle("sage", "int")]);
        assert_eq!(ids(&pour(&m)), vec!["activite"]);
    }

    fn manifeste(cles: Vec<KeySpec>) -> Manifest {
        Manifest {
            id: "demo3".into(),
            formats: vec!["bar".into()],
            refresh_ms: 20,
            stale_s: 30,
            keys: cles,
        }
    }

    /// Fait tourner la boucle d emission pendant duree, rend les labels emis.
    async fn labels_emis(
        m: Manifest,
        sources: Vec<Box<dyn Source>>,
        cadence: Duration,
        duree: Duration,
    ) -> Vec<String> {
        let (tx, mut rx) = broadcast::channel::<Frame>(64);
        let emission = tokio::spawn(emettre(
            Scheduler::new(m),
            sources,
            cadence,
            tx,
            tokio::time::sleep(duree),
        ));
        let mut labels = Vec::new();
        let fin = tokio::time::Instant::now() + duree;
        while let Ok(Ok(f)) = tokio::time::timeout_at(fin, rx.recv()).await {
            if let Some(Value::Text(l)) = f.0.get("label") {
                labels.push(l.clone());
            }
        }
        emission.abort();
        labels
    }

    // multi_thread comme le daemon : la course entre la tache de la source et
    // le tic n existe pas sur un executeur a un seul fil.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn une_source_au_rythme_du_tic_change_la_trame_a_chaque_tic() {
        // Meme periode des deux cotes, comme FakeSource (500 ms) et demo3
        // (refresh_ms = 500). En phase, chaque resultat arrive a la course
        // avec le tic : trames doublees puis sautees. Le tic doit tomber
        // entre deux resultats.
        let periode = Duration::from_millis(100);
        let sources: Vec<Box<dyn Source>> = vec![Box::new(Rapide(0, periode))];
        let labels = labels_emis(
            manifeste(vec![cle("value", "float"), cle("label", "string")]),
            sources,
            periode,
            Duration::from_millis(2000),
        )
        .await;
        let doublons = labels.windows(2).filter(|w| w[0] == w[1]).count();
        assert!(labels.len() >= 15, "trop peu de trames: {labels:?}");
        assert!(doublons <= 1, "{doublons} trames identiques d affilee: {labels:?}");
    }

    #[tokio::test]
    async fn une_source_pendue_ne_retarde_pas_le_tic() {
        let m = manifeste(vec![cle("value", "float"), cle("label", "string"), cle("lent", "string")]);
        let sources: Vec<Box<dyn Source>> =
            vec![Box::new(Pendue(false)), Box::new(Rapide(0, Duration::from_millis(10)))];
        let mut labels =
            labels_emis(m, sources, Duration::from_millis(20), Duration::from_millis(1000)).await;
        labels.dedup();
        // 1 s a 20 ms : 50 tics. La source rapide doit faire avancer la trame
        // a chaque tic, quoi que fasse la source pendue.
        assert!(labels.len() >= 30, "seulement {} trames distinctes en 1 s: {labels:?}", labels.len());
    }
}
