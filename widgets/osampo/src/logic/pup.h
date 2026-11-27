#pragma once
#include <cstdint>
#include <optional>
#include <string>
#include <vector>

#include "logic/needs.h"
#include "logic/reminders.h"
#include "logic/settings.h"

namespace pup {

// Duree de la pose aboie a chaque relance d'une reclamation en laisse (SPEC-012).
constexpr int64_t ABOIE_RELANCE_S = 3;

enum class State { Repos, Dort, Reclame, Rapporte, Content, Tournis, Boude, Bain, Mange, Promenade };
enum class Reason { None, Rappel, Promenade, Claude };
enum class Input { Tap, LongPress, Stroke, Throw, Shake, FaceDown, FaceUp, AnimDone, Brosse, Os, Laisse };

// Evenement a remonter au PC (SPEC-001). rappel : le rappel sortie / sport / dodo concerne ;
// absent pour Accepte / Reporte = reclamation de la jauge (promenade). Os, Chatouille, Bain : sans rappel.
enum class EvtKind { Os, Chatouille, Bain, Accepte, Reporte, Abandonne };
struct Evt {
    EvtKind kind;
    std::optional<Reminder> rappel;
};

// Machine a etats du chiot (spec SP5 §4-§5, U3, B5 ; promenade : spec SP5h §5).
//
// Regles :
// - Dort (heures calmes ou face contre table) prime sur tout : l'entree en Dort
//   arrete les escalades et abandonne le rappel en cours (meme reporte). En Dort,
//   reminder_due, jauge et claude_wait sont ignores, tick rend None et seul
//   FaceUp agit. La sortie mene a l'etat de repos. Exceptions SP5h : une
//   promenade en cours et une reclamation dodo survivent au debut des heures
//   calmes, en silence ; pendant la promenade, FaceDown / FaceUp sont ignores
//   (board dans la poche).
// - Etat de repos : Boude si set_sulking(true), sinon Repos.
// - Reclame : raison Rappel > Promenade > Claude ; une raison superieure
//   remplace l'inferieure, une inferieure est ignoree. Reclame interrompt les
//   animations.
//   - Rappel repas : escalade ; Tap -> Content + Jappe ; LongPress reporte
//     report_min. Rappel sortie / sport / dodo : escalade, laisse ; Tap sans
//     effet ; LongPress reporte comme un repas (ADR-001) ; Laisse -> promenade
//     a heure fixe.
//   - Promenade (jauge) : jauge(sage == 0, ou < seuil_reclame apres une promenade
//     trop courte, jusqu'a ce qu'elle le depasse) ; escalade (commence par Wouf) ;
//     Laisse -> promenade ; la jauge qui remonte a seuil_reclame (absence sans
//     laisse) -> Content + Jappe, Bouger par take_need ; donnees perimees -> repos ;
//     LongPress -> repos, pas de nouvelle reclamation de jauge avant report_min.
//   - Relance (rappel sortie / sport / dodo, jauge) : chaque Cue::Aboie de l'escalade pose
//     aboie ABOIE_RELANCE_S s, puis la pose redevient laisse (SPEC-012, ADR-007).
//   - Claude : un seul Gemit par attente ; Tap -> repos ; Laisse -> promenade. Patte au debut ;
//     passe patte_avant_aboie_min, pose aboie et un Cue::Aboie unique (SPEC-012).
// - Promenade : Laisse au repos (a la demande) ou en reclamation ; silence, aucun
//   rappel, Claude ignore. Fin : Laisse, retour_min minutes d'actif continu
//   (cue Wouf, besoin par take_need), ou 3 h. Reussie : jauge >= seuil_reclame
//   (jauge, a la demande) ou absence vue >= courte_min (heure fixe) ; donnees
//   perimees -> reussie. Reussie : Content + Jappe, Bouger ; dodo -> Dort jusqu'a
//   la fin des heures calmes (12 h au plus), Dormir. Ratee : repos ; redemande
//   600 s plus tard (heure fixe : 2 fois au plus).
// - Hors Reclame et Promenade : Stroke -> Content + Jappe (Jouer), Throw -> Rapporte
//   (Jouer), Shake -> Tournis, Brosse -> Bain + Jappe (Jouer), Os -> Mange + Jappe ;
//   AnimDone ramene ces etats au repos. Tap au repos ne fait rien.
// - Chaque cue ponctuel (Jappe, Gemit) est rendu une seule fois par tick.
class Pup {
public:
    State state() const;
    Reason reason() const;
    // Report, calme et relances ; l'etat en cours (escalade, report, calme) est conserve.
    void set_settings(const Settings &s);
    void set_quiet(bool quiet);
    void set_sulking(bool sulking);
    void reminder_due(const Reminder &r, int64_t mono_s);
    // Jauge du daemon (SP5h) ; frais = donnees utilisables. Signal de niveau.
    // finie_min / finie_age_min : derniere absence terminee et minutes depuis sa
    // fin (-1 : aucune) ; une promenade hors de portee se juge aussi sur elle.
    void jauge(int sage, bool actif, int absence_min, bool frais, int64_t mono_s, int finie_min = 0,
               int finie_age_min = -1);
    void claude_wait(bool waiting, int64_t mono_s); // false aussi si donnees perimees
    // rend le Need a satisfaire, s'il y en a un (rappel valide, caresse, lancer)
    std::optional<Need> input(Input in, int64_t mono_s);
    Cue tick(int64_t mono_s);
    // Rappel reclame ou reporte par LongPress, sinon nullopt.
    std::optional<Reminder> current_reminder() const;
    // Besoin rendu hors d'un input (fin automatique, absence sans laisse), une fois.
    std::optional<Need> take_need();
    // Evenements produits depuis le dernier appel (Os / Stroke pris en compte, laisse,
    // report, rappel abandonne a l'entree en Dort) ; vide ensuite.
    std::vector<Evt> take_events();
    // Rappel sortie / sport / dodo reporte et pas encore revenu.
    bool rappel_reporte() const;
    // Accepte (laisse) le rappel reporte, comme la laisse sur sa reclamation ; false sans effet.
    bool accepter_reporte(int64_t mono_s);
    // Temps mono du debut de la promenade en cours ; nullopt sans promenade ou apres un load.
    std::optional<int64_t> promenade_debut() const;
    // Pose de reclamation affichee (SPEC-012) : "patte", "laisse" ou "aboie" en Reclame, "-" sinon.
    const char *pose() const;
    bool laisse_visible() const { return state_ != State::Dort; }
    bool dodo_reclame() const;
    bool dodo_apres_promenade() const { return dodo_dort_; }
    // Promenade en cours (type, absence vue, redemandes) ; "-" sans promenade.
    std::string save() const;
    bool load(const std::string &s); // false et etat inchange si invalide

private:
    int64_t calm_s() const;
    bool asleep() const;
    State rest() const;
    void go_rest();
    void fall_asleep();
    void claim(Reason r, int64_t mono_s = 0);
    void claude_arreter();
    void advance(int64_t mono_s);
    std::optional<Need> input_reclame(Input in, int64_t mono_s);
    enum class Balade { Jauge, Demande, Heure, Dodo };
    void partir(Balade b, std::optional<Reminder> r, int64_t mono_s);
    std::optional<Need> finir(int64_t mono_s, bool retour_pc);
    bool survit() const; // promenade, reclamation dodo ou dodo reporte : survit au calme
    bool dodo_reporte() const;
    void emettre(EvtKind k, std::optional<Reminder> r = std::nullopt);

