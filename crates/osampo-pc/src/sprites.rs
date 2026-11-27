//! Planches du chien (SPEC-006) : les PNG de `widgets/osampo/ui/sprites/` sont embarqués,
//! découpés en cases de 64 x 64 comme dans `widgets/osampo/tools/gen_assets.py` (planche
//! horizontale, une case par image), puis mis à l'échelle de la fenêtre au plus proche voisin.

use std::io::Cursor;

/// Côté d'une case de planche, en pixels.
pub const CASE: usize = 64;

/// Les animations dont la fenêtre a besoin. `mange` n'a pas de planche à lui : le widget joue
/// `os` (il mâche) puis `os_fin` (il se lèche les babines). `Dort` est `dort.png` devant
/// `niche.png` ; `Laisse` est `laisse_icone.png` seule (une case, sans chien) ; `ReclameLaisse`
/// est `laisse.png`, le chien qui tient la laisse (la planche de la board). Le bain enchaîne
/// `bain` (brossage), `bain_mousse` et `bain_fin`, comme le widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anim {
    Repos,
    Dort,
    Aboie,
    Patte,
    Content,
    Os,
    OsFin,
    Laisse,
    ReclameLaisse,
    Bain,
    BainMousse,
    BainFin,
}

impl Anim {
    /// Nom publié dans `_OSAMPO_ANIM` (SPEC-011). `os_fin` prolonge `os` : même nom.
    pub fn nom(self) -> &'static str {
        match self {
            Anim::Repos => "repos",
            Anim::Dort => "dort",
            Anim::Aboie => "aboie",
            Anim::Patte => "patte",
            Anim::Content => "content",
            Anim::Os | Anim::OsFin => "os",
            Anim::Laisse => "laisse",
            Anim::ReclameLaisse => "reclame_laisse",
            Anim::Bain | Anim::BainMousse | Anim::BainFin => "bain",
        }
    }
}

/// Une case : 64 x 64 pixels RGBA, ligne par ligne.
pub type Case = Vec<u8>;

pub struct Sprites {
    repos: Vec<Case>,
    dort: Vec<Case>,
    laisse: Vec<Case>,
    reclame_laisse: Vec<Case>,
    aboie: Vec<Case>,
    patte: Vec<Case>,
    content: Vec<Case>,
    os: Vec<Case>,
    os_fin: Vec<Case>,
    bain: Vec<Case>,
    bain_mousse: Vec<Case>,
    bain_fin: Vec<Case>,
}

impl Sprites {
    pub fn charger() -> Result<Self, String> {
        let dir = |nom: &str, png: &[u8]| decouper(png).map_err(|e| format!("{nom}.png : {e}"));
        let niche = dir("niche", include_bytes!("../../../widgets/osampo/ui/sprites/niche.png"))?;
        let niche = niche.first().ok_or("niche.png : aucune case")?;
        let dort: Vec<Case> =
            dir("dort", include_bytes!("../../../widgets/osampo/ui/sprites/dort.png"))?
                .iter()
                .map(|c| composer(c, niche))
                .collect();
        let laisse =
            vignette(include_bytes!("../../../widgets/osampo/ui/sprites/laisse_icone.png"))
                .map_err(|e| format!("laisse_icone.png : {e}"))?;
        Ok(Sprites {
            repos: dir("repos", include_bytes!("../../../widgets/osampo/ui/sprites/repos.png"))?,
            dort,
            laisse: vec![laisse],
            reclame_laisse: dir(
                "laisse",
                include_bytes!("../../../widgets/osampo/ui/sprites/laisse.png"),
            )?,
            aboie: dir("aboie", include_bytes!("../../../widgets/osampo/ui/sprites/aboie.png"))?,
            patte: dir("patte", include_bytes!("../../../widgets/osampo/ui/sprites/patte.png"))?,
            content: dir(
                "content",
                include_bytes!("../../../widgets/osampo/ui/sprites/content.png"),
            )?,
            os: dir("os", include_bytes!("../../../widgets/osampo/ui/sprites/os.png"))?,
            os_fin: dir(
                "os_fin",
                include_bytes!("../../../widgets/osampo/ui/sprites/os_fin.png"),
            )?,
            bain: dir("bain", include_bytes!("../../../widgets/osampo/ui/sprites/bain.png"))?,
            bain_mousse: dir(
                "bain_mousse",
                include_bytes!("../../../widgets/osampo/ui/sprites/bain_mousse.png"),
            )?,
            bain_fin: dir(
                "bain_fin",
                include_bytes!("../../../widgets/osampo/ui/sprites/bain_fin.png"),
            )?,
        })
    }

