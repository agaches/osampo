"""REQ-009 : regler la jauge sage depuis le PC (SPEC-009, ADR-002).

Socket du daemon (`{"cmd":"jauge","valeur":N}`), trames recues par la faux board, menu de
l'icone et mode ligne de commande `osampo-reglages --jauge`. La fenetre Slint (cadre
« Jauge sage ») n'est pas pilotable ici : le mode `--jauge` envoie la meme commande.
"""
import os
import subprocess
import time

import pytest

from outils import RACINE, ClientPc, attendre


def _trames_sage(board):
    """Valeurs de `sage` des trames recues par la board, dans l'ordre."""
    return [o["sage"] for o in board.recus() if "sage" in o and "cmd" not in o]


def _attendre_trame(board, valeur, timeout=3.0):
    n = len(_trames_sage(board))
    attendre(lambda: valeur in _trames_sage(board)[n:], timeout,
             msg=f"trame avec sage={valeur} ; recu : {_trames_sage(board)[-5:]}")


# REQ-009 : remplir, vider, fixer : reponse ok, ligne d'etat a jour en moins de 2 s
@pytest.mark.parametrize("valeur", [0, 1, 42, 99, 100])
def test_req009_commande_jauge_ok(monde, valeur):
    m = monde()
    m.client.attendre_etat(lambda e: isinstance(e["sage"], int), 5)
    r = m.client.commande("jauge", valeur=valeur)
    assert r == {"reponse": "jauge", "ok": True}
    m.client.attendre_etat(lambda e: e["sage"] == valeur, 2.5, msg=f"sage == {valeur}")


# REQ-009 : la board recoit la nouvelle valeur dans ses trames en moins de 2 s
@pytest.mark.parametrize("valeur", [0, 42, 100])
def test_req009_la_board_recoit_la_valeur(monde, valeur):
    m = monde()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["board"] == "connectee", 5)
    # une valeur differente d'abord, pour que « recevoir » soit un vrai changement
    autre = 7 if valeur != 7 else 8
    assert m.client.commande("jauge", valeur=autre)["ok"] is True
    _attendre_trame(m.board, autre)
    assert m.client.commande("jauge", valeur=valeur)["ok"] is True
    _attendre_trame(m.board, valeur, 2.5)


# REQ-009 : une valeur hors bornes ou qui n'est pas un entier est refusee, la jauge ne change pas
@pytest.mark.parametrize("kw", [
    {"valeur": -1}, {"valeur": 101}, {"valeur": 1000}, {"valeur": 42.5},
    {"valeur": "42"}, {"valeur": None}, {"valeur": True}, {}])
def test_req009_valeur_refusee(monde, kw):
    m = monde()
    assert m.client.commande("jauge", valeur=55)["ok"] is True
    m.client.attendre_etat(lambda e: e["sage"] == 55, 2.5)
    r = m.client.commande("jauge", **kw)
    assert r == {"reponse": "jauge", "ok": False, "erreur": "valeur hors de 0..100"}
    time.sleep(1.5)
    assert abs(m.client.etat()["sage"] - 55) <= 1, "la jauge ne doit pas changer"
    # la connexion reste ouverte et accepte la commande suivante
    assert not m.client.ferme
    assert m.client.commande("jauge", valeur=60) == {"reponse": "jauge", "ok": True}


# REQ-009 : lien coupe : la commande s'applique au daemon seul, rien n'est mis en file
def test_req009_lien_coupe_rien_n_est_mis_en_file(monde):
    m = monde(board=False)
    r = m.client.commande("jauge", valeur=13)
    assert r == {"reponse": "jauge", "ok": True}
    e = m.client.attendre_etat(lambda e: e["sage"] == 13, 2.5)
    assert e["board"] == "hors_ligne"
    b = m.connecter_board()
    b.etat(etat="repos")
    # au retour du lien, la board recoit la jauge courante dans ses trames, comme d'habitude
    attendre(lambda: 13 in _trames_sage(b) or any(abs(v - 13) <= 1 for v in _trames_sage(b)),
             5, msg=f"trames apres retour du lien : {_trames_sage(b)[-5:]}")
    assert b.commandes() == [], "aucune commande nouvelle sur le lien (ni file)"


# REQ-009 : aucune commande nouvelle vers la board : seule la trame change
def test_req009_aucune_commande_vers_la_board(monde):
    m = monde()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["board"] == "connectee", 5)
    assert m.client.commande("jauge", valeur=0)["ok"] is True
    _attendre_trame(m.board, 0)
    assert m.board.commandes() == []


