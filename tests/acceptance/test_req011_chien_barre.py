"""REQ-011 : le chien posé sur la barre des tâches, desktop pet (SPEC-011, SPEC-006, ADR-003).

La fenêtre est pilotée par XTest sur un Xvfb sans gestionnaire de fenêtres. Le test lit les
propriétés de la fenêtre (`_OSAMPO_ANIM`, `_OSAMPO_MENU`, SPEC-011) et les commandes que
reçoit la fausse board.
"""
import shutil
import time
import tomllib

import pytest

import xfaux
from outils import RACINE, attendre, config_toml, hhmm, id_rappel, maintenant, reglages

MODELE = RACINE / "widgets/osampo/config.toml"
X0, Y0, LARGEUR, HAUTEUR = xfaux.ZONE      # zone de pose simulee : bas de zone a y = 560
BAS = Y0 + HAUTEUR


# ------------------------------------------------------------------ outils
@pytest.fixture(autouse=True)
def _fond_propre(xvfb):
    """Chaque test part sans fenetre du chien ni menu (le osampo-pc precedent est arrete)."""
    xvfb.workarea(xfaux.ZONE)
    attendre(lambda: not xvfb.visibles() and not xvfb.menus(), 5)
    yield
    xvfb.workarea(xfaux.ZONE)


def _chien(xvfb, timeout=5):
    return attendre(lambda: (xvfb.visibles() or [None])[0], timeout,
                    msg="fenetre osampo-pc visible")


def _anim(xvfb, w):
    try:
        return xvfb.anim(w)
    except Exception:
        return None


def _attendre_anim(xvfb, w, anim, timeout, msg="", pas=0.05):
    attendre(lambda: _anim(xvfb, w) == anim, timeout, pas=pas,
             msg=f"animation {anim!r} attendue ({msg}), vue : {_anim(xvfb, w)!r}")


def _geo(xvfb):
    """Geometrie de la fenetre visible du chien, None si elle n'y est pas."""
    try:
        v = xvfb.visibles()
        return xvfb.geometrie(v[0]) if v else None
    except Exception:
        return None


def _attendre_geo(xvfb, timeout, pred, msg):
    attendre(lambda: (lambda g: g is not None and pred(g))(_geo(xvfb)), timeout,
             msg=f"{msg} ; vu {_geo(xvfb)}")


def _fichier_position(m):
    return m.tmp / "state" / "osampo-pc" / "position.toml"


def _ecrire_position(m, texte):
    p = _fichier_position(m)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(texte)
    return p


def _cfg_pc(px):
    return config_toml(extra=f"[pc]\nchien_px = {px}\n")


def _jamais(pred, duree, msg):
    fin = time.monotonic() + duree
    while time.monotonic() < fin:
        assert not pred(), msg
        time.sleep(0.05)


def _pret(m):
    """Board au repos vue par le PC (sinon un geste peut arriver avant l'etat : course)."""
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 3)


# ============================================================ fenetre, presence
# REQ-011 : sans bord, hors barre des taches, au premier plan, sans focus clavier
def test_req011_proprietes_de_la_fenetre(monde, xvfb):
    actif_avant = xvfb.active()
    m = monde(pc=True)
    w = _chien(xvfb)
    assert tuple(w.get_wm_class()) == ("osampo-pc", "Osampo")
    assert w.get_wm_name() == "Osampo"
    motif = xvfb.propriete(w, "_MOTIF_WM_HINTS")
    assert motif and motif[2] == 0 and motif[0] & 2, f"_MOTIF_WM_HINTS : {motif}"
    etat = xvfb.etat_wm(w)
    assert {"_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER",
            "_NET_WM_STATE_ABOVE"} <= etat, etat
    assert xvfb.propriete(w, "_NET_WM_DESKTOP") == [0xFFFFFFFF]
    hints = w.get_wm_hints()
    assert hints is not None and hints.input == 0, f"WM_HINTS : {hints}"
    protos = {xvfb.d.get_atom_name(a) for a in (w.get_wm_protocols() or [])}
    assert "WM_TAKE_FOCUS" not in protos, protos
    g = xvfb.geometrie(w)
    assert g["w"] == g["h"] == 64, g
    assert xvfb.active() == actif_avant, "_NET_ACTIVE_WINDOW a change"


