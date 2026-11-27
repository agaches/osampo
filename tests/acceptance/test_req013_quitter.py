"""REQ-013 : quitter Osampo depuis le menu de l'icone : daemon puis osampo-pc s'arretent."""
from outils import attendre


# REQ-013 : « Quitter Osampo » en dernier, toujours cliquable
def test_req013_quitter_en_dernier_et_actif(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    attendre(lambda: any(e["label"] == "Quitter Osampo" for e in item.entrees()), 5)
    dernier = item.entrees()[-1]
    assert dernier["label"] == "Quitter Osampo" and dernier["enabled"]


# REQ-013 : le clic arrete le daemon (code 0, socket supprime) puis osampo-pc
def test_req013_quitter_arrete_daemon_et_pc(monde):
    m = monde(pc=True)
    item = m.item()
    m.board.etat(etat="repos")
    attendre(lambda: any(e["label"] == "Quitter Osampo" for e in item.entrees()), 5)
    item.cliquer("Quitter Osampo")
    attendre(lambda: m.daemon.poll() is not None, 5, msg="arret du daemon")
    assert m.daemon.returncode == 0, m.stderr_daemon()
    assert not m.sock.exists()
    attendre(lambda: m.pc.poll() is not None, 5, msg="arret de osampo-pc")
    assert m.pc.returncode == 0, m.stderr_pc()


# REQ-013 : daemon injoignable : osampo-pc quitte quand meme
def test_req013_quitter_sans_daemon(monde):
    m = monde(pc=True)
    item = m.item()
    m.arreter_daemon()
    attendre(lambda: any(e["label"] == "Quitter Osampo" and e["enabled"]
                         for e in item.entrees()), 5)
    item.cliquer("Quitter Osampo")
    attendre(lambda: m.pc.poll() is not None, 5, msg="arret de osampo-pc")
    assert m.pc.returncode == 0, m.stderr_pc()