    State state_ = State::Repos;
    Reason reason_ = Reason::None;
    Settings cfg_;
    bool quiet_ = false;
    bool face_down_ = false;
    bool sulking_ = false;
    Cue pending_ = Cue::None;

    std::optional<Reminder> reminder_;
    std::optional<int64_t> snooze_until_;
    Escalation rappel_esc_;

    Escalation inact_esc_; // escalade de la reclamation de promenade (jauge)
    std::optional<int64_t> calm_until_;

    // Jauge (SP5h)
    int sage_ = 100, absence_ = 0;
    bool actif_ = false, frais_ = false;
    bool hysteresis_ = false;              // apres une promenade trop courte
    std::optional<int64_t> redemande_;     // jauge : pas de reclamation avant
    struct Sortie {
        Balade type;
        int64_t debut; // -1 : a fixer au prochain appel (apres load)
        int absence_max;
        std::optional<Reminder> rappel;
    };
    std::optional<Sortie> sortie_;
    std::optional<int64_t> actif_depuis_;
    int redemandes_ = 0;                   // heure fixe
    std::optional<Reminder> a_redemander_;
    std::optional<int64_t> redemande_rappel_;
    bool dodo_dort_ = false;
    int64_t dodo_jusqu_ = 0;
    std::optional<Need> need_;
    std::vector<Evt> events_;

    bool claude_gemit_done_ = false; // Gemit deja donne pour l'attente en cours
    bool claude_dismissed_ = false;  // Tap pendant l'attente en cours
    int64_t claude_aboie_a_ = 0;     // heure monotone du passage patte -> aboie (reclamation Claude)
    bool claude_aboie_ = false;      // escalade faite : pose aboie
    bool aboie_cue_ = false;         // Cue::Aboie a rendre (apres le Gemit en attente)
    int64_t now_ = 0;                // heure monotone du dernier advance / tick
    std::optional<int64_t> aboie_jusqu_a_; // relance d'une reclamation en laisse : pose aboie jusque-la (SPEC-012)
};

} // namespace pup