# REQ-011 : posé dans le coin bas droit de la zone de pose, son bas sur le haut de la barre
def test_req011_position_par_defaut(monde, xvfb):
    m = monde(pc=True)
    w = _chien(xvfb)
    _attendre_geo(xvfb, 2, lambda g: abs(g["x"] + g["w"] - (X0 + LARGEUR)) <= 2
                  and abs(g["y"] + g["h"] - BAS) <= 2, f"coin bas droit de {xfaux.ZONE}")
    assert xvfb.geometrie(w)["w"] == 64


# REQ-011 : le chien est dessiné (la fenêtre n'est pas d'un seul aplat)
def test_req011_le_chien_est_dessine(monde, xvfb):
    from Xlib import X
    m = monde(pc=True)
    m.board.etat(etat="repos")
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    time.sleep(0.5)
    g = xvfb.geometrie(w)
    img = w.get_image(0, 0, g["w"], g["h"], X.ZPixmap, 0xFFFFFFFF).data
    pixels = {bytes(img[i:i + 4]) for i in range(0, len(img) - 3, 4)}
    assert len(pixels) > 2, "la fenetre est un aplat : rien n'est dessine"


# REQ-011 : affichée avant la première ligne d'état et daemon injoignable : animation « repos »
def test_req011_daemon_injoignable_au_lancement(monde, xvfb):
    m = monde(pc="manuel", board=False)
    m.arreter_daemon()
    m.demarrer_pc()
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5, "daemon injoignable")
    _jamais(lambda: not xvfb.visibles(), 3, "le chien a disparu")


# REQ-011 : daemon perdu en cours de route : le chien reste, animation « repos »
def test_req011_daemon_perdu_le_chien_reste(monde, xvfb):
    m = monde(pc=True)
    m.board.etat(etat="dort")
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "dort", 5)
    m.arreter_daemon()
    _attendre_anim(xvfb, w, "repos", 6, "daemon injoignable")
    assert xvfb.visibles()


# ====================================================== animation selon l'état
ETATS = [({"etat": "repos"}, "repos"),
         ({"etat": "dort"}, "dort"),
         ({"etat": "promenade", "promenade_debut": "14:05"}, "laisse"),
         ({"etat": "reclame", "raison": "rappel", "rappel": "repas,1200", "pose": "aboie"}, "aboie"),
         ({"etat": "reclame", "raison": "rappel", "rappel": "sortie,1230", "pose": "laisse"},
          "reclame_laisse"),   # ADR-006 : la laisse dans la gueule, pas la laisse seule
         ({"etat": "reclame", "raison": "promenade", "pose": "laisse"}, "reclame_laisse"),
         ({"etat": "reclame", "raison": "claude", "pose": "patte"}, "patte"),
         ({"etat": "reclame", "raison": "claude", "pose": "aboie"}, "aboie")]  # apres le delai

# Board pas encore reflashee : pas de `pose` ou valeur inconnue, pose recalculee (SPEC-012)
SANS_POSE = [({"etat": "reclame", "raison": "claude"}, "patte"),
             ({"etat": "reclame", "raison": "rappel", "rappel": "dodo,1230"}, "reclame_laisse"),
             ({"etat": "reclame", "raison": "promenade", "pose": "?"}, "reclame_laisse")]


# REQ-011 : l'animation suit l'état du chiot en moins de 5 s, la fenêtre reste visible
@pytest.mark.parametrize("etat,anim", ETATS + SANS_POSE)
def test_req011_animation_suit_l_etat(monde, xvfb, etat, anim):
    m = monde(pc=True)
    m.board.etat(**etat)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, anim, 5, str(etat))
    assert xvfb.visibles(), "la fenetre ne doit jamais disparaitre"


# REQ-011 : enchaînement d'états, y compris la promenade (laisse seule) : jamais de disparition
def test_req011_enchainement_sans_disparition(monde, xvfb):
    m = monde(pc=True)
    w = _chien(xvfb)
    for etat, anim in ETATS + [({"etat": "repos"}, "repos")]:
        m.board.etat(**etat)
        _attendre_anim(xvfb, w, anim, 5, str(etat))
        assert xvfb.visibles(), f"disparu en {anim}"
        if etat.get("raison") == "rappel" and etat["rappel"].split(",")[0] != "repas":
            # SPEC-003 : le daemon adopte la sortie réclamée par la board ; seul un événement
            # y met fin (sinon « repos » pendant ce rappel donne « aboie », SPEC-002 règle 2)
            m.board.evenement("accepte", rappel=etat["rappel"])


