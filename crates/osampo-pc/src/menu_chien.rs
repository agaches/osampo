//! Menu du chien (SPEC-011, « Souris ») : fenêtre `override-redirect` ouverte au pointeur, avec
//! les entrées du menu de l'icône (`menu::entrees`). La partie pure (disposition, propriété
//! `_OSAMPO_MENU`, entrée sous un `y`, décalage au bord de l'écran) est testée sans serveur X.
//! L'ouverture est appelée depuis le fil X de la fenêtre du chien, sur sa connexion.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::menu::{Action, Entree};

const HAUTEUR_ENTREE: u16 = 24;
const HAUTEUR_SEPARATEUR: u16 = 9;
const MARGE_Y: u16 = 3;
const MARGE_X: u16 = 12;
const LARGEUR_MIN: u16 = 120;

const FOND: u32 = 0x00f2_f2f2;
const TEXTE: u32 = 0x0000_0000;
const GRIS: u32 = 0x0080_8080;
const BORD: u32 = 0x0060_6060;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

// --- la partie pure --------------------------------------------------------------------------

/// `(y, hauteur)` de chaque entrée dans la fenêtre du menu, et la hauteur totale.
pub fn disposition(entrees: &[Entree]) -> (Vec<(u16, u16)>, u16) {
    let mut y = MARGE_Y;
    let mut v = Vec::with_capacity(entrees.len());
    for e in entrees {
        let h = if e.separateur { HAUTEUR_SEPARATEUR } else { HAUTEUR_ENTREE };
        v.push((y, h));
        y += h;
    }
    (v, y + MARGE_Y)
}

/// Une entrée est cliquable : ni statut, ni séparateur, ni grisée.
fn cliquable(e: &Entree) -> bool {
    e.actif && e.action.is_some() && !e.separateur
}

