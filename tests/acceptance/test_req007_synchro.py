"""REQ-007 : synchro de la board vers le PC (SPEC-001, SPEC-002, ADR-001)."""
import re
import time

import pytest

from outils import attendre, config_toml, hhmm, id_rappel, ligne_etat, maintenant


# REQ-007 : un os ou une chatouille faits sur la board se reflètent sur le PC en moins de 5 s
@pytest.mark.parametrize("evt", ["os", "chatouille", "bain"])
def test_req007_geste_de_la_board(monde, evt):
    m = monde()
    m.board.evenement(evt, source="board")
    e = m.client.attendre_etat(lambda e: e["dernier_geste"] is not None, 5)
    g = e["dernier_geste"]
    assert g["geste"] == evt and g["source"] == "board" and re.fullmatch(r"\d\d:\d\d", g["heure"])


# REQ-007 : dodo, reveil, promenade commencee puis finie : etat et icone suivent
def test_req007_dodo_reveil_promenade(monde):
    m = monde()
    m.board.etat(etat="dort")
    m.client.attendre_etat(lambda e: e["etat"] == "dort" and e["icone"] == "niche", 5)
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos" and e["icone"] == "assis", 5)
    m.board.etat(etat="promenade", promenade_debut="14:05")
    e = m.client.attendre_etat(lambda e: e["etat"] == "promenade", 5)
    assert e["promenade_debut"] == "14:05" and e["icone"] == "laisse"
    m.board.etat(etat="content")
    e = m.client.attendre_etat(lambda e: e["etat"] == "content", 5)
    assert e["promenade_debut"] == "-" and e["icone"] == "assis"


# REQ-007 : besoins et pile remontent
def test_req007_besoins_et_pile(monde):
    m = monde()
    m.board.etat(pile=41, besoins={"bouger": 10, "jouer": 20, "dormir": 30})
    e = m.client.attendre_etat(lambda e: e["pile"] == 41, 5)
    assert e["besoins"] == {"bouger": 10, "jouer": 20, "dormir": 30}


# REQ-007 : au demarrage du daemon, rien recu : etat « - », besoins nuls, icone assis
def test_req007_rien_recu_depuis_le_demarrage(monde):
    m = monde(board=False)
    e = m.client.attendre_etat(lambda e: True, 5)
    assert e["etat"] == "-" and e["besoins"] is None and e["icone"] == "assis"
    assert e["board"] == "hors_ligne" and e["pile"] == -1
    assert e["dernier_geste"] is None and e["rappel_en_cours"] is None


# REQ-007 : le PC reflete l'etat dans l'infobulle (promenade, besoins, pile) en moins de 5 s
def test_req007_statuts_du_pc_suivent_la_board(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="promenade", promenade_debut="14:05", pile=61,
                 besoins={"bouger": 5, "jouer": 6, "dormir": 7})

    def ok():
        t = item.tooltip()[1]
        return ("Promenade : oui, depuis 14:05" in t and "Pile : 61 %" in t
                and "Besoins : bouger 5 %, jouer 6 %, dormir 7 %" in t)
    attendre(ok, 5, msg=item.tooltip()[1])


# REQ-007 : lien coupe : le PC garde le dernier etat de la board et indique « hors ligne » en moins de 1 s
def test_req007_lien_coupe_garde_le_dernier_etat(monde):
    m = monde()
    m.board.etat(etat="promenade", promenade_debut="14:05", pile=83,
                 besoins={"bouger": 11, "jouer": 22, "dormir": 33})
    m.client.attendre_etat(lambda e: e["etat"] == "promenade", 5)
    m.couper_board()
    e = m.client.attendre_etat(lambda e: e["board"] == "hors_ligne", 1.5)
    assert e["pile"] == -1
    assert e["etat"] == "promenade" and e["promenade_debut"] == "14:05"
    assert e["icone"] == "laisse"
    assert e["besoins"] == {"bouger": 11, "jouer": 22, "dormir": 33}


# REQ-007 : lien coupe, l'icone SNI reste celle du dernier etat, avec la mention hors ligne
def test_req007_icone_sni_hors_ligne(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="promenade", promenade_debut="14:05")
    attendre(lambda: str(item.get("IconName")) == "osampo-laisse", 5)
    m.couper_board()
    attendre(lambda: item.tooltip()[0] == "Osampo : board hors ligne", 5)
    assert str(item.get("IconName")) == "osampo-laisse"
    assert "Promenade : oui, depuis 14:05" in item.tooltip()[1]