# REQ-011 : ADR-004 / ADR-006, patte, laisse dans la gueule et aboiement : la pose de la board se
# copie ; l'une remplace l'autre sans repasser par « repos »
def test_req011_patte_laisse_et_aboiement_se_succedent_sans_repos(monde, xvfb):
    m = monde(pc=True)
    w = _chien(xvfb)
    m.board.etat(etat="reclame", raison="claude", pose="patte")
    _attendre_anim(xvfb, w, "patte", 5, "Claude attend")
    suite = [("claude", "-", "aboie", "aboie"),   # escalade, SPEC-012
             ("rappel", "sortie,1230", "laisse", "reclame_laisse"),
             ("claude", "-", "patte", "patte"),
             ("rappel", "repas,1200", "aboie", "aboie"),
             ("promenade", "-", "laisse", "reclame_laisse")]
    for raison, rappel, pose, anim in suite:
        vues = []
        m.board.etat(etat="reclame", raison=raison, rappel=rappel, pose=pose)

        def atteint():
            vues.append(_anim(xvfb, w))
            return vues[-1] == anim
        attendre(atteint, 5, pas=0.02, msg=f"{raison}/{pose} -> {anim}, vu {vues[-5:]}")
        assert "repos" not in vues, f"passage par repos entre deux reclamations : {vues}"
        assert xvfb.visibles()


# REQ-011 / REQ-012 : la patte puis l'aboiement, la réclamation traitée : retour à l'état du chiot
def test_req011_req012_patte_puis_aboie_puis_repos(monde, xvfb):
    m = monde(pc=True)
    w = _chien(xvfb)
    m.board.etat(etat="reclame", raison="claude", pose="patte")
    _attendre_anim(xvfb, w, "patte", 5)
    m.board.etat(etat="reclame", raison="claude", pose="aboie")
    _attendre_anim(xvfb, w, "aboie", 5, "la board a escaladé")
    m.board.etat(etat="repos")
    _attendre_anim(xvfb, w, "repos", 5, "réclamation traitée")
    assert xvfb.visibles()


# REQ-011 : « reclame_laisse » (laisse dans la gueule) n'est pas « laisse » (promenade : laisse seule)
def test_req011_reclame_laisse_n_est_pas_la_promenade(monde, xvfb):
    m = monde(pc=True)
    w = _chien(xvfb)
    m.board.etat(etat="promenade", promenade_debut="14:05")
    _attendre_anim(xvfb, w, "laisse", 5)
    m.board.etat(etat="reclame", raison="promenade", pose="laisse")
    _attendre_anim(xvfb, w, "reclame_laisse", 5)
    m.board.etat(etat="promenade", promenade_debut="14:05")
    _attendre_anim(xvfb, w, "laisse", 5)


# REQ-011 : ADR-006, board connectée et réclamant : le chien copie sa pose même pendant un rappel
# du daemon ; la board qui ne réclame pas (encore) : « aboie »
def test_req011_pose_de_la_board_pendant_le_rappel_du_daemon(monde, xvfb):
    h = hhmm(maintenant())
    m = monde(pc=True, config=config_toml([("sortie", h, None)]))
    w = _chien(xvfb)
    m.board.etat(etat="repos")
    _attendre_anim(xvfb, w, "aboie", 5, "rappel du daemon, board qui ne réclame pas")
    m.board.etat(etat="reclame", raison="rappel", rappel=id_rappel("sortie", h), pose="laisse")
    _attendre_anim(xvfb, w, "reclame_laisse", 5, "la board tient la laisse")
    _jamais(lambda: _anim(xvfb, w) != "reclame_laisse", 1.5, "le rappel ne prend pas le pas")
    m.client.commande("accepter", rappel=id_rappel("sortie", h))
    m.board.etat(etat="reclame", raison="claude", pose="patte")
    _attendre_anim(xvfb, w, "patte", 5, "rappel accepté, Claude attend toujours")
    assert xvfb.visibles()