    fn planche(&self, anim: Anim) -> &[Case] {
        match anim {
            Anim::Repos => &self.repos,
            Anim::Dort => &self.dort,
            Anim::Laisse => &self.laisse,
            Anim::ReclameLaisse => &self.reclame_laisse,
            Anim::Aboie => &self.aboie,
            Anim::Patte => &self.patte,
            Anim::Content => &self.content,
            Anim::Os => &self.os,
            Anim::OsFin => &self.os_fin,
            Anim::Bain => &self.bain,
            Anim::BainMousse => &self.bain_mousse,
            Anim::BainFin => &self.bain_fin,
        }
    }

    /// Nombre d'images de l'animation.
    pub fn compte(&self, anim: Anim) -> usize {
        self.planche(anim).len()
    }

    pub fn case(&self, anim: Anim, i: usize) -> &Case {
        let p = self.planche(anim);
        &p[i.min(p.len() - 1)]
    }
}

/// Décode un PNG en RGBA 8 bits : largeur, hauteur, pixels.
fn decoder(png: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let mut dec = png::Decoder::new(Cursor::new(png));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut lecteur = dec.read_info().map_err(|e| e.to_string())?;
    let taille = lecteur.output_buffer_size().ok_or("image trop grande")?;
    let mut buf = vec![0u8; taille];
    let info = lecteur.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = en_rgba(&buf[..info.buffer_size()], info.color_type)?;
    Ok((w, h, rgba))
}

/// Décode un PNG en RGBA 8 bits et le découpe en cases de 64 x 64.
pub fn decouper(png: &[u8]) -> Result<Vec<Case>, String> {
    let (w, h, rgba) = decoder(png)?;
    if h != CASE || w == 0 || w % CASE != 0 {
        return Err(format!("{w} x {h} : une planche fait 64 de haut, par cases de 64"));
    }
    Ok((0..w / CASE)
        .map(|c| {
            let mut case = Vec::with_capacity(CASE * CASE * 4);
            for y in 0..CASE {
                let debut = (y * w + c * CASE) * 4;
                case.extend_from_slice(&rgba[debut..debut + CASE * 4]);
            }
            case
        })
        .collect())
}

/// Une image de 64 x 64 pixels ou moins, posée en bas et au milieu d'une case transparente, à
/// sa taille : elle n'est jamais agrandie, donc tient dans toute fenêtre de 32 pixels et plus
/// (la mise à l'échelle de la case la réduit au besoin). Plus grande que la case : erreur.
fn vignette(png: &[u8]) -> Result<Case, String> {
    let (w, h, rgba) = decoder(png)?;
    if w == 0 || h == 0 || w > CASE || h > CASE {
        return Err(format!("{w} x {h} : au plus 64 x 64"));
    }
    let mut case = vec![0u8; CASE * CASE * 4];
    let (x0, y0) = ((CASE - w) / 2, CASE - h);
    for y in 0..h {
        let dst = ((y0 + y) * CASE + x0) * 4;
        case[dst..dst + w * 4].copy_from_slice(&rgba[y * w * 4..(y + 1) * w * 4]);
    }
    Ok(case)
}

/// Pose `devant` sur `derriere` (alpha « over », couleurs non prémultipliées).
pub fn composer(devant: &Case, derriere: &Case) -> Case {
    let mut out = Vec::with_capacity(devant.len());
    for (d, f) in devant.chunks(4).zip(derriere.chunks(4)) {
        let (ad, af) = (d[3] as u32, f[3] as u32);
        let fond = af * (255 - ad) / 255; // part visible du fond
        let a = ad + fond;
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for k in 0..3 {
            out.push(((d[k] as u32 * ad + f[k] as u32 * fond) / a) as u8);
        }
        out.push(a as u8);
    }
    out
}

