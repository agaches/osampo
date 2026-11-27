#pragma once
#include <cstdint>
#include <optional>
#include <string>

#include "logic/pup.h"
#include "logic/rappel.h"

namespace pup {

// Lien board <-> PC (SPEC-001). Logique pure : ni Slint, ni ESP-IDF, ni BLE.
// Les lignes rendues n'ont pas de '\n' final : le transport l'ajoute et decoupe en morceaux de 20 octets.

// Champs de la ligne d'etat.
struct EtatBoard {
    std::string etat = "repos";         // etat du chiot, minuscules
    std::string raison = "-";           // "rappel" "promenade" "claude" en reclamation, sinon "-"
    std::string pose = "-";             // "patte" "laisse" "aboie" en reclamation, sinon "-" (SPEC-012)
    std::string rappel = "-";           // "categorie,HHMM" du rappel reclame ou reporte, sinon "-"
    std::string promenade_debut = "-";  // "HH:MM" de la promenade en cours, sinon "-"
    int pile = -1;                      // 0..100, -1 inconnue
    int bouger = 0, jouer = 0, dormir = 0; // 0..100
};
bool operator==(const EtatBoard &a, const EtatBoard &b);

// Evenement a emettre. source : "board" ou "pc" pour Os / Chatouille / Bain, vide sinon.
// rappel : id "categorie,HHMM" ou "promenade" pour Accepte / Reporte ; id pour Abandonne ; vide sinon.
struct Evenement {
    EvtKind kind;
    std::string source;
    std::string rappel;
};

// Commande recue du PC.
struct Commande {
    enum class Kind { Chatouille, Os, Bain, Accepter, Reporter };
    Kind kind;
    std::string rappel; // id "categorie,HHMM" ou "promenade" ; vide pour Chatouille / Os / Bain
};

// "categorie,HHMM" (ex. "sortie,1230").
std::string id_rappel(const Reminder &r);

std::string ligne_etat(const EtatBoard &e);
std::string ligne_evenement(const Evenement &e);

// true si la ligne est un objet JSON portant la cle "cmd" (valide ou non).
bool est_ligne_commande(const std::string &ligne);
// nullopt si la ligne n'est pas une commande connue et complete (cmd inconnue, rappel manquant pour
// accepter / reporter). Un "rappel" present sur chatouille / os / bain est ignore.
std::optional<Commande> lire_commande(const std::string &ligne);

// Tension de pile (mV) -> 0..100 ; -1 si inconnue (mv <= 0). Courbe LiPo 1S
// reprise d un autre pilote de board du meme auteur.
int pile_pourcent(int mv);

// Quand emettre la ligne d'etat : au premier appel, a chaque changement, et au plus tard 30 s
// apres la derniere emission.
class Emetteur {
public:
    static constexpr int64_t kPeriodeS = 30;
    bool a_emettre(const EtatBoard &e, int64_t mono_s);
    void forcer(); // prochain a_emettre vrai (nouvel abonne)

private:
    std::optional<EtatBoard> dernier_;
    int64_t dernier_mono_ = 0;
};

} // namespace pup
