//! Fenêtre permanente du chien posé sur la barre des tâches (SPEC-011, SPEC-006).
//!
//! `Fenetre` n'est qu'une poignée : la boucle X tourne dans son propre fil et reçoit ses
//! ordres par un canal, elle ne bloque jamais la boucle D-Bus / socket de `main`. Ce qui sort
//! de la fenêtre (clic gauche, entrée du menu du chien) repart vers `main` sous forme d'`Action`.
//! `Machine` (pure, testée sans serveur X) décide de l'animation à chaque instant ; `X` la
//! dessine et traite la souris.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use x11rb::connection::Connection;
use x11rb::properties::{WmHints, WmHintsState, WmSizeHints, WmSizeHintsSpecification};
use x11rb::protocol::xproto::*;
use x11rb::protocol::{randr, shape, Event};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::menu::{self, Action, Entree};
use crate::menu_chien::{self, Menu};
use crate::position;
use crate::sprites::{mettre_a_l_echelle, pixels_x, Anim, Sprites};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Après `Chatouiller`.
    Content,
    /// Après `Donner un os`.
    Mange,
    /// Après `Donner un bain`.
    Bain,
}

/// L'état du chiot vu par la fenêtre, d'après l'`icone` de la ligne d'état (SPEC-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Humeur {
    /// `assis`, daemon injoignable, aucune ligne reçue.
    Repos,
    /// `niche` : il dort.
    Niche,
    /// `laisse` : en promenade, la laisse seule.
    Laisse,
    /// `aboie` : réclamation de sortie ou de la board.
    Aboie,
    /// `patte` : la board réclame parce que Claude attend une réponse (ADR-004).
    Patte,
    /// `reclame_laisse` : la board réclame la laisse dans la gueule (ADR-006).
    ReclameLaisse,
}

impl Humeur {
    pub fn de_icone(icone: &str) -> Self {
        match icone {
            "aboie" => Humeur::Aboie,
            "patte" => Humeur::Patte,
            "reclame_laisse" => Humeur::ReclameLaisse,
            "niche" => Humeur::Niche,
            "laisse" => Humeur::Laisse,
            _ => Humeur::Repos,
        }
    }
}

/// Durée d'une case d'animation (même cadence que le widget : `Theme.frame`).
const CASE_MS: Duration = Duration::from_millis(150);
/// Pas de la boucle : attente d'un ordre, puis les événements X (environ 30 fois par seconde).
const PAS: Duration = Duration::from_millis(33);
/// Délai avant de réessayer d'ouvrir le serveur X.
const NOUVEL_ESSAI: Duration = Duration::from_secs(2);
/// Déplacement à partir duquel un appui suivi d'un relâchement est un glisser-déposer.
const SEUIL_GLISSE: i32 = 4;
/// Alpha à partir duquel un pixel fait partie de la découpe Shape.
const ALPHA_DECOUPE: u8 = 128;
/// Fond uni quand rien ne compose et que Shape manque (fond de la bulle du widget).
const FOND_UNI: [u8; 3] = [253, 253, 246];
const TOUTES: [Anim; 12] = [
    Anim::Repos,
    Anim::Dort,
    Anim::Aboie,
    Anim::Patte,
    Anim::Content,
    Anim::Os,
    Anim::OsFin,
    Anim::Laisse,
    Anim::ReclameLaisse,
    Anim::Bain,
    Anim::BainMousse,
    Anim::BainFin,
];

// --- ce qui se voit -----------------------------------------------------------------------

/// Ce qu'il faut montrer à un instant : l'animation et sa case.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vue {
    pub anim: Anim,
    pub case: usize,
}

/// Les animations dans l'ordre de SPEC-011 (la réaction `os` enchaîne `os` puis `os_fin`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Famille {
    Aboie,
    Patte,
    ReclameLaisse,
    Reaction(Reaction),
    Dort,
    Laisse,
    Repos,
}

/// Les animations jouées par une famille, dans l'ordre.
fn suite(f: Famille) -> &'static [Anim] {
    match f {
        Famille::Aboie => &[Anim::Aboie],
        Famille::Patte => &[Anim::Patte],
        Famille::ReclameLaisse => &[Anim::ReclameLaisse],
        Famille::Reaction(Reaction::Content) => &[Anim::Content],
        Famille::Reaction(Reaction::Mange) => &[Anim::Os, Anim::OsFin],
        // ponytail: mousse en boucle 3 fois (6 sur la board), un bain plus court sur le PC
        Famille::Reaction(Reaction::Bain) => {
            &[Anim::Bain, Anim::BainMousse, Anim::BainMousse, Anim::BainMousse, Anim::BainFin]
        }
        Famille::Dort => &[Anim::Dort],
        Famille::Laisse => &[Anim::Laisse],
        Famille::Repos => &[Anim::Repos],
    }
}

pub struct Machine {
    /// Nombre de cases de chaque animation.
    compte: fn(Anim) -> usize,
    humeur: Humeur,
    reaction: Option<Reaction>,
    famille: Famille,
    /// Début de l'animation en cours.
    t_anim: Duration,
}

impl Machine {
    pub fn new(compte: fn(Anim) -> usize) -> Self {
        Machine {
            compte,
            humeur: Humeur::Repos,
            reaction: None,
            famille: Famille::Repos,
            t_anim: Duration::ZERO,
        }
    }

    /// La première animation qui s'applique : `aboie`, `patte` ou `reclame_laisse` > réaction > `dort` > `laisse` > `repos`.
    fn choisie(&self) -> Famille {
        if self.humeur == Humeur::Aboie {
            Famille::Aboie
        } else if self.humeur == Humeur::Patte {
            Famille::Patte
        } else if self.humeur == Humeur::ReclameLaisse {
            Famille::ReclameLaisse
        } else if let Some(r) = self.reaction {
            Famille::Reaction(r)
        } else {
            match self.humeur {
                Humeur::Niche => Famille::Dort,
                Humeur::Laisse => Famille::Laisse,
                _ => Famille::Repos,
            }
        }
    }

