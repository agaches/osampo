"""REQ-006 : chatouiller et donner un os depuis le PC (SPEC-001, SPEC-002, SPEC-004, SPEC-006, SPEC-008)."""
import re
import time

import pytest

from outils import attendre, config_toml, hhmm, maintenant

GESTES = [("chatouille", "Chatouiller"), ("os", "Donner un os")]


# REQ-006 : la commande est relayee a la board, qui rejoue le meme geste, en moins de 5 s
@pytest.mark.parametrize("cmd,_label", GESTES)
def test_req006_commande_relayee_a_la_board(monde, cmd, _label):
    m = monde()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 3)
    r = m.client.commande(cmd)
    assert r == {"reponse": cmd, "ok": True}
    m.board.attendre_commande(lambda c: c == {"cmd": cmd}, 5)


# REQ-006 : dernier_geste porte la source « pc » et l'heure
@pytest.mark.parametrize("cmd,_label", GESTES)
def test_req006_dernier_geste_source_pc(monde, cmd, _label):
    m = monde()
    m.board.etat(etat="repos")
    # l'etat de la board et la commande passent par deux sockets distincts : sans cette
    # attente, la commande peut arriver avant « repos » et etre refusee (course)
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 3)
    assert m.client.commande(cmd) == {"reponse": cmd, "ok": True}
    e = m.client.attendre_etat(lambda e: e["dernier_geste"] is not None, 3)
    g = e["dernier_geste"]
    assert g["geste"] == cmd and g["source"] == "pc" and re.fullmatch(r"\d\d:\d\d", g["heure"])


# REQ-006 : lien coupe, le PC le signale et la board ne recoit rien (ni maintenant, ni au retour)
@pytest.mark.parametrize("cmd,_label", GESTES)
def test_req006_lien_coupe_rien_n_est_envoye(monde, cmd, _label):
    m = monde(board=False)
    r = m.client.commande(cmd)
    assert r == {"reponse": cmd, "ok": False, "erreur": "board hors ligne"}
    b = m.connecter_board()
    b.etat(etat="repos")
    time.sleep(2)
    assert b.commandes() == [], "rien ne doit etre mis en file"


# REQ-006 : un chiot qui dort, reclame ou est en promenade ne joue pas
@pytest.mark.parametrize("etat", ["dort", "reclame", "promenade"])
@pytest.mark.parametrize("cmd,_label", GESTES)
def test_req006_le_chiot_ne_peut_pas_jouer(monde, cmd, _label, etat):
    m = monde()
    m.board.etat(etat=etat, raison="claude" if etat == "reclame" else "-")
    m.client.attendre_etat(lambda e: e["etat"] == etat, 3)
    r = m.client.commande(cmd)
    assert r == {"reponse": cmd, "ok": False, "erreur": "le chiot ne peut pas jouer maintenant"}
    time.sleep(1)
    assert m.board.commandes() == []


# REQ-006 : commande inconnue ou illisible : reponse d'erreur, connexion ouverte
def test_req006_commande_inconnue(monde):
    m = monde()
    r = m.client.commande("brosse")
    assert r == {"reponse": "?", "ok": False, "erreur": "commande inconnue"}
    m.client.sock.sendall(b"pas du json\n")
    time.sleep(0.5)
    assert not m.client.ferme
    assert m.client.commande("chatouille")["ok"] is True


# REQ-006 : menu de l'icone : statuts, separateur, puis Chatouiller, Donner un os, ..., Réglages…
# (REQ-013 ajoute « Quitter Osampo » en dernier)
# (SPEC-004 ; SPEC-009 insere les entrees de la jauge avant Réglages…, verifiees par REQ-009)
def test_req006_menu_de_l_icone(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    attendre(lambda: any(e["label"] == "Chatouiller" for e in item.entrees()), 5)
    e = item.entrees()
    labels = [x["label"] for x in e]
    i = next(i for i, x in enumerate(e) if x["separator"])
    actions = e[i + 1:]
    noms = [x["label"] for x in actions]
    assert noms[:2] == ["Chatouiller", "Donner un os"], labels
    assert noms[-2:] == ["Réglages…", "Quitter Osampo"], labels
    assert all(x["enabled"] for x in actions)


# REQ-006 : depuis le menu : commande a la board, reaction du chien du PC (son jappe, fenetre)
@pytest.mark.parametrize("cmd,label", GESTES)
def test_req006_menu_envoie_la_commande_et_le_chien_reagit(monde, xvfb, cmd, label):
    attendre(lambda: not xvfb.visibles(), 5)
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 3)
    reaction = {"chatouille": "content", "os": "os"}[cmd]
    w = attendre(lambda: (xvfb.visibles() or [None])[0], 3, msg="fenetre du chien (permanente)")
    item.cliquer(label)
    attendre(lambda: xvfb.anim(w) == reaction, 3, pas=0.02,
             msg=f"le chien joue {reaction}, vu {xvfb.anim(w)!r}")
    m.board.attendre_commande(lambda c: c == {"cmd": cmd}, 5)
    attendre(lambda: "son: jappe" in m.stderr_pc(), 3, msg=m.stderr_pc())
    attendre(lambda: xvfb.anim(w) == "repos", 15, msg="puis il revient a l'etat du chiot")
    assert xvfb.visibles(), "le chien ne disparait pas"


# REQ-006 : lien coupe : la reaction joue quand meme, une notification signale que rien n'est transmis
@pytest.mark.parametrize("cmd,label,texte", [
    ("chatouille", "Chatouiller", "Board hors ligne : chatouille non transmise"),
    ("os", "Donner un os", "Board hors ligne : os non transmis")])
def test_req006_menu_lien_coupe(monde, xvfb, cmd, label, texte):
    attendre(lambda: not xvfb.visibles(), 5)
    m = monde(pc=True, board=False)
    item = m.item()
    attendre(lambda: any(e["label"] == label and e["enabled"] for e in item.entrees()), 5)
    reaction = {"chatouille": "content", "os": "os"}[cmd]
    w = attendre(lambda: (xvfb.visibles() or [None])[0], 3, msg="fenetre du chien (permanente)")
    item.cliquer(label)
    attendre(lambda: xvfb.anim(w) == reaction, 3, pas=0.02,
             msg=f"le chien joue {reaction} meme lien coupe, vu {xvfb.anim(w)!r}")
    n = attendre(lambda: m.faux_notif.recues, 3, msg="notification de lien coupe")[0]
    assert texte in (n["summary"] + "\n" + n["body"])
    attendre(lambda: "son: jappe" in m.stderr_pc(), 3)


# REQ-006 : entrees grisees quand le chiot ne peut pas jouer (board connectee)
@pytest.mark.parametrize("etat", ["dort", "reclame", "promenade"])
def test_req006_entrees_grisees(monde, etat):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat=etat, raison="claude" if etat == "reclame" else "-")

    def grises():
        e = {x["label"]: x for x in item.entrees()}
        return "Chatouiller" in e and not e["Chatouiller"]["enabled"] \
            and not e["Donner un os"]["enabled"]
    attendre(grises, 5, msg=f"entrees grisees en {etat}")


# REQ-006 : entrees grisees pendant un rappel (la fenetre est prise)
def test_req006_entrees_grisees_pendant_un_rappel(monde):
    cfg = config_toml([("sortie", hhmm(maintenant()), None)])
    m = monde(pc=True, config=cfg)
    item = m.item()

    def grises():
        e = {x["label"]: x for x in item.entrees()}
        return "Chatouiller" in e and not e["Chatouiller"]["enabled"]
    attendre(grises, 5)