# REQ-011 : pose « patte » de la board pendant un rappel du daemon : la patte (pose copiée)
def test_req011_patte_de_la_board_pendant_le_rappel(monde, xvfb):
    h = hhmm(maintenant())
    m = monde(pc=True, config=config_toml([("sortie", h, None)]))
    w = _chien(xvfb)
    m.board.etat(etat="reclame", raison="claude", pose="patte")
    _attendre_anim(xvfb, w, "patte", 5, "pose de la board copiée")
    _jamais(lambda: _anim(xvfb, w) != "patte", 1.5, "le rappel ne prend pas le pas")


# REQ-011 : os / chatouille faits sur la board : la fenêtre joue la réaction, sans son
@pytest.mark.parametrize("evt,anim", [("os", "os"), ("chatouille", "content"), ("bain", "bain")])
def test_req011_geste_de_la_board(monde, xvfb, evt, anim):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    m.board.evenement(evt, source="board")
    _attendre_anim(xvfb, w, anim, 5, f"geste {evt} sur la board", pas=0.02)
    _attendre_anim(xvfb, w, "repos", 15, "retour a l'etat du chiot")
    assert "son: jappe" not in m.stderr_pc(), "la board joue son son, pas le PC"


# REQ-011 : un geste de la board déjà passé ne se rejoue pas à la connexion du programme
def test_req011_pas_de_reaction_au_geste_passe(monde, xvfb):
    m = monde(pc="manuel")
    _pret(m)
    m.board.evenement("os", source="board")
    m.client.attendre_etat(lambda e: e["dernier_geste"] is not None, 3)
    m.demarrer_pc()
    w = _chien(xvfb)
    _jamais(lambda: _anim(xvfb, w) in ("os", "content"), 3,
            "reaction jouee pour un geste passe")


# REQ-011 : un geste de source « pc » (déjà joué par le PC qui l'a fait) ne déclenche rien ici
def test_req011_geste_source_pc_ne_declenche_rien(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    assert m.client.commande("chatouille")["ok"] is True
    _jamais(lambda: _anim(xvfb, w) in ("os", "content"), 2, "reaction pour un geste du PC")
    assert "son: jappe" not in m.stderr_pc()


# ============================================================ souris : clic gauche
# REQ-011 : clic gauche = chatouiller (commande à la board, réaction, son), sans focus clavier
def test_req011_clic_gauche_chatouille(monde, xvfb):
    actif_avant = xvfb.active()
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    xvfb.clic(*xvfb.centre(w))
    _attendre_anim(xvfb, w, "content", 3, "clic gauche", pas=0.02)
    m.board.attendre_commande(lambda c: c == {"cmd": "chatouille"}, 5)
    attendre(lambda: "son: jappe" in m.stderr_pc(), 3, msg=m.stderr_pc())
    _attendre_anim(xvfb, w, "repos", 15)
    assert xvfb.active() == actif_avant, "_NET_ACTIVE_WINDOW a change"


# REQ-011 : clic gauche, board hors ligne : réaction quand même et notification d'erreur
def test_req011_clic_gauche_board_hors_ligne(monde, xvfb):
    m = monde(pc=True, board=False)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    time.sleep(1)
    xvfb.clic(*xvfb.centre(w))
    _attendre_anim(xvfb, w, "content", 3, "reaction lien coupe", pas=0.02)
    n = attendre(lambda: m.faux_notif.recues, 3, msg="notification de lien coupe")[0]
    assert "Board hors ligne : chatouille non transmise" in n["summary"] + "\n" + n["body"]
    attendre(lambda: "son: jappe" in m.stderr_pc(), 3)


# REQ-011 : clic gauche quand le chiot dort : l'entrée serait grisée, le clic ne fait rien
def test_req011_clic_gauche_grise_ne_fait_rien(monde, xvfb):
    m = monde(pc=True)
    m.board.etat(etat="dort")
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "dort", 5)
    time.sleep(1)
    xvfb.clic(*xvfb.centre(w))
    time.sleep(1.5)
    assert m.board.commandes() == []
    assert _anim(xvfb, w) == "dort"
    assert "son: jappe" not in m.stderr_pc()


# ============================================================ souris : clic droit, menu
def _menu(m, xvfb, w, bouton_xy=None):
    """Clic droit sur le chien ; rend la fenetre du menu et ses entrees."""
    xvfb.clic(*(bouton_xy or xvfb.centre(w)), bouton=3)
    mw = attendre(lambda: (xvfb.menus() or [None])[0], 3, msg="menu du chien")
    attendre(lambda: xvfb.entrees_menu(mw), 3, msg="_OSAMPO_MENU")
    return mw, xvfb.entrees_menu(mw)