    /// Repart de la première case quand l'animation change.
    fn suivre(&mut self, now: Duration) {
        let f = self.choisie();
        if f != self.famille {
            self.famille = f;
            self.t_anim = now;
        }
    }

    /// Nouvel état du chiot ; `aboie`, `patte` et `reclame_laisse` abandonnent une réaction
    /// en cours.
    pub fn humeur(&mut self, h: Humeur, now: Duration) {
        self.humeur = h;
        if matches!(h, Humeur::Aboie | Humeur::Patte | Humeur::ReclameLaisse) {
            self.reaction = None;
        }
        self.suivre(now);
    }

    /// Joue une fois la réaction, puis revient à l'animation de l'état ; sans effet pendant
    /// `aboie`, `patte` ou `reclame_laisse`.
    pub fn reaction(&mut self, r: Reaction, now: Duration) {
        if matches!(self.humeur, Humeur::Aboie | Humeur::Patte | Humeur::ReclameLaisse) {
            return;
        }
        self.reaction = Some(r);
        self.famille = Famille::Reaction(r);
        self.t_anim = now;
    }

    /// Dit ce qu'il faut montrer ; met fin à une réaction dont toutes les cases ont été jouées.
    pub fn vue(&mut self, now: Duration) -> Vue {
        if let Famille::Reaction(_) = self.famille {
            if now.saturating_sub(self.t_anim) >= duree(self.famille, self.compte) {
                self.reaction = None;
                self.suivre(now);
            }
        }
        let (anim, case) = case_a(self.famille, now.saturating_sub(self.t_anim), self.compte);
        Vue { anim, case }
    }
}

/// Durée totale d'une suite (utile pour les réactions, jouées une fois).
fn duree(f: Famille, compte: fn(Anim) -> usize) -> Duration {
    suite(f).iter().map(|&a| CASE_MS * compte(a) as u32).sum()
}

/// L'animation et la case à montrer `ecoule` après le début ; les familles qui ne sont pas des
/// réactions bouclent.
fn case_a(f: Famille, ecoule: Duration, compte: fn(Anim) -> usize) -> (Anim, usize) {
    let mut n = (ecoule.as_millis() / CASE_MS.as_millis()) as usize;
    let anims = suite(f);
    if !matches!(f, Famille::Reaction(_)) {
        let a = anims[0];
        return (a, n % compte(a).max(1));
    }
    for &a in anims {
        if n < compte(a) {
            return (a, n);
        }
        n -= compte(a);
    }
    let dernier = *anims.last().expect("une suite n'est jamais vide");
    (dernier, compte(dernier).saturating_sub(1))
}

// --- la zone et la pose ---------------------------------------------------------------------

/// Zone de pose : `_NET_WORKAREA` recoupée avec le moniteur principal de RandR ; à défaut, ce
/// qui reste de l'un ou de l'autre, puis l'écran entier. Rectangles `(x, y, largeur, hauteur)`.
fn zone_principale(
    travail: Option<[i32; 4]>,
    moniteur: Option<[i32; 4]>,
    ecran: (i32, i32),
) -> [i32; 4] {
    let base = travail.unwrap_or([0, 0, ecran.0, ecran.1]);
    let Some(m) = moniteur else { return base };
    let x0 = base[0].max(m[0]);
    let y0 = base[1].max(m[1]);
    let x1 = (base[0] + base[2]).min(m[0] + m[2]);
    let y1 = (base[1] + base[3]).min(m[1] + m[3]);
    if x1 > x0 && y1 > y0 {
        [x0, y0, x1 - x0, y1 - y0]
    } else {
        base
    }
}

/// Coin haut gauche de la fenêtre : bas de la fenêtre sur le bas de la zone, `droite_px`
/// (ramenée dans la zone) entre le bord droit de la fenêtre et celui de la zone.
fn pose(zone: [i32; 4], droite_px: i64, cote: u16) -> (i32, i32) {
    let c = i64::from(cote);
    (position::x_fenetre(zone, droite_px, c) as i32, zone[1] + zone[3] - i32::from(cote))
}

/// Un appui de la souris : clic ou glisser-déposer selon le déplacement.
#[derive(Debug, Clone, Copy)]
struct Appui {
    x0: i32,
    y0: i32,
    /// `x` de la fenêtre à l'appui.
    fenetre_x0: i32,
    glisse: bool,
}

impl Appui {
    /// Le pointeur est en `(x, y)` (coordonnées racine) ; `true` dès que c'est un glisser,
    /// et ça le reste jusqu'au relâchement.
    fn bouge(&mut self, x: i32, y: i32) -> bool {
        let (dx, dy) = (x - self.x0, y - self.y0);
        if dx * dx + dy * dy >= SEUIL_GLISSE * SEUIL_GLISSE {
            self.glisse = true;
        }
        self.glisse
    }

    /// `x` que doit avoir la fenêtre quand le pointeur est en `x`.
    fn fenetre_x(&self, x: i32) -> i32 {
        self.fenetre_x0 + x - self.x0
    }
}

/// Rectangles (une rangée, une suite de pixels non transparents) de la découpe Shape.
pub(crate) fn rectangles_opaques(rgba: &[u8], cote: usize) -> Vec<Rectangle> {
    let mut out = Vec::new();
    for y in 0..cote {
        let ligne = &rgba[y * cote * 4..(y + 1) * cote * 4];
        let mut debut = None;
        for x in 0..=cote {
            let opaque = x < cote && ligne[x * 4 + 3] >= ALPHA_DECOUPE;
            match (opaque, debut) {
                (true, None) => debut = Some(x),
                (false, Some(d)) => {
                    out.push(Rectangle { x: d as i16, y: y as i16, width: (x - d) as u16, height: 1 });
                    debut = None;
                }
                _ => {}
            }
        }
    }
    out
}

// --- le serveur X ---------------------------------------------------------------------------

