// Les assert doivent rester actifs, meme en Release.
#undef NDEBUG
#include <cassert>
#include <string>

#include "logic/lien_pc.h"

namespace {

using pup::Category;
using pup::Commande;
using pup::EtatBoard;
using pup::Evenement;
using pup::EvtKind;
using pup::Reminder;

void ligne_d_etat_exacte()
{
    EtatBoard e;
    e.pile = 83;
    e.bouger = e.jouer = e.dormir = 70;
    assert(pup::ligne_etat(e) ==
           "{\"etat\":\"repos\",\"raison\":\"-\",\"pose\":\"-\",\"rappel\":\"-\",\"promenade_debut\":\"-\",\"pile\":83,"
           "\"besoins\":{\"bouger\":70,\"jouer\":70,\"dormir\":70}}");
    e.etat = "reclame";
    e.raison = "rappel";
    e.pose = "laisse";
    e.rappel = "sortie,1230";
    e.promenade_debut = "12:35";
    e.pile = -1;
    e.bouger = 0;
    e.dormir = 100;
    assert(pup::ligne_etat(e) ==
           "{\"etat\":\"reclame\",\"raison\":\"rappel\",\"pose\":\"laisse\",\"rappel\":\"sortie,1230\",\"promenade_debut\":\"12:35\","
           "\"pile\":-1,\"besoins\":{\"bouger\":0,\"jouer\":70,\"dormir\":100}}");
}

void id_du_rappel()
{
    assert(pup::id_rappel(Reminder{Category::Sortie, 12 * 60 + 30, 0x7F}) == "sortie,1230");
    assert(pup::id_rappel(Reminder{Category::Dodo, 5, 0x7F}) == "dodo,0005");
    assert(pup::id_rappel(Reminder{Category::Sport, 18 * 60, 1}) == "sport,1800");
    assert(pup::id_rappel(Reminder{Category::Repas, 19 * 60 + 30, 1}) == "repas,1930");
}

void lignes_d_evenement()
{
    assert(pup::ligne_evenement({EvtKind::Os, "board", ""}) == "{\"evt\":\"os\",\"source\":\"board\"}");
    assert(pup::ligne_evenement({EvtKind::Chatouille, "pc", ""}) == "{\"evt\":\"chatouille\",\"source\":\"pc\"}");
    assert(pup::ligne_evenement({EvtKind::Bain, "board", ""}) == "{\"evt\":\"bain\",\"source\":\"board\"}");
    assert(pup::ligne_evenement({EvtKind::Accepte, "", "sortie,1230"}) ==
           "{\"evt\":\"accepte\",\"rappel\":\"sortie,1230\"}");
    assert(pup::ligne_evenement({EvtKind::Reporte, "", "promenade"}) ==
           "{\"evt\":\"reporte\",\"rappel\":\"promenade\"}");
    assert(pup::ligne_evenement({EvtKind::Abandonne, "", "dodo,2145"}) ==
           "{\"evt\":\"abandonne\",\"rappel\":\"dodo,2145\"}");
}

void lecture_des_commandes()
{
    auto c = pup::lire_commande("{\"cmd\":\"chatouille\"}");
    assert(c && c->kind == Commande::Kind::Chatouille);
    c = pup::lire_commande("{\"cmd\":\"os\"}");
    assert(c && c->kind == Commande::Kind::Os);
    c = pup::lire_commande("{\"cmd\":\"bain\"}");
    assert(c && c->kind == Commande::Kind::Bain);
    c = pup::lire_commande("{\"cmd\":\"accepter\",\"rappel\":\"sortie,1230\"}");
    assert(c && c->kind == Commande::Kind::Accepter && c->rappel == "sortie,1230");
    c = pup::lire_commande("{\"cmd\":\"reporter\",\"rappel\":\"promenade\"}");
    assert(c && c->kind == Commande::Kind::Reporter && c->rappel == "promenade");

    assert(!pup::lire_commande("{\"cmd\":\"accepter\"}"));          // rappel manquant
    assert(!pup::lire_commande("{\"cmd\":\"brosse\"}"));            // inconnue
    assert(!pup::lire_commande("{\"cfg\":1}"));                     // pas une commande
    assert(!pup::lire_commande("pas du json"));
    assert(!pup::lire_commande(""));

    assert(pup::est_ligne_commande("{\"cmd\":\"brosse\"}"));
    assert(pup::est_ligne_commande("{\"cmd\":\"os\"}"));
    assert(!pup::est_ligne_commande("{\"sage\":0}"));
    assert(!pup::est_ligne_commande("n'importe quoi"));
}

void pile_en_pourcent()
{
    assert(pup::pile_pourcent(-1) == -1);
    assert(pup::pile_pourcent(0) == -1);
    assert(pup::pile_pourcent(3000) == 0);
    assert(pup::pile_pourcent(3300) == 0);
    assert(pup::pile_pourcent(3500) == 5);
    assert(pup::pile_pourcent(3800) == 50);
    assert(pup::pile_pourcent(3950) == 77); // 70 + 50 * 15 / 100
    assert(pup::pile_pourcent(4200) == 100);
    assert(pup::pile_pourcent(4500) == 100);
    // Monotone sur toute la plage.
    int prec = 0;
    for (int mv = 3000; mv <= 4400; ++mv) {
        const int p = pup::pile_pourcent(mv);
        assert(p >= prec && p >= 0 && p <= 100);
        prec = p;
    }
}

void emission_au_changement_et_toutes_les_30_s()
{
    pup::Emetteur em;
    EtatBoard e;
    assert(em.a_emettre(e, 100));       // premier appel
    assert(!em.a_emettre(e, 101));
    assert(!em.a_emettre(e, 129));
    assert(em.a_emettre(e, 130));       // 30 s apres la derniere emission
    assert(!em.a_emettre(e, 159));
    e.pile = 50;
    assert(em.a_emettre(e, 140));       // changement
    assert(!em.a_emettre(e, 141));
    e.jouer = 90;
    assert(em.a_emettre(e, 142));
    assert(em.a_emettre(e, 172));       // 30 s depuis 142
    em.forcer();
    assert(em.a_emettre(e, 173));       // nouvel abonne
    assert(!em.a_emettre(e, 174));
    // Un changement de pose seul fait emettre (patte -> aboie, SPEC-012).
    {
        pup::Emetteur x;
        EtatBoard a;
        a.etat = "reclame";
        a.raison = "claude";
        a.pose = "patte";
        assert(x.a_emettre(a, 0));
        a.pose = "aboie";
        assert(x.a_emettre(a, 1));
        assert(!x.a_emettre(a, 2));
    }
    // Chaque champ compte.
    for (int i = 0; i < 8; ++i) {
        EtatBoard f = e;
        switch (i) {
        case 0: f.etat = "dort"; break;
        case 1: f.raison = "claude"; break;
        case 2: f.rappel = "dodo,2145"; break;
        case 3: f.promenade_debut = "10:00"; break;
        case 4: f.bouger = 1; break;
        case 5: f.dormir = 1; break;
        case 6: f.pose = "aboie"; break;
        default: f.pile = 51; break;
        }
        pup::Emetteur x;
        x.a_emettre(e, 0);
        assert(x.a_emettre(f, 1));
    }
}

} // namespace

int main()
{
    ligne_d_etat_exacte();
    id_du_rappel();
    lignes_d_evenement();
    lecture_des_commandes();
    pile_en_pourcent();
    emission_au_changement_et_toutes_les_30_s();
    return 0;
}