def _attendu_du_menu_de_l_icone(item):
    """(texte, actif) attendus d'apres le menu de l'icone (SPEC-004) : statuts, `-`, actions."""
    sortie, apres_sep = [], False
    for e in item.entrees():
        if e["separator"]:
            sortie.append(("-", False))
            apres_sep = True
        else:
            sortie.append((e["label"], bool(e["enabled"]) and apres_sep))
    return sortie


# REQ-011 : clic droit = le menu de l'icône : mêmes entrées, même ordre, mêmes textes et grisés
def test_req011_clic_droit_menu_identique_a_celui_de_l_icone(monde, xvfb):
    m = monde(pc=True)
    item = m.item()
    _pret(m)
    w = _chien(xvfb)
    attendre(lambda: any(e["label"] == "Chatouiller" for e in item.entrees()), 5)
    mw, entrees = _menu(m, xvfb, w)
    assert [(e["texte"], e["actif"]) for e in entrees] == _attendu_du_menu_de_l_icone(item)
    textes = [e["texte"] for e in entrees]
    i = textes.index("-")
    assert textes[i + 1:i + 3] == ["Chatouiller", "Donner un os"]
    assert "Remplir la jauge" in textes and "Vider la jauge" in textes
    assert textes[-2:] == ["Réglages…", "Quitter Osampo"]


# REQ-011 : le menu : sans bord, override-redirect, WM_CLASS OsampoMenu, entièrement à l'écran
def test_req011_menu_fenetre(monde, xvfb):
    actif_avant = xvfb.active()
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    mw, entrees = _menu(m, xvfb, w)
    assert tuple(mw.get_wm_class()) == ("osampo-pc", "OsampoMenu")
    assert mw.get_attributes().override_redirect == 1
    g = xvfb.geometrie(mw)
    assert g["x"] >= 0 and g["y"] >= 0, g
    assert g["x"] + g["w"] <= xfaux.LARGEUR and g["y"] + g["h"] <= xfaux.HAUTEUR, g
    assert all(0 <= e["y"] and e["y"] + e["h"] <= g["h"] for e in entrees)
    assert xvfb.active() == actif_avant, "_NET_ACTIVE_WINDOW a change"