pub(crate) struct Atomes {
    pub(crate) wm_state: Atom,
    pub(crate) skip_taskbar: Atom,
    pub(crate) skip_pager: Atom,
    pub(crate) above: Atom,
    pub(crate) motif: Atom,
    pub(crate) net_wm_name: Atom,
    pub(crate) utf8: Atom,
    pub(crate) user_time: Atom,
    pub(crate) workarea: Atom,
    pub(crate) desktop: Atom,
    pub(crate) anim: Atom,
}

/// Comment les pixels transparents laissent voir le bureau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Rendu {
    /// Visuel ARGB : un gestionnaire de fenêtres compose.
    Argb,
    /// Découpe par l'extension Shape.
    Shape,
    /// Ni l'un ni l'autre : fond uni.
    Uni,
}

pub(crate) type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Ce qui survit à une reconnexion au serveur X.
struct Fil {
    sprites: Sprites,
    machine: Machine,
    entrees: Vec<Entree>,
    cote: u16,
    droite_px: i64,
    actions: UnboundedSender<Action>,
}

/// Comment dessiner : profondeur, visuel et colormap communs aux fenêtres du chien.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Visuel {
    pub(crate) profondeur: u8,
    pub(crate) visuel: Visualid,
    pub(crate) colormap: Option<Colormap>,
    pub(crate) rendu: Rendu,
}

/// ARGB si un gestionnaire de fenêtres compose, sinon Shape, à défaut fond uni.
pub(crate) fn choisir_visuel(conn: &RustConnection, ecran: usize, racine: Window) -> Res<Visuel> {
    let ecran_x = &conn.setup().roots[ecran];
    let composite = {
        let cm = conn.intern_atom(false, format!("_NET_WM_CM_S{ecran}").as_bytes())?.reply()?.atom;
        conn.get_selection_owner(cm)?.reply()?.owner != x11rb::NONE
    };
    let argb = composite
        .then(|| {
            ecran_x
                .allowed_depths
                .iter()
                .find(|d| d.depth == 32)
                .and_then(|d| d.visuals.iter().find(|v| v.class == VisualClass::TRUE_COLOR))
                .map(|v| v.visual_id)
        })
        .flatten();
    Ok(match argb {
        Some(v) => {
            let cm = conn.generate_id()?;
            conn.create_colormap(ColormapAlloc::NONE, cm, racine, v)?;
            Visuel { profondeur: 32, visuel: v, colormap: Some(cm), rendu: Rendu::Argb }
        }
        None => {
            let shape_ok = shape::query_version(conn).ok().is_some_and(|c| c.reply().is_ok());
            let rendu = if shape_ok { Rendu::Shape } else { Rendu::Uni };
            Visuel { profondeur: ecran_x.root_depth, visuel: ecran_x.root_visual, colormap: None, rendu }
        }
    })
}

/// Pixel de fond uni (rien ne compose et Shape manque).
fn pixel_fond() -> u32 {
    (u32::from(FOND_UNI[0]) << 16) | (u32::from(FOND_UNI[1]) << 8) | u32::from(FOND_UNI[2])
}

/// Crée une fenêtre sans bord du chien (non mappée) avec le visuel donné.
pub(crate) fn creer_fenetre(
    conn: &RustConnection,
    racine: Window,
    vis: Visuel,
    (x, y): (i32, i32),
    cote: u16,
    masque: EventMask,
) -> Res<(Window, Gcontext)> {
    let fenetre = conn.generate_id()?;
    let gc = conn.generate_id()?;
    let mut aux = CreateWindowAux::new()
        .background_pixel(if vis.colormap.is_some() { 0 } else { pixel_fond() })
        .border_pixel(0)
        .event_mask(masque);
    if let Some(cm) = vis.colormap {
        aux = aux.colormap(cm);
    }
    conn.create_window(
        vis.profondeur,
        fenetre,
        racine,
        x as i16,
        y as i16,
        cote,
        cote,
        0,
        WindowClass::INPUT_OUTPUT,
        vis.visuel,
        &aux,
    )?;
    conn.create_gc(gc, fenetre, &CreateGCAux::new())?;
    Ok((fenetre, gc))
}

/// Propriétés communes aux fenêtres du chien : identité, sans bord, au premier plan hors
/// barre des tâches, sur tous les espaces, sans focus clavier, taille fixe, `_OSAMPO_ANIM`.
pub(crate) fn proprietes(
    conn: &RustConnection,
    fenetre: Window,
    a: &Atomes,
    classe: &[u8],
    cote: u16,
    pos: (i32, i32),
    anim: &str,
) -> Res<()> {
    conn.change_property8(PropMode::REPLACE, fenetre, AtomEnum::WM_NAME, AtomEnum::STRING, b"Osampo")?;
    conn.change_property8(PropMode::REPLACE, fenetre, a.net_wm_name, a.utf8, b"Osampo")?;
    conn.change_property8(PropMode::REPLACE, fenetre, AtomEnum::WM_CLASS, AtomEnum::STRING, classe)?;
    // sans bord : décorations à 0 (flags = MWM_HINTS_DECORATIONS)
    conn.change_property32(PropMode::REPLACE, fenetre, a.motif, a.motif, &[2, 0, 0, 0, 0])?;
    // hors barre des tâches, hors sélecteur de fenêtres, au premier plan
    conn.change_property32(
        PropMode::REPLACE,
        fenetre,
        a.wm_state,
        AtomEnum::ATOM,
        &[a.skip_taskbar, a.skip_pager, a.above],
    )?;
    // sur tous les espaces de travail
    conn.change_property32(PropMode::REPLACE, fenetre, a.desktop, AtomEnum::CARDINAL, &[0xFFFF_FFFF])?;
    // sans focus clavier : `input` à faux, pas de WM_TAKE_FOCUS (WM_PROTOCOLS absent), et
    // aucune date d'interaction
    let mut hints = WmHints::new();
    hints.input = Some(false);
    hints.initial_state = Some(WmHintsState::Normal);
    hints.set(conn, fenetre)?;
    conn.change_property32(PropMode::REPLACE, fenetre, a.user_time, AtomEnum::CARDINAL, &[0])?;
    taille_fixe(conn, fenetre, cote, pos)?;
    conn.change_property8(PropMode::REPLACE, fenetre, a.anim, a.utf8, anim.as_bytes())?;
    Ok(())
}