/// Met une case de 64 x 64 à `cote` x `cote` pixels (32..=256) au plus proche voisin, RGBA
/// non prémultiplié : un pixel transparent reste transparent.
pub fn mettre_a_l_echelle(case: &Case, cote: usize) -> Vec<u8> {
    let cote = cote.clamp(32, 256);
    let mut out = Vec::with_capacity(cote * cote * 4);
    for y in 0..cote {
        for x in 0..cote {
            let i = ((y * CASE / cote) * CASE + x * CASE / cote) * 4;
            out.extend_from_slice(&case[i..i + 4]);
        }
    }
    out
}

fn en_rgba(src: &[u8], couleur: png::ColorType) -> Result<Vec<u8>, String> {
    use png::ColorType::*;
    Ok(match couleur {
        Rgba => src.to_vec(),
        Rgb => src.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        GrayscaleAlpha => src.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        Indexed => return Err("PNG indexé non décodé".into()),
    })
}

/// Met des pixels RGBA (une case déjà mise à l'échelle, voir [`mettre_a_l_echelle`]) au format
/// des pixels X (4 octets par pixel). `fond` = `None` : pixels prémultipliés avec leur alpha
/// (visuel ARGB, compositeur) ; `Some(rgb)` : composés sur ce fond uni, alpha à 255. `msb` :
/// serveur X en octets de poids fort d'abord.
pub fn pixels_x(rgba: &[u8], fond: Option<[u8; 3]>, msb: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let (r, g, b, a) = (p[0] as u32, p[1] as u32, p[2] as u32, p[3] as u32);
        let (r, g, b, a) = match fond {
            None => (r * a / 255, g * a / 255, b * a / 255, a),
            Some(f) => {
                let mix = |c: u32, f: u8| (c * a + f as u32 * (255 - a)) / 255;
                (mix(r, f[0]), mix(g, f[1]), mix(b, f[2]), 255)
            }
        };
        let (r, g, b, a) = (r as u8, g as u8, b as u8, a as u8);
        if msb {
            out.extend_from_slice(&[a, r, g, b]);
        } else {
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_planches_embarquees_ont_leurs_cases() {
        // cases comptees dans widgets/osampo/ui/sprites/ : 256, 384, 512, 512 pixels de large
        let s = Sprites::charger().expect("planches");
        assert_eq!(s.compte(Anim::Aboie), 4);
        assert_eq!(s.compte(Anim::Patte), 8); // patte.png : 512 de large
        assert!(s.case(Anim::Patte, 0).chunks(4).any(|p| p[3] == 255));
        assert_eq!(s.compte(Anim::Content), 6);
        assert_eq!(s.compte(Anim::Os), 8);
        assert_eq!(s.compte(Anim::OsFin), 8);
        assert_eq!(s.compte(Anim::Bain), 12); // bain.png : 768 de large
        assert_eq!(s.compte(Anim::BainMousse), 8);
        assert_eq!(s.compte(Anim::BainFin), 8);
        assert_eq!(s.case(Anim::Aboie, 3).len(), CASE * CASE * 4);
        // une image ne reste pas vide : il y a du chien dedans
        assert!(s.case(Anim::Aboie, 0).chunks(4).any(|p| p[3] == 255));
        // hors planche : la dernière case
        assert_eq!(s.case(Anim::Aboie, 99), s.case(Anim::Aboie, 3));
    }

    #[test]
    fn planches_du_chien_pose() {
        let s = Sprites::charger().expect("planches");
        assert_eq!(s.compte(Anim::Repos), 8); // repos.png : 512 de large
        assert_eq!(s.compte(Anim::Dort), 4); // dort.png : 256 de large
        assert_eq!(s.compte(Anim::Laisse), 1);
        // laisse.png (256 de large) : le chien qui tient la laisse, pas la laisse seule
        assert_eq!(s.compte(Anim::ReclameLaisse), 4);
        let rl = s.case(Anim::ReclameLaisse, 0);
        assert!(rl.chunks(4).any(|p| p[3] == 255));
        assert_ne!(rl, s.case(Anim::Laisse, 0));
        let opaques = |c: &Case| c.chunks(4).filter(|p| p[3] > 0).count();
        // la niche est derrière le chien qui dort : plus de pixels que dort seul
        let dort_seul =
            decouper(include_bytes!("../../../widgets/osampo/ui/sprites/dort.png")).unwrap();
        assert!(opaques(s.case(Anim::Dort, 0)) > opaques(&dort_seul[0]));
        // la laisse est seule, en bas de la case
        let l = s.case(Anim::Laisse, 0);
        assert!(opaques(l) > 0);
        assert!(l[..CASE * 40 * 4].chunks(4).all(|p| p[3] == 0), "rien dans le haut");
    }

    #[test]
    fn composition_devant_derriere() {
        let mut devant = vec![0u8; CASE * CASE * 4];
        let mut derriere = vec![0u8; CASE * CASE * 4];
        devant[0..4].copy_from_slice(&[10, 20, 30, 255]); // opaque devant : gagne
        derriere[0..4].copy_from_slice(&[200, 200, 200, 255]);
        derriere[4..8].copy_from_slice(&[1, 2, 3, 255]); // devant transparent : le fond
        let c = composer(&devant, &derriere);
        assert_eq!(&c[0..4], &[10, 20, 30, 255]);
        assert_eq!(&c[4..8], &[1, 2, 3, 255]);
        assert_eq!(&c[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn echelle_garde_la_transparence() {
        let s = Sprites::charger().expect("planches");
        for anim in [Anim::Repos, Anim::Dort, Anim::Aboie, Anim::Laisse] {
            let case = s.case(anim, 0);
            assert!(case.chunks(4).any(|p| p[3] == 0));
            for cote in [32usize, 64, 256] {
                let e = mettre_a_l_echelle(case, cote);
                assert_eq!(e.len(), cote * cote * 4);
                assert!(e.chunks(4).any(|p| p[3] == 0), "{anim:?} {cote}");
                // chaque pixel de sortie est un pixel de la case, sans mélange
                for (n, p) in e.chunks(4).enumerate() {
                    let (x, y) = (n % cote, n / cote);
                    let i = ((y * CASE / cote) * CASE + x * CASE / cote) * 4;
                    assert_eq!(p, &case[i..i + 4]);
                }
            }
        }
        let c = s.case(Anim::Repos, 0);
        assert_eq!(&mettre_a_l_echelle(c, 64), c);
    }

    #[test]
    fn noms_publies() {
        assert_eq!(Anim::Repos.nom(), "repos");
        assert_eq!(Anim::Dort.nom(), "dort");
        assert_eq!(Anim::Aboie.nom(), "aboie");
        assert_eq!(Anim::Patte.nom(), "patte");
        assert_eq!(Anim::Content.nom(), "content");
        assert_eq!(Anim::Os.nom(), "os");
        assert_eq!(Anim::OsFin.nom(), "os");
        assert_eq!(Anim::Laisse.nom(), "laisse");
        assert_eq!(Anim::ReclameLaisse.nom(), "reclame_laisse");
        for a in [Anim::Bain, Anim::BainMousse, Anim::BainFin] {
            assert_eq!(a.nom(), "bain");
        }
    }

    #[test]
    fn planche_de_mauvaise_hauteur_refusee() {
        let mut png_octets = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_octets, 64, 32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&vec![0u8; 64 * 32 * 4]).unwrap();
        }
        assert!(decouper(&png_octets).is_err());
    }

    #[test]
    fn pixels_agrandis_et_composes() {
        let mut case = vec![0u8; CASE * CASE * 4];
        case[0..4].copy_from_slice(&[200, 100, 50, 128]); // premier pixel, à moitié transparent
        let argb = pixels_x(&mettre_a_l_echelle(&case, 192), None, false);
        assert_eq!(argb.len(), 192 * 192 * 4);
        // BGRA prémultiplié, répété sur 3 x 3 pixels
        for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
            let i = (y * 192 + x) * 4;
            assert_eq!(&argb[i..i + 4], &[25, 50, 100, 128]);
        }
        assert_eq!(&argb[12..16], &[0, 0, 0, 0]); // le pixel suivant est transparent
        let uni = pixels_x(&case, Some([10, 20, 30]), false);
        assert_eq!(&uni[0..4], &[40, 60, 105, 255]);
        assert_eq!(&uni[4..8], &[30, 20, 10, 255]); // transparent = le fond
        let msb = pixels_x(&case, None, true);
        assert_eq!(&msb[0..4], &[128, 100, 50, 25]);
    }
}