def _clic_entree(xvfb, mw, e):
    g = xvfb.geometrie(mw)
    xvfb.clic(g["x"] + g["w"] // 2, g["y"] + e["y"] + e["h"] // 2)


# REQ-011 : une entrée active : son effet, puis le menu se ferme
@pytest.mark.parametrize("texte,cmd", [("Chatouiller", "chatouille"), ("Donner un os", "os"), ("Donner un bain", "bain")])
def test_req011_menu_entree_active(monde, xvfb, texte, cmd):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    mw, entrees = _menu(m, xvfb, w)
    e = next(e for e in entrees if e["texte"] == texte)
    assert e["actif"]
    _clic_entree(xvfb, mw, e)
    m.board.attendre_commande(lambda c: c == {"cmd": cmd}, 5)
    attendre(lambda: not xvfb.menus(), 2, msg="le menu doit se fermer")
    assert xvfb.visibles(), "le chien reste"


# REQ-011 : « Vider la jauge » depuis le menu du chien : même effet que depuis l'icône
def test_req011_menu_vider_la_jauge(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    mw, entrees = _menu(m, xvfb, w)
    _clic_entree(xvfb, mw, next(e for e in entrees if e["texte"] == "Vider la jauge"))
    m.client.attendre_etat(lambda e: e["sage"] == 0, 5, msg="jauge videe")
    attendre(lambda: not xvfb.menus(), 2)


# REQ-011 : un statut, le séparateur ou une entrée grisée : rien ne se passe
def test_req011_menu_entrees_inertes(monde, xvfb):
    m = monde(pc=True)
    m.board.etat(etat="dort")
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "dort", 5)
    time.sleep(1)
    mw, entrees = _menu(m, xvfb, w)
    inertes = [e for e in entrees if not e["actif"]]
    assert any(e["texte"] == "-" for e in inertes)
    assert any(e["texte"] == "Chatouiller" for e in inertes), "grisee quand le chiot dort"
    for e in inertes:
        if xvfb.menus():
            _clic_entree(xvfb, xvfb.menus()[0], e)
    time.sleep(1)
    assert m.board.commandes() == []


# REQ-011 : un clic hors du menu le ferme sans effet
def test_req011_menu_clic_hors_du_menu(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    mw, entrees = _menu(m, xvfb, w)
    xvfb.clic(5, 5)
    attendre(lambda: not xvfb.menus(), 2, msg="le menu doit se fermer")
    time.sleep(0.5)
    assert m.board.commandes() == []
    assert xvfb.visibles()


# ============================================================ souris : glisser-déposer
def _droite(xvfb):
    g = _geo(xvfb)
    return (X0 + LARGEUR) - (g["x"] + g["w"]) if g else None


# REQ-011 : glisser-déposer = déplacer le chien le long de la barre, son bas reste posé
def test_req011_glisser_deplace_le_chien(monde, xvfb):
    actif_avant = xvfb.active()
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    _attendre_geo(xvfb, 2, lambda g: abs(g["y"] + g["h"] - BAS) <= 2, "pose sur la barre")
    y0 = xvfb.geometrie(w)["y"]
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, cx - 100, cy)
    attendre(lambda: abs(_droite(xvfb) - 100) <= 6, 2,
             msg=f"droite_px ~ 100, vu {_droite(xvfb)}")
    assert _geo(xvfb)["y"] == y0, "le bas reste pose sur la barre"
    time.sleep(1)
    assert m.board.commandes() == [], "un glisser-deposer n'envoie aucune commande"
    assert xvfb.active() == actif_avant, "_NET_ACTIVE_WINDOW a change"


# REQ-011 : un déplacement de moins de 4 px est un clic (chatouille), pas un glissement
def test_req011_petit_deplacement_est_un_clic(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    _attendre_anim(xvfb, w, "repos", 5)
    time.sleep(1)
    avant = xvfb.geometrie(w)
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, cx - 3, cy, pas=1)
    m.board.attendre_commande(lambda c: c == {"cmd": "chatouille"}, 5)
    assert xvfb.geometrie(w) == avant


# REQ-011 : la position est enregistrée au relâchement (droite_px) et retenue au redémarrage
def test_req011_position_retenue(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, cx - 150, cy)
    f = _fichier_position(m)
    attendre(lambda: f.exists(), 3, msg=f"{f} ecrit au relachement")
    d = tomllib.loads(f.read_text())["droite_px"]
    assert isinstance(d, int) and abs(d - 150) <= 6, d
    # redemarrage du programme : meme position
    m.pc.terminate()
    m.pc.wait(5)
    attendre(lambda: not xvfb.visibles(), 5)
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: (X0 + LARGEUR) - (g["x"] + g["w"]) == d,
                  f"position retenue droite_px = {d}")


# REQ-011 : le glisser ramène droite_px dans la zone de pose (le chien reste entier)
def test_req011_glisser_borne_a_la_zone(monde, xvfb):
    m = monde(pc=True)
    _pret(m)
    w = _chien(xvfb)
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, 0, cy)            # bien au-dela du bord gauche
    attendre(lambda: _geo(xvfb) and abs(_geo(xvfb)["x"] - X0) <= 2, 2,
             msg=f"bord gauche de la zone, vu {_geo(xvfb)}")
    d = tomllib.loads(_fichier_position(m).read_text())["droite_px"]
    assert d == LARGEUR - 64
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, cx + 40, cy)       # revient de 40 px vers la droite
    attendre(lambda: abs(_droite(xvfb) - (LARGEUR - 64 - 40)) <= 6, 2,
             msg=f"droite_px ~ {LARGEUR - 104}, vu {_droite(xvfb)}")
    cx, cy = xvfb.centre(w)
    xvfb.glisser(cx, cy, xfaux.LARGEUR - 1, cy)   # bien au-dela du bord droit
    attendre(lambda: _droite(xvfb) == 0, 2, msg=f"bord droit de la zone, vu {_droite(xvfb)}")
    assert tomllib.loads(_fichier_position(m).read_text())["droite_px"] == 0


# ======================================================= position retenue : lecture
# REQ-011 : position lue au lancement
def test_req011_position_lue_au_lancement(monde, xvfb):
    m = monde(pc="manuel")
    _ecrire_position(m, "droite_px = 200\n")
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: (X0 + LARGEUR) - (g["x"] + g["w"]) == 200
                  and abs(g["y"] + g["h"] - BAS) <= 2, "droite_px = 200")