/// Peint l'image `rgba` (carrée, `cote` px) : découpe Shape au besoin, fond uni sans ARGB.
pub(crate) fn peindre(
    conn: &RustConnection,
    fenetre: Window,
    gc: Gcontext,
    vis: Visuel,
    msb: bool,
    rgba: &[u8],
    cote: u16,
) -> Res<()> {
    let c = usize::from(cote);
    if vis.rendu == Rendu::Shape {
        shape::rectangles(
            conn,
            shape::SO::SET,
            shape::SK::BOUNDING,
            ClipOrdering::UNSORTED,
            fenetre,
            0,
            0,
            &rectangles_opaques(rgba, c),
        )?;
    }
    let fond = (vis.rendu != Rendu::Argb).then_some(FOND_UNI);
    let pixels = pixels_x(rgba, fond, msb);
    conn.put_image(ImageFormat::Z_PIXMAP, fenetre, gc, cote, cote, 0, 0, 0, vis.profondeur, &pixels)?;
    conn.flush()?;
    Ok(())
}

/// La connexion et la fenêtre ouverte.
struct X {
    conn: RustConnection,
    racine: Window,
    ecran: usize,
    atomes: Atomes,
    msb: bool,
    fenetre: Window,
    gc: Gcontext,
    vis: Visuel,
    zone: [i32; 4],
    /// Position et côté actuels de la fenêtre.
    place: (i32, i32),
    taille: u16,
    /// Dernière image dessinée (`None` : à redessiner) et nom d'animation publié.
    derniere: Option<(Anim, usize)>,
    nom: &'static str,
    appui: Option<Appui>,
    droit: bool,
    menu: Option<Menu>,
}