/// Texte de `_OSAMPO_MENU` : une ligne `y<TAB>hauteur<TAB>actif<TAB>texte` par entrée,
/// séparateur : texte `-`.
pub fn propriete(entrees: &[Entree], dispo: &[(u16, u16)]) -> String {
    entrees
        .iter()
        .zip(dispo)
        .map(|(e, (y, h))| {
            let texte = if e.separateur { "-" } else { e.texte.as_str() };
            format!("{y}\t{h}\t{}\t{texte}", u8::from(cliquable(e)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Index de l'entrée qui contient la ligne `y` (dans la fenêtre du menu).
pub fn entree_sous(dispo: &[(u16, u16)], y: i32) -> Option<usize> {
    dispo
        .iter()
        .position(|&(ey, h)| y >= i32::from(ey) && y < i32::from(ey) + i32::from(h))
}

/// Coin haut gauche du menu ouvert au pointeur, ramené dans l'écran (décalé vers la gauche ou
/// vers le haut si le pointeur est trop près du bord).
pub fn decaler(pointeur: (i32, i32), taille: (u16, u16), ecran: (i32, i32)) -> (i32, i32) {
    let x = pointeur.0.min(ecran.0 - i32::from(taille.0)).max(0);
    let y = pointeur.1.min(ecran.1 - i32::from(taille.1)).max(0);
    (x, y)
}

/// Résultat d'un clic dans la fenêtre du menu : `Some(Some(action))` entrée active,
/// `Some(None)` clic hors du menu (fermer sans effet), `None` clic sans effet dans le menu.
pub fn clic(
    entrees: &[Entree],
    dispo: &[(u16, u16)],
    largeur: u16,
    hauteur: u16,
    x: i32,
    y: i32,
) -> Option<Option<Action>> {
    if x < 0 || y < 0 || x >= i32::from(largeur) || y >= i32::from(hauteur) {
        return Some(None);
    }
    let i = entree_sous(dispo, y)?;
    let e = &entrees[i];
    if cliquable(e) {
        Some(e.action)
    } else {
        None
    }
}

// --- le serveur X ----------------------------------------------------------------------------

/// Un menu ouvert : à alimenter avec les événements X de la connexion jusqu'à ce que
/// [`Menu::evenement`] rende `Some`.
pub struct Menu {
    fenetre: Window,
    gc: Gcontext,
    police: Font,
    entrees: Vec<Entree>,
    dispo: Vec<(u16, u16)>,
    largeur: u16,
    hauteur: u16,
    ascent: i16,
    descent: i16,
}

fn char2b(texte: &str) -> Vec<Char2b> {
    texte
        .encode_utf16()
        .take(255)
        .map(|u| Char2b { byte1: (u >> 8) as u8, byte2: u as u8 })
        .collect()
}

fn ouvrir_police(conn: &RustConnection) -> Res<Font> {
    let id = conn.generate_id()?;
    let noms: [&[u8]; 2] = [b"-misc-fixed-medium-r-normal--14-*-*-*-c-*-iso10646-1", b"fixed"];
    for nom in noms {
        if conn.open_font(id, nom)?.check().is_ok() {
            return Ok(id);
        }
    }
    Err("aucune police du serveur X".into())
}

/// Ouvre le menu au pointeur `(x, y)` (coordonnées racine) et saisit le pointeur (pas le
/// clavier). Erreur si la saisie échoue : le menu est alors détruit.
pub fn ouvrir(
    conn: &RustConnection,
    ecran: usize,
    pointeur: (i32, i32),
    entrees: Vec<Entree>,
) -> Res<Menu> {
    let racine = conn.setup().roots[ecran].root;
    let (largeur_ecran, hauteur_ecran) = {
        let e = &conn.setup().roots[ecran];
        (i32::from(e.width_in_pixels), i32::from(e.height_in_pixels))
    };
    let police = ouvrir_police(conn)?;
    let (dispo, hauteur) = disposition(&entrees);

    let (mut ascent, mut descent, mut texte_max) = (12i16, 3i16, 0i16);
    for e in entrees.iter().filter(|e| !e.separateur) {
        let r = conn.query_text_extents(police, &char2b(&e.texte))?.reply()?;
        ascent = r.font_ascent;
        descent = r.font_descent;
        texte_max = texte_max.max(r.overall_width.clamp(0, i32::from(i16::MAX)) as i16);
    }
    let largeur = (texte_max as u16 + 2 * MARGE_X).max(LARGEUR_MIN);
    let (x, y) = decaler(pointeur, (largeur, hauteur), (largeur_ecran, hauteur_ecran));

    let fenetre = conn.generate_id()?;
    let gc = conn.generate_id()?;
    conn.create_window(
        0, // CopyFromParent
        fenetre,
        racine,
        x as i16,
        y as i16,
        largeur,
        hauteur,
        0, // sans bord X : le liseré est dessiné dans la fenêtre (cf. `decaler`)
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(FOND)
            .event_mask(EventMask::EXPOSURE | EventMask::BUTTON_PRESS),
    )?;
    conn.create_gc(gc, fenetre, &CreateGCAux::new().font(police).foreground(TEXTE).background(FOND))?;

    let utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let prop = conn.intern_atom(false, b"_OSAMPO_MENU")?.reply()?.atom;
    conn.change_property8(PropMode::REPLACE, fenetre, AtomEnum::WM_NAME, AtomEnum::STRING, b"OsampoMenu")?;
    conn.change_property8(
        PropMode::REPLACE,
        fenetre,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"osampo-pc\0OsampoMenu\0",
    )?;
    conn.change_property8(PropMode::REPLACE, fenetre, prop, utf8, propriete(&entrees, &dispo).as_bytes())?;
    conn.map_window(fenetre)?;

    let menu = Menu { fenetre, gc, police, entrees, dispo, largeur, hauteur, ascent, descent };
    let prise = conn
        .grab_pointer(
            false,
            fenetre,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            x11rb::NONE,
            x11rb::NONE,
            x11rb::CURRENT_TIME,
        )?
        .reply()?;
    if prise.status != GrabStatus::SUCCESS {
        menu.fermer(conn);
        return Err("saisie du pointeur refusée".into());
    }
    menu.dessiner(conn)?;
    conn.flush()?;
    Ok(menu)
}

impl Menu {
    fn dessiner(&self, conn: &RustConnection) -> Res<()> {
        conn.change_gc(self.gc, &ChangeGCAux::new().foreground(BORD))?;
        conn.poly_rectangle(
            self.fenetre,
            self.gc,
            &[Rectangle { x: 0, y: 0, width: self.largeur - 1, height: self.hauteur - 1 }],
        )?;
        for (e, &(y, h)) in self.entrees.iter().zip(&self.dispo) {
            if e.separateur {
                conn.change_gc(self.gc, &ChangeGCAux::new().foreground(BORD))?;
                let ligne = (y + h / 2) as i16;
                conn.poly_line(
                    CoordMode::ORIGIN,
                    self.fenetre,
                    self.gc,
                    &[Point { x: MARGE_X as i16 / 2, y: ligne }, Point { x: self.largeur as i16 - MARGE_X as i16 / 2, y: ligne }],
                )?;
                continue;
            }
            let couleur = if cliquable(e) { TEXTE } else { GRIS };
            conn.change_gc(self.gc, &ChangeGCAux::new().foreground(couleur))?;
            let base = y as i16 + (h as i16 + self.ascent - self.descent) / 2;
            conn.image_text16(self.fenetre, self.gc, MARGE_X as i16, base, &char2b(&e.texte))?;
        }
        Ok(())
    }

    /// Traite un événement X. `None` : le menu reste ouvert. `Some(choix)` : le menu est fermé,
    /// `choix` est l'entrée active cliquée, ou `None` pour un clic hors du menu.
    pub fn evenement(&self, conn: &RustConnection, ev: &Event) -> Option<Option<Action>> {
        match ev {
            Event::Expose(e) if e.window == self.fenetre && e.count == 0 => {
                let _ = self.dessiner(conn).and_then(|()| Ok(conn.flush()?));
                None
            }
            Event::ButtonPress(b) if b.event == self.fenetre => {
                let r = if b.detail == 1 {
                    clic(
                        &self.entrees,
                        &self.dispo,
                        self.largeur,
                        self.hauteur,
                        i32::from(b.event_x),
                        i32::from(b.event_y),
                    )
                } else if b.event_x < 0
                    || b.event_y < 0
                    || b.event_x >= self.largeur as i16
                    || b.event_y >= self.hauteur as i16
                {
                    Some(None)
                } else {
                    None
                };
                if r.is_some() {
                    self.fermer(conn);
                }
                r
            }
            _ => None,
        }
    }

    /// Relâche le pointeur et détruit la fenêtre.
    pub fn fermer(&self, conn: &RustConnection) {
        let _ = conn.ungrab_pointer(x11rb::CURRENT_TIME);
        let _ = conn.free_gc(self.gc);
        let _ = conn.close_font(self.police);
        let _ = conn.destroy_window(self.fenetre);
        let _ = conn.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::statuts::Vue;

    fn e(texte: &str, action: Option<Action>, actif: bool) -> Entree {
        Entree { texte: texte.into(), action, actif, separateur: false }
    }

    fn sep() -> Entree {
        Entree { texte: String::new(), action: None, actif: false, separateur: true }
    }

    fn exemple() -> Vec<Entree> {
        vec![
            e("Statut", None, false),
            sep(),
            e("Chatouiller", Some(Action::Chatouille), true),
            e("Donner un os", Some(Action::Os), false),
        ]
    }

    #[test]
    fn disposition_empile_les_entrees() {
        let (d, h) = disposition(&exemple());
        assert_eq!(
            d,
            vec![
                (MARGE_Y, HAUTEUR_ENTREE),
                (MARGE_Y + HAUTEUR_ENTREE, HAUTEUR_SEPARATEUR),
                (MARGE_Y + HAUTEUR_ENTREE + HAUTEUR_SEPARATEUR, HAUTEUR_ENTREE),
                (MARGE_Y + 2 * HAUTEUR_ENTREE + HAUTEUR_SEPARATEUR, HAUTEUR_ENTREE),
            ]
        );
        assert_eq!(h, MARGE_Y + 3 * HAUTEUR_ENTREE + HAUTEUR_SEPARATEUR + MARGE_Y);
    }

    #[test]
    fn propriete_une_ligne_par_entree() {
        let en = exemple();
        let (d, _) = disposition(&en);
        let p = propriete(&en, &d);
        let lignes: Vec<&str> = p.split('\n').collect();
        assert_eq!(lignes.len(), 4);
        assert_eq!(lignes[0], "3\t24\t0\tStatut");
        assert_eq!(lignes[1], "27\t9\t0\t-");
        assert_eq!(lignes[2], "36\t24\t1\tChatouiller");
        assert_eq!(lignes[3], "60\t24\t0\tDonner un os");
    }

    #[test]
    fn entree_sous_un_y() {
        let (d, h) = disposition(&exemple());
        assert_eq!(entree_sous(&d, 0), None);
        assert_eq!(entree_sous(&d, 3), Some(0));
        assert_eq!(entree_sous(&d, 26), Some(0));
        assert_eq!(entree_sous(&d, 27), Some(1));
        assert_eq!(entree_sous(&d, 40), Some(2));
        assert_eq!(entree_sous(&d, i32::from(h) - 1), None);
    }

    #[test]
    fn decalage_au_bord() {
        let ecran = (1920, 1080);
        assert_eq!(decaler((100, 200), (150, 300), ecran), (100, 200));
        assert_eq!(decaler((1900, 200), (150, 300), ecran), (1770, 200));
        assert_eq!(decaler((100, 1070), (150, 300), ecran), (100, 780));
        assert_eq!(decaler((1919, 1079), (150, 300), ecran), (1770, 780));
        // menu plus grand que l'écran : reste à l'origine
        assert_eq!(decaler((50, 50), (2000, 2000), ecran), (0, 0));
    }

    #[test]
    fn clic_selon_l_entree() {
        let en = exemple();
        let (d, h) = disposition(&en);
        let l = 150;
        assert_eq!(clic(&en, &d, l, h, 10, 40), Some(Some(Action::Chatouille)));
        // grisée, statut, séparateur, marge : rien
        assert_eq!(clic(&en, &d, l, h, 10, 65), None);
        assert_eq!(clic(&en, &d, l, h, 10, 10), None);
        assert_eq!(clic(&en, &d, l, h, 10, 30), None);
        assert_eq!(clic(&en, &d, l, h, 10, 0), None);
        // hors du menu : fermeture sans effet
        assert_eq!(clic(&en, &d, l, h, -5, 40), Some(None));
        assert_eq!(clic(&en, &d, l, h, 10, i32::from(h)), Some(None));
        assert_eq!(clic(&en, &d, l, h, i32::from(l), 40), Some(None));
    }

    #[test]
    fn menu_du_chien_reprend_les_entrees_de_l_icone() {
        let vue: Vue = None;
        let en = crate::menu::entrees(&vue);
        let (d, _) = disposition(&en);
        let p = propriete(&en, &d);
        assert_eq!(p.lines().count(), en.len());
        assert!(p.lines().any(|l| l.ends_with("\t-")));
        assert!(p.lines().last().unwrap().ends_with("\t1\tQuitter Osampo"));
    }
}