# REQ-011 : fichier absent, illisible ou négatif : retour au coin bas droit
@pytest.mark.parametrize("texte", [None, "n'importe quoi [", "droite_px = -30\n"],
                         ids=["absent", "illisible", "negatif"])
def test_req011_position_invalide_donne_le_coin(monde, xvfb, texte):
    m = monde(pc="manuel")
    if texte is not None:
        _ecrire_position(m, texte)
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: (X0 + LARGEUR) - (g["x"] + g["w"]) == 0, "droite_px = 0")


# REQ-011 : valeur trop grande : ramenée dans la zone, sans réécrire le fichier
def test_req011_position_trop_grande_ramenee(monde, xvfb):
    m = monde(pc="manuel")
    f = _ecrire_position(m, "droite_px = 5000\n")
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["x"] == X0, "chien entier, au bord gauche")
    assert f.read_text() == "droite_px = 5000\n", "le fichier n'est pas reecrit"


# ============================================================ zone de pose
# REQ-011 : _NET_WORKAREA change (barre déplacée, résolution) : reposé en moins de 2 s
def test_req011_zone_de_pose_changee(monde, xvfb):
    m = monde(pc="manuel")
    _ecrire_position(m, "droite_px = 50\n")
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: abs(g["y"] + g["h"] - BAS) <= 2, "pose initiale")
    xvfb.workarea((0, 0, 700, 500))
    _attendre_geo(xvfb, 2, lambda g: abs(g["y"] + g["h"] - 500) <= 2
                  and abs(g["x"] + g["w"] - (700 - 50)) <= 2,
                  "repose sur la nouvelle zone, droite_px gardee")
    xvfb.workarea(xfaux.ZONE)
    _attendre_geo(xvfb, 2, lambda g: abs(g["y"] + g["h"] - BAS) <= 2
                  and abs(g["x"] + g["w"] - (LARGEUR - 50)) <= 2, "retour a la zone initiale")


# ================================================================== taille
# REQ-011 : `[pc] chien_px` donne le côté de la fenêtre, posée sur la barre
@pytest.mark.parametrize("px", [32, 128, 256])
def test_req011_taille_au_lancement(monde, xvfb, px):
    m = monde(pc=True, config=_cfg_pc(px))
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["w"] == px and g["h"] == px
                  and abs(g["y"] + g["h"] - BAS) <= 2, f"fenetre de {px} px sur la barre")


# REQ-011 : fichier absent : 64 px
def test_req011_fichier_absent_64_px(monde, xvfb):
    m = monde(pc="manuel", board=False)
    m.arreter_daemon()
    m.cfg.unlink()
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["w"] == g["h"] == 64, "64 px par defaut")


# REQ-011 : fichier refusé (valeur hors bornes) : taille par défaut au lancement
@pytest.mark.parametrize("px", [300, 31])
def test_req011_taille_hors_bornes_gardee(monde, xvfb, px):
    m = monde(pc=True, config=_cfg_pc(px))
    _chien(xvfb)
    time.sleep(3)
    assert _geo(xvfb)["w"] == 64


# REQ-011 : nouvelle taille valide appliquée sans relancer, en moins de 5 s ; reposée sur la barre
def test_req011_nouvelle_taille_sans_relancer(monde, xvfb):
    m = monde(pc="manuel")
    _ecrire_position(m, "droite_px = 100\n")
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["w"] == 64, "64 px au depart")
    pid = m.pc.pid
    m.config(_cfg_pc(128))
    _attendre_geo(xvfb, 6, lambda g: g["w"] == g["h"] == 128
                  and abs(g["y"] + g["h"] - BAS) <= 2
                  and abs(g["x"] + g["w"] - (LARGEUR - 100)) <= 2,
                  "128 px, sur la barre, droite_px = 100 gardee")
    assert m.pc.poll() is None and m.pc.pid == pid, "le programme n'a pas ete relance"
    assert len(xvfb.visibles()) == 1


# REQ-011 : une taille qui ne tient plus à la position retenue : droite_px ramenée dans ses bornes
def test_req011_nouvelle_taille_ramene_la_position(monde, xvfb):
    m = monde(pc="manuel")
    _ecrire_position(m, f"droite_px = {LARGEUR - 64}\n")     # tout a gauche, avec 64 px
    m.demarrer_pc()
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["x"] == X0, "tout a gauche")
    m.config(_cfg_pc(128))
    _attendre_geo(xvfb, 6, lambda g: g["w"] == 128 and g["x"] == X0,
                  "128 px, toujours entier dans la zone")


