#include "logic/lien_pc.h"

#include <cstdio>

#include "json_line.h"

namespace pup {

namespace {
const char *category_name(Category c)
{
    switch (c) {
    case Category::Sortie: return "sortie";
    case Category::Sport: return "sport";
    case Category::Repas: return "repas";
    case Category::Dodo: return "dodo";
    }
    return "";
}

struct Point {
    int mv;
    int pct;
};
// Courbe LiPo 1S (tension au repos -> pourcentage).
constexpr Point kCourbe[] = {
    {3300, 0},  {3500, 5},  {3600, 10}, {3700, 25}, {3750, 40},  {3800, 50},
    {3850, 60}, {3900, 70}, {4000, 85}, {4100, 95}, {4200, 100},
};
constexpr int kN = sizeof(kCourbe) / sizeof(kCourbe[0]);

const char *kind_name(EvtKind k)
{
    switch (k) {
    case EvtKind::Os: return "os";
    case EvtKind::Chatouille: return "chatouille";
    case EvtKind::Bain: return "bain";
    case EvtKind::Accepte: return "accepte";
    case EvtKind::Reporte: return "reporte";
    case EvtKind::Abandonne: return "abandonne";
    }
    return "";
}
} // namespace

bool operator==(const EtatBoard &a, const EtatBoard &b)
{
    return a.etat == b.etat && a.raison == b.raison && a.pose == b.pose && a.rappel == b.rappel &&
           a.promenade_debut == b.promenade_debut && a.pile == b.pile && a.bouger == b.bouger &&
           a.jouer == b.jouer && a.dormir == b.dormir;
}

std::string id_rappel(const Reminder &r)
{
    char buf[32];
    std::snprintf(buf, sizeof buf, "%s,%02d%02d", category_name(r.cat), r.minute / 60, r.minute % 60);
    return buf;
}

// Les textes inseres sont des mots fixes ou des ids "categorie,HHMM" : rien a echapper.
std::string ligne_etat(const EtatBoard &e)
{
    std::string s = "{\"etat\":\"" + e.etat + "\",\"raison\":\"" + e.raison + "\",\"pose\":\"" + e.pose + "\",\"rappel\":\"" + e.rappel +
                    "\",\"promenade_debut\":\"" + e.promenade_debut + "\",\"pile\":" + std::to_string(e.pile) +
                    ",\"besoins\":{\"bouger\":" + std::to_string(e.bouger) + ",\"jouer\":" + std::to_string(e.jouer) +
                    ",\"dormir\":" + std::to_string(e.dormir) + "}}";
    return s;
}

std::string ligne_evenement(const Evenement &e)
{
    std::string s = std::string("{\"evt\":\"") + kind_name(e.kind) + "\"";
    if (e.kind == EvtKind::Os || e.kind == EvtKind::Chatouille || e.kind == EvtKind::Bain)
        s += ",\"source\":\"" + e.source + "\"";
    else
        s += ",\"rappel\":\"" + e.rappel + "\"";
    return s + "}";
}

bool est_ligne_commande(const std::string &ligne)
{
    JsonLine j;
    return j.parse(ligne.data(), ligne.size()) && j.has("cmd");
}

std::optional<Commande> lire_commande(const std::string &ligne)
{
    JsonLine j;
    if (!j.parse(ligne.data(), ligne.size())) return std::nullopt;
    const std::string cmd = j.text("cmd", "");
    if (cmd == "chatouille") return Commande{Commande::Kind::Chatouille, ""};
    if (cmd == "os") return Commande{Commande::Kind::Os, ""};
    if (cmd == "bain") return Commande{Commande::Kind::Bain, ""};
    const std::string rappel = j.text("rappel", "");
    if (rappel.empty()) return std::nullopt;
    if (cmd == "accepter") return Commande{Commande::Kind::Accepter, rappel};
    if (cmd == "reporter") return Commande{Commande::Kind::Reporter, rappel};
    return std::nullopt;
}

int pile_pourcent(int mv)
{
    if (mv <= 0) return -1;
    if (mv <= kCourbe[0].mv) return 0;
    if (mv >= kCourbe[kN - 1].mv) return 100;
    for (int i = 1; i < kN; i++) {
        if (mv <= kCourbe[i].mv) {
            const Point &a = kCourbe[i - 1];
            const Point &b = kCourbe[i];
            return a.pct + (mv - a.mv) * (b.pct - a.pct) / (b.mv - a.mv);
        }
    }
    return 100;
}

bool Emetteur::a_emettre(const EtatBoard &e, int64_t mono_s)
{
    if (dernier_ && *dernier_ == e && mono_s - dernier_mono_ < kPeriodeS) return false;
    dernier_ = e;
    dernier_mono_ = mono_s;
    return true;
}

void Emetteur::forcer()
{
    dernier_.reset();
}

} // namespace pup