# REQ-009 : une jauge videe : la board reclame la promenade, le PC la voit (rappel_en_cours)
# La board est simulee : on joue sa reaction (SPEC-009, « Effet sur la board »).
def test_req009_jauge_videe_puis_remplie_suit_la_reclamation(monde):
    m = monde()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 3)
    assert m.client.commande("jauge", valeur=0)["ok"] is True
    _attendre_trame(m.board, 0)
    m.board.etat(etat="reclame", raison="promenade")
    e = m.client.attendre_etat(lambda e: e["rappel_en_cours"], 2)
    # ADR-006 : le chien copie la pose de la board, la laisse dans la gueule (pose recalculée : pas de `pose`)
    assert e["rappel_en_cours"]["id"] == "promenade" and e["icone"] == "reclame_laisse"
    assert m.client.commande("jauge", valeur=100)["ok"] is True
    _attendre_trame(m.board, 100)
    m.board.etat(etat="repos")
    e = m.client.attendre_etat(lambda e: e["rappel_en_cours"] is None, 2)
    assert e["sage"] == 100 or e["sage"] >= 99


# REQ-009 : ensuite la jauge reprend son evolution normale a partir de la valeur reglee
def test_req009_evolution_normale_a_partir_de_la_valeur(monde):
    m = monde()
    assert m.client.commande("jauge", valeur=40)["ok"] is True
    m.client.attendre_etat(lambda e: e["sage"] == 40, 2.5)
    time.sleep(4)
    s = m.client.etat()["sage"]
    # activite ou absence de quelques secondes : la jauge reste pres de 40 (pas de retour
    # a l'ancienne valeur, pas de reinitialisation a 100), vide_apres_min = 90 par defaut
    assert 38 <= s <= 42, f"sage = {s} apres un reglage a 40"


# REQ-009 : la valeur reglee survit a un redemarrage du daemon (a 1 point pres)
def test_req009_valeur_conservee_au_redemarrage(monde):
    m = monde()
    assert m.client.commande("jauge", valeur=37) == {"reponse": "jauge", "ok": True}
    m.arreter_daemon()   # tout de suite : sans attendre la sauvegarde periodique
    m.demarrer_daemon()
    c = ClientPc(m.sock)
    m.clients.append(c)
    e = c.attendre_etat(lambda e: 36 <= e["sage"] <= 38, 10,
                        msg="sage proche de 37 apres redemarrage")
    assert 36 <= e["sage"] <= 38


# REQ-009 : la valeur reglee est ecrite dans le fichier d'etat de la jauge, au moment du reglage
def test_req009_fichier_d_etat_ecrit_tout_de_suite(monde):
    m = monde()
    assert m.client.commande("jauge", valeur=21)["ok"] is True
    f = m.tmp / "state" / "osampo" / "jauge_osampo"
    attendre(lambda: f.exists() and abs(float(f.read_text().split()[0]) - 21) <= 1, 2,
             msg=f"fichier {f} avec 21 ; contenu : {f.read_text() if f.exists() else None}")


# ------------------------------------------------------------------ menu de l'icone
ENTREES = ["Chatouiller", "Donner un os", "Remplir la jauge", "Vider la jauge", "Réglages…",
           "Quitter Osampo"]


# REQ-009 : le menu propose Remplir et Vider la jauge entre « Donner un os » et « Réglages… »
def test_req009_menu_entrees_et_ordre(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    attendre(lambda: any(e["label"] == "Remplir la jauge" for e in item.entrees()), 5)
    e = item.entrees()
    i = next(i for i, x in enumerate(e) if x["separator"])
    assert [x["label"] for x in e[i + 1:]] == ENTREES
    assert all(x["enabled"] for x in e[i + 1:])


# REQ-009 : « Vider » puis « Remplir » depuis le menu : daemon, infobulle et board suivent
def test_req009_menu_vider_et_remplir(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["board"] == "connectee", 5)
    item.cliquer("Vider la jauge")
    m.client.attendre_etat(lambda e: e["sage"] == 0, 3)
    _attendre_trame(m.board, 0, 3)
    attendre(lambda: "Sage : 0 %" in item.tooltip()[1], 5, msg=f"infobulle : {item.tooltip()}")
    item.cliquer("Remplir la jauge")
    m.client.attendre_etat(lambda e: e["sage"] == 100, 3)
    _attendre_trame(m.board, 100, 3)
    attendre(lambda: "Sage : 100 %" in item.tooltip()[1], 5, msg=f"infobulle : {item.tooltip()}")


# REQ-009 : les entrees restent actives board coupee ou chiot endormi
@pytest.mark.parametrize("etat", ["repos", "dort", "promenade"])
def test_req009_menu_actif_quel_que_soit_l_etat(monde, etat):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat=etat)
    m.client.attendre_etat(lambda e: e["etat"] == etat, 3)

    def actives():
        d = {x["label"]: x for x in item.entrees()}
        return all(n in d and d[n]["enabled"] for n in ("Remplir la jauge", "Vider la jauge"))
    attendre(actives, 5, msg=f"entrees actives, etat {etat} : {item.entrees()}")