impl X {
    /// Crée et affiche la fenêtre, avec toutes ses propriétés.
    fn ouvrir(fil: &Fil) -> Res<Self> {
        let (conn, ecran) = RustConnection::connect(None)?;
        let racine = conn.setup().roots[ecran].root;
        let atome = |nom: &str| -> Res<Atom> { Ok(conn.intern_atom(false, nom.as_bytes())?.reply()?.atom) };
        let atomes = Atomes {
            wm_state: atome("_NET_WM_STATE")?,
            skip_taskbar: atome("_NET_WM_STATE_SKIP_TASKBAR")?,
            skip_pager: atome("_NET_WM_STATE_SKIP_PAGER")?,
            above: atome("_NET_WM_STATE_ABOVE")?,
            motif: atome("_MOTIF_WM_HINTS")?,
            net_wm_name: atome("_NET_WM_NAME")?,
            utf8: atome("UTF8_STRING")?,
            user_time: atome("_NET_WM_USER_TIME")?,
            workarea: atome("_NET_WORKAREA")?,
            desktop: atome("_NET_WM_DESKTOP")?,
            anim: atome("_OSAMPO_ANIM")?,
        };
        let msb = conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let a = &atomes;

        // la zone change : on veut le savoir (tableau de bord, résolution)
        conn.change_window_attributes(
            racine,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let _ = randr::query_version(&conn, 1, 5).map(|c| c.reply());
        let _ = randr::select_input(
            &conn,
            racine,
            randr::NotifyMask::SCREEN_CHANGE | randr::NotifyMask::CRTC_CHANGE | randr::NotifyMask::OUTPUT_CHANGE,
        );

        let zone = zone_de(&conn, racine, ecran, a.workarea)?;
        let (x, y) = pose(zone, fil.droite_px, fil.cote);
        let vis = choisir_visuel(&conn, ecran, racine)?;
        let (fenetre, gc) = creer_fenetre(
            &conn,
            racine,
            vis,
            (x, y),
            fil.cote,
            EventMask::EXPOSURE | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::BUTTON_MOTION,
        )?;
        proprietes(&conn, fenetre, a, b"osampo-pc\0Osampo\0", fil.cote, (x, y), Anim::Repos.nom())?;

        conn.map_window(fenetre)?;
        conn.flush()?;
        Ok(X {
            conn,
            racine,
            ecran,
            atomes,
            msb,
            fenetre,
            gc,
            vis,
            zone,
            place: (x, y),
            taille: fil.cote,
            derniere: None,
            nom: Anim::Repos.nom(),
            appui: None,
            droit: false,
            menu: None,
        })
    }

    fn relire_zone(&mut self) -> Res<()> {
        self.zone = zone_de(&self.conn, self.racine, self.ecran, self.atomes.workarea)?;
        Ok(())
    }

    /// Pose la fenêtre où elle doit être (zone, `droite_px`, côté) ; la redessine si le côté change.
    fn poser(&mut self, fil: &Fil) -> Res<()> {
        let (x, y) = pose(self.zone, fil.droite_px, fil.cote);
        if (x, y) == self.place && fil.cote == self.taille {
            return Ok(());
        }
        let mut aux = ConfigureWindowAux::new().x(x).y(y);
        if fil.cote != self.taille {
            aux = aux.width(u32::from(fil.cote)).height(u32::from(fil.cote));
            taille_fixe(&self.conn, self.fenetre, fil.cote, (x, y))?;
            self.derniere = None;
        }
        self.conn.configure_window(self.fenetre, &aux)?;
        self.conn.flush()?;
        self.place = (x, y);
        self.taille = fil.cote;
        Ok(())
    }

    /// Dessine l'image si elle a changé ; publie `_OSAMPO_ANIM` à chaque changement d'animation.
    fn montrer(&mut self, fil: &Fil, vue: Vue) -> Res<()> {
        let nom = vue.anim.nom();
        if nom != self.nom {
            self.conn
                .change_property8(PropMode::REPLACE, self.fenetre, self.atomes.anim, self.atomes.utf8, nom.as_bytes())?;
            self.nom = nom;
        }
        if self.derniere == Some((vue.anim, vue.case)) {
            return Ok(());
        }
        let rgba = mettre_a_l_echelle(fil.sprites.case(vue.anim, vue.case), usize::from(self.taille));
        peindre(&self.conn, self.fenetre, self.gc, self.vis, self.msb, &rgba, self.taille)?;
        self.derniere = Some((vue.anim, vue.case));
        Ok(())
    }

    /// Vide les événements X (souris, exposition, changement de zone).
    fn evenements(&mut self, fil: &mut Fil) -> Res<()> {
        while let Some(ev) = self.conn.poll_for_event()? {
            self.evenement(fil, ev)?;
        }
        Ok(())
    }

    fn evenement(&mut self, fil: &mut Fil, ev: Event) -> Res<()> {
        // le menu du chien saisit le pointeur : c'est lui qui reçoit les clics tant qu'il est ouvert
        if let Some(m) = &self.menu {
            if let Some(choix) = m.evenement(&self.conn, &ev) {
                self.menu = None;
                if let Some(action) = choix {
                    let _ = fil.actions.send(action);
                }
                return Ok(());
            }
        }
        match ev {
            Event::Expose(e) if e.window == self.fenetre => self.derniere = None,
            Event::PropertyNotify(e) if e.atom == self.atomes.workarea => self.relire_zone()?,
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => self.relire_zone()?,
            Event::ButtonPress(b) if b.event == self.fenetre => match b.detail {
                1 => {
                    self.appui = Some(Appui {
                        x0: i32::from(b.root_x),
                        y0: i32::from(b.root_y),
                        fenetre_x0: self.place.0,
                        glisse: false,
                    })
                }
                3 => self.droit = true,
                _ => {}
            },
            Event::MotionNotify(m) if m.event == self.fenetre => {
                if let Some(a) = &mut self.appui {
                    if a.bouge(i32::from(m.root_x), i32::from(m.root_y)) {
                        let x = a.fenetre_x(i32::from(m.root_x));
                        fil.droite_px = position::droite_depuis_x(self.zone, i64::from(x), i64::from(fil.cote));
                        self.poser(fil)?;
                    }
                }
            }
            Event::ButtonRelease(b) if b.event == self.fenetre => match b.detail {
                1 => {
                    if let Some(mut a) = self.appui.take() {
                        if a.bouge(i32::from(b.root_x), i32::from(b.root_y)) {
                            if let Err(e) = position::ecrire(fil.droite_px) {
                                eprintln!("position du chien non enregistrée : {e}");
                            }
                        } else {
                            let _ = fil.actions.send(Action::Chatouille);
                        }
                    }
                }
                3 if self.droit => {
                    self.droit = false;
                    if self.menu.is_none() {
                        let pointeur = (i32::from(b.root_x), i32::from(b.root_y));
                        match menu_chien::ouvrir(&self.conn, self.ecran, pointeur, fil.entrees.clone()) {
                            Ok(m) => self.menu = Some(m),
                            Err(e) => eprintln!("menu du chien : {e}"),
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        Ok(())
    }
}

fn zone_de(conn: &RustConnection, racine: Window, ecran: usize, workarea: Atom) -> Res<[i32; 4]> {
    let travail = conn
        .get_property(false, racine, workarea, AtomEnum::CARDINAL, 0, 4)?
        .reply()?
        .value32()
        .and_then(|mut v| Some([v.next()? as i32, v.next()? as i32, v.next()? as i32, v.next()? as i32]));
    let e = &conn.setup().roots[ecran];
    let moniteur = moniteur_principal(conn, racine).ok().flatten();
    Ok(zone_principale(travail, moniteur, (i32::from(e.width_in_pixels), i32::from(e.height_in_pixels))))
}

fn moniteur_principal(conn: &RustConnection, racine: Window) -> Res<Option<[i32; 4]>> {
    let sortie = randr::get_output_primary(conn, racine)?.reply()?.output;
    if sortie == 0 {
        return Ok(None);
    }
    let crtc = randr::get_output_info(conn, sortie, 0)?.reply()?.crtc;
    if crtc == 0 {
        return Ok(None);
    }
    let c = randr::get_crtc_info(conn, crtc, 0)?.reply()?;
    Ok(Some([i32::from(c.x), i32::from(c.y), i32::from(c.width), i32::from(c.height)]))
}

/// Taille fixe (le côté) et position demandée.
fn taille_fixe(conn: &RustConnection, fenetre: Window, cote: u16, (x, y): (i32, i32)) -> Res<()> {
    let mut t = WmSizeHints::new();
    t.position = Some((WmSizeHintsSpecification::ProgramSpecified, x, y));
    t.min_size = Some((i32::from(cote), i32::from(cote)));
    t.max_size = Some((i32::from(cote), i32::from(cote)));
    t.set_normal_hints(conn, fenetre)?;
    Ok(())
}

// --- le fil ---------------------------------------------------------------------------------

enum Ordre {
    Humeur(Humeur),
    Reaction(Reaction),
    Cote(u16),
    Menu(Vec<Entree>),
}

/// Poignée sur la fenêtre : les appels ne bloquent pas, ils envoient un ordre au fil X.
pub struct Fenetre {
    ordres: Sender<Ordre>,
}

impl Fenetre {
    /// `cote` : côté de la fenêtre en pixels ; `actions` reçoit le clic gauche (`Chatouille`) et
    /// les entrées choisies dans le menu du chien.
    pub fn new(cote: u16, actions: UnboundedSender<Action>) -> Self {
        let (ordres, rx) = mpsc::channel();
        let fil = std::thread::Builder::new()
            .name("fenetre-x11".into())
            .spawn(move || boucle(rx, cote, actions));
        if let Err(e) = fil {
            eprintln!("fenêtre du chien : fil non lancé : {e}");
        }
        Fenetre { ordres }
    }

    /// Nouvel état du chiot (`icone` de la ligne d'état).
    pub fn humeur(&mut self, humeur: Humeur) {
        let _ = self.ordres.send(Ordre::Humeur(humeur));
    }

    /// Joue une fois la réaction à un geste.
    pub fn reaction(&mut self, reaction: Reaction) {
        let _ = self.ordres.send(Ordre::Reaction(reaction));
    }

    /// Nouveau côté de la fenêtre, en pixels.
    pub fn cote(&mut self, cote: u16) {
        let _ = self.ordres.send(Ordre::Cote(cote));
    }

    /// Entrées du menu du chien, à jour de la dernière ligne d'état.
    pub fn menu(&mut self, entrees: Vec<Entree>) {
        let _ = self.ordres.send(Ordre::Menu(entrees));
    }
}

fn boucle(rx: mpsc::Receiver<Ordre>, cote: u16, actions: UnboundedSender<Action>) {
    let sprites = match Sprites::charger() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fenêtre du chien : planches illisibles : {e}");
            return;
        }
    };
    // les nombres de cases sont ceux des planches embarquées, lus une fois
    static COMPTES: std::sync::OnceLock<[usize; 12]> = std::sync::OnceLock::new();
    let _ = COMPTES.set(TOUTES.map(|a| sprites.compte(a)));
    fn compte(a: Anim) -> usize {
        COMPTES.get().map_or(1, |c| c[a as usize])
    }

    let debut = Instant::now();
    let mut fil = Fil {
        sprites,
        machine: Machine::new(compte),
        entrees: menu::entrees(&None),
        cote: cote.clamp(32, 256),
        droite_px: position::lire(),
        actions,
    };
    let mut x: Option<X> = None;
    let mut prochain_essai = Instant::now();
    let mut signale = false;
    loop {
        let mut ordres = Vec::new();
        match rx.recv_timeout(PAS) {
            Ok(o) => ordres.push(o),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        ordres.extend(rx.try_iter());
        let now = debut.elapsed();
        for o in ordres {
            match o {
                Ordre::Humeur(h) => fil.machine.humeur(h, now),
                Ordre::Reaction(r) => fil.machine.reaction(r, now),
                Ordre::Cote(c) => fil.cote = c.clamp(32, 256),
                Ordre::Menu(e) => fil.entrees = e,
            }
        }
        if x.is_none() && Instant::now() >= prochain_essai {
            match X::ouvrir(&fil) {
                Ok(ouverte) => {
                    x = Some(ouverte);
                    signale = false;
                }
                Err(e) => {
                    if !signale {
                        eprintln!("fenêtre du chien : {e}");
                        signale = true;
                    }
                    prochain_essai = Instant::now() + NOUVEL_ESSAI;
                }
            }
        }
        if let Some(ouverte) = x.as_mut() {
            let vue = fil.machine.vue(debut.elapsed());
            let r = ouverte
                .evenements(&mut fil)
                .and_then(|()| ouverte.poser(&fil))
                .and_then(|()| ouverte.montrer(&fil, vue));
            if let Err(e) = r {
                // serveur X perdu ou refus : tout est refermé, la fenêtre est rouverte plus tard
                eprintln!("fenêtre du chien : {e}");
                x = None;
                signale = true;
                prochain_essai = Instant::now() + NOUVEL_ESSAI;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compte(a: Anim) -> usize {
        match a {
            Anim::Aboie => 4,
            Anim::Patte => 8,
            Anim::Content => 6,
            Anim::Os => 8,
            Anim::OsFin => 8,
            Anim::Repos => 8,
            Anim::Dort => 4,
            Anim::Laisse => 1,
            Anim::ReclameLaisse => 4,
            Anim::Bain => 12,
            Anim::BainMousse | Anim::BainFin => 8,
        }
    }

    #[test]
    fn bain_brosse_mousse_puis_fin() {
        let mut m = Machine::new(compte);
        m.reaction(Reaction::Bain, s(0));
        assert_eq!(m.vue(s(0)), Vue { anim: Anim::Bain, case: 0 });
        assert_eq!(m.vue(s(150 * 12)), Vue { anim: Anim::BainMousse, case: 0 });
        assert_eq!(m.vue(s(150 * (12 + 8))), Vue { anim: Anim::BainMousse, case: 0 });
        assert_eq!(m.vue(s(150 * (12 + 24))), Vue { anim: Anim::BainFin, case: 0 });
        assert_eq!(m.vue(s(150 * (12 + 24 + 8))), Vue { anim: Anim::Repos, case: 0 });
    }

    fn s(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn ordre_des_animations() {
        let mut m = Machine::new(compte);
        // rien reçu : repos, en boucle
        assert_eq!(m.vue(s(0)), Vue { anim: Anim::Repos, case: 0 });
        assert_eq!(m.vue(s(150 * 9)).case, 1);
        m.humeur(Humeur::Laisse, s(2000));
        assert_eq!(m.vue(s(2000)), Vue { anim: Anim::Laisse, case: 0 });
        assert_eq!(m.vue(s(9000)), Vue { anim: Anim::Laisse, case: 0 });
        m.humeur(Humeur::Niche, s(10_000));
        assert_eq!(m.vue(s(10_000 + 150 * 5)), Vue { anim: Anim::Dort, case: 1 });
        // la réaction passe avant dort et laisse
        m.reaction(Reaction::Content, s(11_000));
        assert_eq!(m.vue(s(11_000)).anim, Anim::Content);
        // aboie passe avant tout
        m.humeur(Humeur::Aboie, s(11_100));
        assert_eq!(m.vue(s(11_100)).anim, Anim::Aboie);
        // fin de la réclamation : l'état du chiot revient (et pas la réaction abandonnée)
        m.humeur(Humeur::Repos, s(20_000));
        assert_eq!(m.vue(s(20_000)), Vue { anim: Anim::Repos, case: 0 });
        assert_eq!(m.vue(s(25_000)).anim, Anim::Repos);
    }

    #[test]
    fn aboie_boucle_sans_fin() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Aboie, s(0));
        assert_eq!(m.vue(s(1000)).case, 6 % 4);
        assert_eq!(m.vue(s(3_600_000)).anim, Anim::Aboie);
    }

    #[test]
    fn la_reaction_joue_une_fois_puis_revient() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Niche, s(0));
        m.reaction(Reaction::Content, s(1000));
        assert_eq!(m.vue(s(1000)), Vue { anim: Anim::Content, case: 0 });
        assert_eq!(m.vue(s(1000 + 150 * 5)).case, 5);
        // 6 cases * 150 ms : finie, retour à l'état (dort, depuis sa première case)
        assert_eq!(m.vue(s(1000 + 900)), Vue { anim: Anim::Dort, case: 0 });
        assert_eq!(m.vue(s(1000 + 900 + 150)), Vue { anim: Anim::Dort, case: 1 });
    }

    #[test]
    fn mange_joue_os_puis_os_fin_puis_revient() {
        let mut m = Machine::new(compte);
        m.reaction(Reaction::Mange, s(0));
        assert_eq!(m.vue(s(0)).anim, Anim::Os);
        assert_eq!(m.vue(s(150 * 8)), Vue { anim: Anim::OsFin, case: 0 });
        assert_eq!(m.vue(s(150 * 15)), Vue { anim: Anim::OsFin, case: 7 });
        assert_eq!(m.vue(s(150 * 16)).anim, Anim::Repos);
    }

    #[test]
    fn une_reaction_pendant_aboie_est_abandonnee() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Aboie, s(0));
        m.reaction(Reaction::Mange, s(100));
        assert_eq!(m.vue(s(200)).anim, Anim::Aboie);
        // elle n'a rien laissé derrière elle
        m.humeur(Humeur::Repos, s(300));
        assert_eq!(m.vue(s(400)).anim, Anim::Repos);
    }

    #[test]
    fn une_reaction_en_cours_est_abandonnee_pour_aboie() {
        let mut m = Machine::new(compte);
        m.reaction(Reaction::Content, s(0));
        m.humeur(Humeur::Aboie, s(300));
        assert_eq!(m.vue(s(400)).anim, Anim::Aboie);
        m.humeur(Humeur::Repos, s(500));
        assert_eq!(m.vue(s(600)).anim, Anim::Repos);
    }

    #[test]
    fn une_reaction_rejouee_repart_du_debut() {
        let mut m = Machine::new(compte);
        m.reaction(Reaction::Content, s(0));
        assert_eq!(m.vue(s(450)).case, 3);
        m.reaction(Reaction::Content, s(450));
        assert_eq!(m.vue(s(450)).case, 0);
    }

    #[test]
    fn patte_boucle_et_passe_avant_les_reactions() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Patte, s(0));
        assert_eq!(m.vue(s(0)), Vue { anim: Anim::Patte, case: 0 });
        assert_eq!(m.vue(s(150 * 9)), Vue { anim: Anim::Patte, case: 1 });
        assert_eq!(m.vue(s(3_600_000)).anim, Anim::Patte);
        // une réaction pendant patte est abandonnée
        m.reaction(Reaction::Mange, s(100));
        assert_eq!(m.vue(s(200)).anim, Anim::Patte);
        m.humeur(Humeur::Repos, s(300));
        assert_eq!(m.vue(s(400)).anim, Anim::Repos);
    }

    #[test]
    fn une_reaction_en_cours_est_abandonnee_pour_patte() {
        let mut m = Machine::new(compte);
        m.reaction(Reaction::Content, s(0));
        m.humeur(Humeur::Patte, s(300));
        assert_eq!(m.vue(s(400)).anim, Anim::Patte);
        m.humeur(Humeur::Repos, s(500));
        assert_eq!(m.vue(s(600)).anim, Anim::Repos);
    }

    #[test]
    fn aboie_patte_repos_sans_passer_par_repos() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Aboie, s(0));
        assert_eq!(m.vue(s(100)).anim, Anim::Aboie);
        m.humeur(Humeur::Patte, s(1000));
        assert_eq!(m.vue(s(1000)), Vue { anim: Anim::Patte, case: 0 });
        m.humeur(Humeur::Aboie, s(2000));
        assert_eq!(m.vue(s(2000)), Vue { anim: Anim::Aboie, case: 0 });
        m.humeur(Humeur::Patte, s(3000));
        assert_eq!(m.vue(s(3000)).anim, Anim::Patte);
        m.humeur(Humeur::Repos, s(4000)); // icone `assis`
        assert_eq!(m.vue(s(4000)), Vue { anim: Anim::Repos, case: 0 });
    }

    #[test]
    fn reclame_laisse_distincte_de_laisse_et_directe() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Laisse, s(0));
        assert_eq!(m.vue(s(0)), Vue { anim: Anim::Laisse, case: 0 });
        m.humeur(Humeur::ReclameLaisse, s(1000));
        assert_eq!(m.vue(s(1000)), Vue { anim: Anim::ReclameLaisse, case: 0 });
        assert_eq!(m.vue(s(1000 + 150 * 5)), Vue { anim: Anim::ReclameLaisse, case: 1 }); // boucle
        assert_eq!(m.vue(s(3_600_000)).anim, Anim::ReclameLaisse);
        // une réaction est abandonnée, puis ignorée
        m.reaction(Reaction::Content, s(2000));
        assert_eq!(m.vue(s(2100)).anim, Anim::ReclameLaisse);
    }

    #[test]
    fn passages_directs_entre_les_trois_reclamations() {
        let mut m = Machine::new(compte);
        m.humeur(Humeur::Patte, s(0));
        assert_eq!(m.vue(s(0)).anim, Anim::Patte);
        m.humeur(Humeur::Aboie, s(1000)); // patte -> aboie, sans repos
        assert_eq!(m.vue(s(1000)), Vue { anim: Anim::Aboie, case: 0 });
        m.humeur(Humeur::ReclameLaisse, s(2000));
        assert_eq!(m.vue(s(2000)), Vue { anim: Anim::ReclameLaisse, case: 0 });
        m.humeur(Humeur::Aboie, s(3000)); // reclame_laisse -> aboie
        assert_eq!(m.vue(s(3000)), Vue { anim: Anim::Aboie, case: 0 });
        m.humeur(Humeur::Repos, s(4000)); // aboie -> repos
        assert_eq!(m.vue(s(4000)), Vue { anim: Anim::Repos, case: 0 });
    }

    #[test]
    fn relance_en_laisse_aboie_puis_reprend_la_laisse_sans_repos() {
        // reclame_laisse -> aboie -> reclame_laisse, plusieurs fois : jamais `repos`, aucune
        // reaction relancee, chaque pose repart de sa premiere case.
        let mut m = Machine::new(compte);
        m.humeur(Humeur::ReclameLaisse, s(0));
        assert_eq!(m.vue(s(0)), Vue { anim: Anim::ReclameLaisse, case: 0 });
        for tour in 1..=3u64 {
            let t = tour * 10_000;
            m.humeur(Humeur::Aboie, s(t));
            assert_eq!(m.vue(s(t)), Vue { anim: Anim::Aboie, case: 0 });
            assert_eq!(m.vue(s(t + 2_900)).anim, Anim::Aboie);
            m.humeur(Humeur::ReclameLaisse, s(t + 3_000));
            assert_eq!(m.vue(s(t + 3_000)), Vue { anim: Anim::ReclameLaisse, case: 0 });
            assert_eq!(m.vue(s(t + 9_000)).anim, Anim::ReclameLaisse);
        }
        // une humeur identique rejouee (ligne d'etat repetee) ne fait pas repartir la pose
        m.humeur(Humeur::ReclameLaisse, s(40_300));
        assert_eq!(m.vue(s(40_300)).anim, Anim::ReclameLaisse);
        assert_eq!(m.reaction, None);
    }

    #[test]
    fn humeur_de_l_icone() {
        assert_eq!(Humeur::de_icone("patte"), Humeur::Patte);
        assert_eq!(Humeur::de_icone("aboie"), Humeur::Aboie);
        assert_eq!(Humeur::de_icone("niche"), Humeur::Niche);
        assert_eq!(Humeur::de_icone("laisse"), Humeur::Laisse);
        assert_eq!(Humeur::de_icone("reclame_laisse"), Humeur::ReclameLaisse);
        assert_eq!(Humeur::de_icone("assis"), Humeur::Repos);
        assert_eq!(Humeur::de_icone("autre"), Humeur::Repos);
    }

    #[test]
    fn zone_du_moniteur_principal() {
        let travail = Some([0, 0, 1920, 1040]);
        // un seul écran : la zone de travail
        assert_eq!(zone_principale(travail, None, (1920, 1080)), [0, 0, 1920, 1040]);
        // deux écrans côte à côte, le principal à gauche
        let deux = Some([0, 0, 3840, 1040]);
        assert_eq!(
            zone_principale(deux, Some([0, 0, 1920, 1080]), (3840, 1080)),
            [0, 0, 1920, 1040]
        );
        // pas de _NET_WORKAREA : l'écran
        assert_eq!(zone_principale(None, None, (800, 600)), [0, 0, 800, 600]);
        // moniteur hors de la zone de travail : on garde la zone
        assert_eq!(
            zone_principale(travail, Some([5000, 0, 100, 100]), (1920, 1080)),
            [0, 0, 1920, 1040]
        );
    }

    #[test]
    fn pose_sur_le_bas_de_la_zone() {
        let zone = [0, 0, 800, 560];
        // par défaut : coin bas droit
        assert_eq!(pose(zone, 0, 64), (736, 496));
        // décalée vers la gauche, ramenée dans la zone
        assert_eq!(pose(zone, 100, 64), (636, 496));
        assert_eq!(pose(zone, 5000, 64), (0, 496));
        assert_eq!(pose(zone, -3, 64), (736, 496));
        // zone qui ne commence pas à l'origine (second écran à gauche, tableau de bord en haut)
        assert_eq!(pose([1920, 30, 1280, 1000], 10, 100), (1920 + 1280 - 100 - 10, 30 + 1000 - 100));
    }

    #[test]
    fn clic_ou_glisser_selon_le_deplacement() {
        let neuf = || Appui { x0: 100, y0: 100, fenetre_x0: 700, glisse: false };
        let mut a = neuf();
        assert!(!a.bouge(102, 101)); // moins de 4 px : clic
        assert!(a.bouge(104, 100)); // 4 px : glisser
        assert!(a.bouge(100, 100)); // et ça le reste
        assert_eq!(a.fenetre_x(90), 690);
        let mut b = neuf();
        assert!(!b.bouge(103, 100));
        assert!(b.bouge(100, 96));
    }

    #[test]
    fn decoupe_shape_par_rangees() {
        // 4 x 4 (deux dernières rangées vides) : rangée 0 « .XX. », rangée 1 « XX.X » (le dernier pixel de la rangée 1 : 128)
        let rgba: Vec<u8> = [0u8, 255, 255, 0, 255, 255, 127, 128, 0, 0, 0, 0, 0, 0, 0, 0]
            .iter()
            .flat_map(|&a| [0, 0, 0, a])
            .collect();
        let r: Vec<(i16, i16, u16, u16)> =
            rectangles_opaques(&rgba, 4).iter().map(|r| (r.x, r.y, r.width, r.height)).collect();
        assert_eq!(r, vec![(1, 0, 2, 1), (0, 1, 2, 1), (3, 1, 1, 1)]);
    }
}