# REQ-007 : lien coupe + reclamation du daemon en cours : le PC aboie ; une fois acceptee, il revient a l'etat garde
def test_req007_hors_ligne_le_rappel_du_daemon_fait_aboyer(monde):
    cfg = config_toml([("sortie", hhmm(maintenant()), None)])
    m = monde(pc=True, board=False, config=cfg)
    item = m.item()
    attendre(lambda: str(item.get("IconName")) == "osampo-aboie", 5)
    rid = m.client.attendre_etat(lambda e: e["rappel_en_cours"], 3)["rappel_en_cours"]["id"]
    r = m.client.commande("accepter", rappel=rid)
    assert r["ok"] is True
    attendre(lambda: str(item.get("IconName")) == "osampo-assis", 3)


# REQ-007 : le daemon ne calcule rien a la place de la board : un dodo hors ligne n'apparait qu'au retour du lien
def test_req007_etat_remplace_au_retour_du_lien(monde):
    m = monde()
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["etat"] == "repos", 5)
    m.couper_board()
    m.client.attendre_etat(lambda e: e["board"] == "hors_ligne", 2)
    time.sleep(3)
    assert m.client.etat()["etat"] == "repos" and m.client.etat()["icone"] == "assis"
    b = m.connecter_board()
    b.etat(etat="dort", pile=77)
    e = m.client.attendre_etat(lambda e: e["etat"] == "dort", 5)
    assert e["board"] == "connectee" and e["pile"] == 77 and e["icone"] == "niche"


# REQ-007 : une reclamation de la jauge perdue avec le lien n'est pas gardee (le PC n'aboie pas sans fin)
def test_req007_reclamation_de_la_jauge_coupee_avec_le_lien(monde):
    m = monde()
    m.board.etat(etat="reclame", raison="promenade")
    m.client.attendre_etat(lambda e: e["rappel_en_cours"], 5)
    m.couper_board()
    e = m.client.attendre_etat(lambda e: e["board"] == "hors_ligne"
                               and e["rappel_en_cours"] is None, 3)
    assert e["icone"] == "assis"


# REQ-007 : la reclamation de la jauge finie sans evenement : fin de la reclamation
def test_req007_jauge_remontee_sans_evenement(monde):
    m = monde()
    m.board.etat(etat="reclame", raison="promenade")
    m.client.attendre_etat(lambda e: e["rappel_en_cours"], 5)
    m.board.etat(etat="repos")
    m.client.attendre_etat(lambda e: e["rappel_en_cours"] is None, 2)


# REQ-007 : un rappel repas reclame par la board ne fait pas de reclamation de sortie
def test_req007_repas_reclame_par_la_board(monde):
    m = monde()
    m.board.etat(etat="reclame", raison="rappel", rappel=id_rappel("repas", "12:00"))
    time.sleep(2)
    assert m.client.etat()["rappel_en_cours"] is None


# REQ-007 : une ligne de plus de 4096 octets est jetee, sans couper le lien
def test_req007_ligne_trop_longue_jetee(monde):
    m = monde()
    m.board.sock.sendall(b'{"etat":"dort","raison":"' + b"x" * 5000 + b'"}\n')
    time.sleep(1)
    assert m.client.etat()["etat"] != "dort"
    m.board.etat(etat="dort")
    m.client.attendre_etat(lambda e: e["etat"] == "dort" and e["board"] == "connectee", 5)


# REQ-007 : un second client du faux BLE est refuse, le premier garde le lien
def test_req007_un_seul_client_faux_ble(monde):
    from outils import FausseBoard
    m = monde()
    m.board.etat(etat="dort")
    m.client.attendre_etat(lambda e: e["etat"] == "dort", 5)
    second = FausseBoard(m.ble)
    attendre(lambda: second.ferme, 3, msg="le second client doit etre ferme aussitot")
    assert m.client.etat()["board"] == "connectee"


# REQ-007 : le changement d'un champ est dans la ligne d'etat du daemon en moins de 1 s ; sinon toutes les 10 s
def test_req007_ligne_d_etat_periodique(monde):
    m = monde()
    m.client.attendre_etat(lambda e: True, 3)
    n = len([o for o in m.client.recus() if "pc" in o])
    time.sleep(11)
    assert len([o for o in m.client.recus() if "pc" in o]) > n
    m.board.etat(etat="dort")
    m.client.attendre_etat(lambda e: e["etat"] == "dort", 1.5)