def test_req009_menu_actif_board_coupee(monde):
    m = monde(pc=True, board=False)
    item = m.item()
    m.client.attendre_etat(lambda e: e["board"] == "hors_ligne", 3)

    def actives():
        d = {x["label"]: x for x in item.entrees()}
        return all(n in d and d[n]["enabled"] for n in ("Remplir la jauge", "Vider la jauge"))
    attendre(actives, 5, msg=f"entrees actives board coupee : {item.entrees()}")
    item.cliquer("Vider la jauge")
    m.client.attendre_etat(lambda e: e["sage"] == 0, 3)


# REQ-009 : daemon injoignable : les deux entrees sont grisees
def test_req009_menu_grise_daemon_injoignable(monde):
    m = monde(pc=True)
    item = m.item()
    attendre(lambda: any(e["label"] == "Remplir la jauge" and e["enabled"]
                         for e in item.entrees()), 5)
    m.arreter_daemon()

    def grises():
        d = {x["label"]: x for x in item.entrees()}
        return all(n in d and not d[n]["enabled"] for n in ("Remplir la jauge", "Vider la jauge"))
    attendre(grises, 10, msg=f"entrees grisees daemon arrete : {item.entrees()}")


# ----------------------------------------------------------- mode ligne de commande
def _jauge(bins, args, env_extra=None, timeout=20):
    env = {**os.environ, **(env_extra or {})}
    return subprocess.run([bins["osampo-reglages"], *args], capture_output=True,
                          text=True, timeout=timeout, cwd=RACINE, env=env)


# REQ-009 : --jauge remplir | vider | entier : code 0, rien sur stdout ni stderr, jauge changee
@pytest.mark.parametrize("arg,attendu", [("remplir", 100), ("vider", 0), ("42", 42), ("0", 0),
                                         ("100", 100)])
def test_req009_cli_jauge_ok(bins, monde, arg, attendu):
    m = monde()
    m.client.attendre_etat(lambda e: isinstance(e["sage"], int), 5)
    if attendu == 42:
        m.client.commande("jauge", valeur=7)   # etat de depart different
    r = _jauge(bins, ["--socket", str(m.sock), "--jauge", arg])
    assert r.returncode == 0, r.stderr
    assert r.stdout == "" and r.stderr == ""
    m.client.attendre_etat(lambda e: e["sage"] == attendu, 3)


# REQ-009 : --jauge refuse (code 2, message, rien n'est envoye)
@pytest.mark.parametrize("arg", ["-1", "101", "abc", "42.5", "", "REMPLIR"])
def test_req009_cli_jauge_valeur_refusee(bins, monde, arg):
    m = monde()
    assert m.client.commande("jauge", valeur=55)["ok"] is True
    m.client.attendre_etat(lambda e: e["sage"] == 55, 2.5)
    r = _jauge(bins, ["--socket", str(m.sock), "--jauge", arg])
    assert r.returncode == 2, f"code {r.returncode}, stderr : {r.stderr!r}"
    assert "jauge: valeur hors de 0..100" in r.stderr
    time.sleep(1.5)
    assert abs(m.client.etat()["sage"] - 55) <= 1, "rien ne doit etre envoye"


# REQ-009 : daemon injoignable : code 1 et message
def test_req009_cli_jauge_daemon_injoignable(bins, tmp_path):
    r = _jauge(bins, ["--socket", str(tmp_path / "absent.sock"), "--jauge", "remplir"])
    assert r.returncode == 1, f"code {r.returncode}, stderr : {r.stderr!r}"
    assert "jauge: daemon injoignable" in r.stderr


# REQ-009 : sans --socket, le socket par defaut du daemon ($XDG_RUNTIME_DIR/osampo.sock)
def test_req009_cli_jauge_socket_par_defaut(bins, monde):
    m = monde()
    os.symlink(m.sock, m.run / "osampo.sock")
    m.client.attendre_etat(lambda e: isinstance(e["sage"], int), 5)
    r = _jauge(bins, ["--jauge", "vider"], {"XDG_RUNTIME_DIR": str(m.run)})
    assert r.returncode == 0, r.stderr
    m.client.attendre_etat(lambda e: e["sage"] == 0, 3)


# REQ-009 : --jauge n'ecrit pas osampo.toml
def test_req009_cli_jauge_n_ecrit_pas_le_fichier(bins, monde):
    m = monde()
    avant = m.cfg.read_bytes()
    r = _jauge(bins, ["--config", str(m.cfg), "--socket", str(m.sock), "--jauge", "vider"])
    assert r.returncode == 0, r.stderr
    m.client.attendre_etat(lambda e: e["sage"] == 0, 3)
    assert m.cfg.read_bytes() == avant