# REQ-011 : un fichier refusé ensuite garde la taille en cours
def test_req011_fichier_refuse_garde_la_taille(monde, xvfb):
    m = monde(pc=True, config=_cfg_pc(96))
    _chien(xvfb)
    _attendre_geo(xvfb, 3, lambda g: g["w"] == 96, "96 px")
    m.config(_cfg_pc(300))
    time.sleep(5)
    assert _geo(xvfb)["w"] == 96
    m.config("[pc\nchien_px = ")
    time.sleep(4)
    assert _geo(xvfb)["w"] == 96


# REQ-011 : le daemon accepte la section [pc] et refuse la clé hors bornes (message exact)
def test_req011_le_daemon_accepte_pc_et_refuse_hors_bornes(monde):
    def refus(m):
        return [l for l in m.stderr_daemon().splitlines()
                if l.startswith("config:") and "refus" in l]
    m = monde(config=_cfg_pc(128))
    time.sleep(3)
    assert not refus(m), m.stderr_daemon()
    m.config(_cfg_pc(300))
    attendre(lambda: refus(m), 6, msg="refus du fichier hors bornes")
    assert "pc.chien_px: 300 hors de 32..256" in m.stderr_daemon()


# ============================================ réglages : clé, bornes, aide, modèle
@pytest.fixture
def conf(tmp_path):
    p = tmp_path / "osampo.toml"
    shutil.copy(MODELE, p)
    return p


def _lignes(bins, conf, *args):
    r = reglages(bins, conf, "--lister", *args)
    assert r.returncode == 0, r.stderr
    return {l.split("\t")[1]: l.split("\t") for l in r.stdout.splitlines() if l.strip()}


# REQ-011 : le modèle commenté a une section [pc] avec chien_px = 64 et un commentaire
def test_req011_modele_commente(conf):
    texte = MODELE.read_text()
    assert tomllib.loads(texte)["pc"]["chien_px"] == 64
    lignes = texte.splitlines()
    i = next(i for i, l in enumerate(lignes) if l.strip().startswith("chien_px"))
    assert lignes[i - 1].lstrip().startswith("#"), "un commentaire doit preceder chien_px"


# REQ-011 : --lister : onglet avancé, valeur par défaut, bornes affichées
def test_req011_reglages_lister(bins, conf):
    l = _lignes(bins, conf)["pc.chien_px"]
    assert l == ["avance", "pc.chien_px", "64", "32 à 256 px"]


# REQ-011 : libellé et aide exacts (SPEC-011, SPEC-010), même sans commentaires dans le fichier
def test_req011_reglages_libelle_et_aide(bins, conf, tmp_path):
    l = _lignes(bins, conf, "--aide")["pc.chien_px"]
    assert l[4] == "Taille du chien sur le bureau"
    assert l[5] == ("Hauteur et largeur, en pixels, du chien posé au-dessus de la barre des "
                    "tâches du PC ; plus haut, il est plus grand et cache plus de bureau, "
                    "plus bas, il est plus discret.")
    nu = tmp_path / "nu.toml"
    nu.write_text("[pc]\nchien_px = 100\n")
    n = _lignes(bins, nu, "--aide")["pc.chien_px"]
    assert n[2] == "100" and (n[4], n[5]) == (l[4], l[5])


# REQ-011 : hors bornes non enregistrable (message du daemon), valeurs limites enregistrées
@pytest.mark.parametrize("v", [31, 257, 300])
def test_req011_reglages_hors_bornes_refuse(bins, conf, v):
    avant = conf.read_bytes()
    r = reglages(bins, conf, "--set", f"pc.chien_px={v}")
    assert r.returncode == 2, r.stderr
    assert f"pc.chien_px: {v} hors de 32..256" in r.stderr
    assert conf.read_bytes() == avant


@pytest.mark.parametrize("v", [32, 96, 256])
def test_req011_reglages_valeur_enregistree(bins, conf, v):
    r = reglages(bins, conf, "--set", f"pc.chien_px={v}")
    assert r.returncode == 0, r.stderr
    assert tomllib.loads(conf.read_text())["pc"]["chien_px"] == v
