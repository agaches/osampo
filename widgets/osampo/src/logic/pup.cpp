#include "logic/pup.h"

#include <algorithm>
#include <cstdio>

namespace pup {

namespace {
constexpr int64_t kRedemandeS = 600;         // promenade ratee : nouvelle demande
constexpr int64_t kGardeFouS = 3 * 3600;     // promenade oubliee
constexpr int64_t kDodoMaxS = 12 * 3600;     // dodo sans heures calmes
constexpr int kRedemandesMax = 2;            // heure fixe

bool est_promenade(Category c) { return c != Category::Repas; }

// Rang de priorite d'une raison de Reclame : plus grand = plus fort.
int rank(Reason r)
{
    switch (r) {
    case Reason::Rappel: return 3;
    case Reason::Promenade: return 2;
    case Reason::Claude: return 1;
    case Reason::None: return 0;
    }
    return 0;
}
} // namespace

int64_t Pup::calm_s() const { return static_cast<int64_t>(cfg_.calme_validation_min) * 60; }

State Pup::state() const { return state_; }
Reason Pup::reason() const { return reason_; }
std::optional<Reminder> Pup::current_reminder() const { return reminder_; }

std::optional<Need> Pup::take_need()
{
    std::optional<Need> n = need_;
    need_.reset();
    return n;
}

bool Pup::dodo_reclame() const
{
    return state_ == State::Reclame && reason_ == Reason::Rappel && reminder_ && reminder_->cat == Category::Dodo;
}

bool Pup::dodo_reporte() const
{
    return reminder_ && reminder_->cat == Category::Dodo && snooze_until_ && state_ != State::Reclame;
}

bool Pup::survit() const { return state_ == State::Promenade || dodo_reclame() || dodo_reporte(); }

void Pup::emettre(EvtKind k, std::optional<Reminder> r) { events_.push_back(Evt{k, r}); }

std::vector<Evt> Pup::take_events()
{
    std::vector<Evt> e;
    e.swap(events_);
    return e;
}

bool Pup::rappel_reporte() const
{
    return reminder_ && est_promenade(reminder_->cat) && snooze_until_ && state_ != State::Promenade;
}

std::optional<int64_t> Pup::promenade_debut() const
{
    if (!sortie_ || sortie_->debut < 0) return std::nullopt;
    return sortie_->debut;
}

bool Pup::accepter_reporte(int64_t mono_s)
{
    advance(mono_s);
    if (!rappel_reporte()) return false;
    const Reminder r = *reminder_;
    emettre(EvtKind::Accepte, r);
    partir(r.cat == Category::Dodo ? Balade::Dodo : Balade::Heure, r, mono_s);
    return true;
}

bool Pup::asleep() const { return quiet_ || face_down_ || dodo_dort_; }
State Pup::rest() const { return sulking_ ? State::Boude : State::Repos; }

void Pup::claude_arreter()
{
    claude_aboie_ = false;
    aboie_cue_ = false;
    aboie_jusqu_a_.reset(); // toute sortie ou reprise de reclamation arrete la relance
}

const char *Pup::pose() const
{
    if (state_ != State::Reclame) return "-";
    switch (reason_) {
    case Reason::Promenade: return aboie_jusqu_a_ && now_ < *aboie_jusqu_a_ ? "aboie" : "laisse";
    case Reason::Rappel:
        if (!reminder_ || reminder_->cat == Category::Repas) return "aboie";
        return aboie_jusqu_a_ && now_ < *aboie_jusqu_a_ ? "aboie" : "laisse";
    case Reason::Claude: return claude_aboie_ ? "aboie" : "patte";
    case Reason::None: break;
    }
    return "-";
}

void Pup::go_rest()
{
    claude_arreter();
    state_ = rest();
    reason_ = Reason::None;
    if (pending_ == Cue::Gemit) pending_ = Cue::None; // Claude n'est plus la raison (R8)
}

// Calme ou face contre table : tout s'arrete, le rappel est abandonne (R-a).
void Pup::fall_asleep()
{
    if (reminder_ && est_promenade(reminder_->cat)) emettre(EvtKind::Abandonne, reminder_);
    state_ = State::Dort;
    reason_ = Reason::None;
    pending_ = Cue::None;
    claude_arreter();
    rappel_esc_.stop();
    inact_esc_.stop();
    reminder_.reset();
    snooze_until_.reset();
    a_redemander_.reset();
    redemande_rappel_.reset();
}

// Passe en Reclame pour r ; l'appelant a deja verifie la priorite. Une raison
// remplacee perd son escalade.
void Pup::claim(Reason r, int64_t mono_s)
{
    claude_arreter(); // une reclamation Claude (re)prise recommence a la patte
    if (r == Reason::Claude) claude_aboie_a_ = mono_s + static_cast<int64_t>(cfg_.patte_avant_aboie_min) * 60;
    if (reason_ == Reason::Promenade && r != Reason::Promenade) inact_esc_.stop();
    if (r != Reason::Claude && pending_ == Cue::Gemit) pending_ = Cue::None; // R8
    state_ = State::Reclame;
    reason_ = r;
}

// Echeances : report d'un rappel, nouvelle demande d'une promenade a heure fixe,
// garde-fou de la promenade, fin d'un dodo sans heures calmes.
void Pup::advance(int64_t mono_s)
{
    now_ = mono_s;
    if (state_ == State::Reclame && reason_ == Reason::Claude && !claude_aboie_ && mono_s >= claude_aboie_a_) {
        claude_aboie_ = true;
        aboie_cue_ = true;
    }
    if (sortie_) {
        if (sortie_->debut < 0) sortie_->debut = mono_s;
        if (mono_s - sortie_->debut >= kGardeFouS) need_ = finir(mono_s, false);
        return;
    }
    if (dodo_dort_ && mono_s >= dodo_jusqu_) {
        dodo_dort_ = false;
        if (!quiet_ && !face_down_ && state_ == State::Dort) go_rest();
    }
    if (a_redemander_ && redemande_rappel_ && mono_s >= *redemande_rappel_) {
        const Reminder r = *a_redemander_;
        a_redemander_.reset();
        redemande_rappel_.reset();
        if (!face_down_ && !dodo_dort_ && (!quiet_ || r.cat == Category::Dodo)) {
            reminder_ = r;
            rappel_esc_.start(mono_s);
            claim(Reason::Rappel);
        }
    }
    if (!snooze_until_ || mono_s < *snooze_until_) return;
    // Un dodo reporte revient meme pendant les heures calmes, en silence.
    if (asleep() && !(quiet_ && !face_down_ && !dodo_dort_ && dodo_reporte())) return;
    snooze_until_.reset();
    claim(Reason::Rappel);
}

void Pup::set_settings(const Settings &s)
{
    cfg_ = s;
    rappel_esc_.set_settings(s);
    inact_esc_.set_settings(s);
}

void Pup::set_quiet(bool quiet)
{
    const bool avant = quiet_;
    quiet_ = quiet;
    if (quiet_) {
        if (!survit() && state_ != State::Dort) fall_asleep();
        else if (state_ == State::Dort) pending_ = Cue::None;
        return;
    }
    if (avant && dodo_dort_) dodo_dort_ = false; // matin apres le dodo
    if (avant) { // matin : une reclamation dodo sans reponse est abandonnee
        if (dodo_reclame() || dodo_reporte()) {
            const bool reclame = dodo_reclame();
            rappel_esc_.stop();
            reminder_.reset();
            snooze_until_.reset();
            if (reclame) go_rest();
        }
        if (a_redemander_ && a_redemander_->cat == Category::Dodo) {
            a_redemander_.reset();
            redemande_rappel_.reset();
        }
    }
    if (!face_down_ && !dodo_dort_ && state_ == State::Dort) go_rest();
}

void Pup::set_sulking(bool sulking)
{
    sulking_ = sulking;
    if (state_ == State::Repos || state_ == State::Boude) state_ = rest();
}

void Pup::reminder_due(const Reminder &r, int64_t mono_s)
{
    advance(mono_s);
    if (asleep() || state_ == State::Promenade) return; // abandonne (B5)
    reminder_ = r;
    snooze_until_.reset();
    a_redemander_.reset();
    redemande_rappel_.reset();
    redemandes_ = 0;
    rappel_esc_.start(mono_s);
    claim(Reason::Rappel);
}

void Pup::jauge(int sage, bool actif, int absence_min, bool frais, int64_t mono_s, int finie_min, int finie_age_min)
{
    advance(mono_s);
    frais_ = frais;
    if (frais) {
        sage_ = sage;
        actif_ = actif;
        absence_ = absence_min;
    }
    if (state_ == State::Promenade) {
        if (!frais) return;
        sortie_->absence_max = std::max(sortie_->absence_max, absence_min);
        // Absence terminee pendant la promenade (hors de portee, PC en veille :
        // a la reconnexion elle est deja finie) ; une minute de marge d'arrondi.
        if (finie_age_min >= 0 && mono_s - static_cast<int64_t>(finie_age_min + 1) * 60 >= sortie_->debut)
            sortie_->absence_max = std::max(sortie_->absence_max, finie_min);
        if (!actif) {
            actif_depuis_.reset();
            return;
        }
        if (!actif_depuis_) actif_depuis_ = mono_s;
        if (mono_s - *actif_depuis_ >= static_cast<int64_t>(cfg_.retour_min) * 60) need_ = finir(mono_s, true);
        return;
    }
    const bool reclame_jauge = state_ == State::Reclame && reason_ == Reason::Promenade;
    if (!frais) {
        if (reclame_jauge) {
            inact_esc_.stop();
            go_rest();
        }
        return;
    }
    if (sage_ >= cfg_.seuil_reclame) hysteresis_ = false;
    if (reclame_jauge) {
        if (sage_ > 0 && sage_ >= cfg_.seuil_reclame) { // absence sans laisse (P4)
            inact_esc_.stop();
            state_ = State::Content;
            reason_ = Reason::None;
            pending_ = Cue::Jappe;
            need_ = Need::Bouger;
        }
        return;
    }
    if (asleep()) return;
    const bool vide = hysteresis_ ? sage_ < cfg_.seuil_reclame : sage_ <= 0;
    if (!vide) return;
    if (calm_until_) {
        if (mono_s < *calm_until_) return;
        calm_until_.reset();
    }
    if (redemande_) {
        if (mono_s < *redemande_) return;
        redemande_.reset();
    }
    if (state_ == State::Reclame && rank(reason_) >= rank(Reason::Promenade)) return;
    inact_esc_.start(mono_s);
    claim(Reason::Promenade);
}

void Pup::claude_wait(bool waiting, int64_t mono_s)
{
    advance(mono_s);
    if (!waiting) {
        claude_gemit_done_ = false;
        claude_dismissed_ = false;
        if (state_ == State::Reclame && reason_ == Reason::Claude) go_rest();
        return;
    }
    if (asleep() || claude_dismissed_ || state_ == State::Promenade) return;
    if (state_ == State::Reclame && reason_ == Reason::Claude) {
        // Gemit retenu par un Jappe en attente : il part une fois le Jappe joue.
        if (!claude_gemit_done_ && pending_ == Cue::None) {
            pending_ = Cue::Gemit;
            claude_gemit_done_ = true;
        }
        return;
    }
    if (state_ == State::Reclame && rank(reason_) >= rank(Reason::Claude)) return;
    claim(Reason::Claude, mono_s);
    if (!claude_gemit_done_ && pending_ != Cue::Jappe) {
        pending_ = Cue::Gemit;
        claude_gemit_done_ = true;
    }
}

void Pup::partir(Balade b, std::optional<Reminder> r, int64_t mono_s)
{
    rappel_esc_.stop();
    inact_esc_.stop();
    reminder_.reset();
    snooze_until_.reset();
    aboie_jusqu_a_.reset();
    sortie_ = Sortie{b, mono_s, 0, r};
    actif_depuis_.reset();
    state_ = State::Promenade;
    reason_ = Reason::None;
    pending_ = quiet_ ? Cue::None : Cue::Jappe;
}

std::optional<Need> Pup::finir(int64_t mono_s, bool retour_pc)
{
    const Sortie s = *sortie_;
    sortie_.reset();
    actif_depuis_.reset();
    const bool heure = s.type == Balade::Heure || s.type == Balade::Dodo;
    const bool ok = !frais_ || (heure ? s.absence_max >= cfg_.courte_min : sage_ >= cfg_.seuil_reclame);
    if (!ok) {
        go_rest();
        if (quiet_) fall_asleep();
        pending_ = retour_pc && !quiet_ ? Cue::Wouf : Cue::None;
        if (!heure) {
            hysteresis_ = true;
            redemande_ = mono_s + kRedemandeS;
        } else if (s.rappel && redemandes_ < kRedemandesMax) {
            ++redemandes_;
            a_redemander_ = s.rappel;
            redemande_rappel_ = mono_s + kRedemandeS;
        } else {
            redemandes_ = 0;
        }
        return std::nullopt;
    }
    hysteresis_ = false;
    redemande_.reset();
    redemandes_ = 0;
    if (s.type == Balade::Dodo) {
        state_ = State::Dort;
        reason_ = Reason::None;
        pending_ = Cue::None;
        dodo_dort_ = true;
        dodo_jusqu_ = mono_s + kDodoMaxS;
        return Need::Dormir;
    }
    if (quiet_) {
        fall_asleep();
        return Need::Bouger;
    }
    state_ = State::Content;
    reason_ = Reason::None;
    pending_ = retour_pc ? Cue::Wouf : Cue::Jappe;
    return Need::Bouger;
}

// En Reclame : Tap, LongPress et Laisse (R-g, SP5h).
std::optional<Need> Pup::input_reclame(Input in, int64_t mono_s)
{
    switch (reason_) {
    case Reason::Rappel: {
        const Reminder r = *reminder_;
        if (in == Input::LongPress) { // report, aussi pour sortie / sport / dodo (ADR-001)
            rappel_esc_.snooze(mono_s);
            snooze_until_ = mono_s + static_cast<int64_t>(cfg_.report_min) * 60;
            calm_until_ = mono_s + calm_s(); // R7
            go_rest();
            if (est_promenade(r.cat)) emettre(EvtKind::Reporte, r);
            return std::nullopt;
        }
        if (est_promenade(r.cat)) {
            if (in == Input::Laisse) {
                emettre(EvtKind::Accepte, r);
                partir(r.cat == Category::Dodo ? Balade::Dodo : Balade::Heure, r, mono_s);
            }
            return std::nullopt;
        }
        if (in == Input::Tap) {
            rappel_esc_.stop();
            reminder_.reset();
            state_ = State::Content;
            reason_ = Reason::None;
            pending_ = Cue::Jappe;
            calm_until_ = mono_s + calm_s(); // R7
        }
        return std::nullopt; // repas : aucun besoin
    }
    case Reason::Promenade:
        if (in == Input::Laisse) {
            emettre(EvtKind::Accepte);
            partir(Balade::Jauge, std::nullopt, mono_s);
        } else if (in == Input::LongPress) { // arrete la reclamation de la jauge
            inact_esc_.stop();
            redemande_ = mono_s + static_cast<int64_t>(cfg_.report_min) * 60;
            go_rest();
            emettre(EvtKind::Reporte);
        }
        return std::nullopt;
    case Reason::Claude:
        if (in == Input::Tap) {
            claude_dismissed_ = true;
            go_rest();
        } else if (in == Input::Laisse) {
            claude_dismissed_ = true;
            partir(Balade::Demande, std::nullopt, mono_s);
        }
        return std::nullopt;
    case Reason::None: return std::nullopt;
    }
    return std::nullopt;
}

std::optional<Need> Pup::input(Input in, int64_t mono_s)
{
    advance(mono_s);
    if (state_ == State::Promenade) {
        if (in == Input::Laisse) return finir(mono_s, false);
        return std::nullopt; // poche : ni retournement ni geste
    }
    if (in == Input::FaceDown) {
        face_down_ = true;
        fall_asleep();
        return std::nullopt;
    }
    if (in == Input::FaceUp) {
        face_down_ = false;
        if (!quiet_ && !dodo_dort_ && state_ == State::Dort) go_rest();
        return std::nullopt;
    }
    if (state_ == State::Reclame && (!asleep() || dodo_reclame())) return input_reclame(in, mono_s);
    if (asleep()) return std::nullopt;

    switch (in) {
    case Input::Stroke:
        state_ = State::Content;
        pending_ = Cue::Jappe;
        emettre(EvtKind::Chatouille);
        return Need::Jouer;
    case Input::Throw:
        state_ = State::Rapporte;
        return Need::Jouer;
    case Input::Shake:
        state_ = State::Tournis;
        return std::nullopt;
    case Input::Brosse: // icone brosse : bain (SP5e)
        state_ = State::Bain;
        pending_ = Cue::Jappe;
        emettre(EvtKind::Bain);
        return Need::Jouer;
    case Input::Os: // icone os : il croque un os (SP5f), friandise
        state_ = State::Mange;
        pending_ = Cue::Jappe;
        emettre(EvtKind::Os);
        return std::nullopt;
    case Input::Laisse: // promenade a la demande (SP5h)
        partir(Balade::Demande, std::nullopt, mono_s);
        return std::nullopt;
    case Input::AnimDone:
        if (state_ == State::Content || state_ == State::Rapporte || state_ == State::Tournis || state_ == State::Bain || state_ == State::Mange)
            go_rest();
        return std::nullopt;
    case Input::Tap:       // au repos : sans effet (R-h)
    case Input::LongPress: // hors Reclame/Rappel : sans effet
    case Input::FaceDown:
    case Input::FaceUp: return std::nullopt;
    }
    return std::nullopt;
}

Cue Pup::tick(int64_t mono_s)
{
    advance(mono_s);
    if (asleep()) {
        pending_ = Cue::None;
        aboie_cue_ = false;
        return Cue::None;
    }
    if (pending_ != Cue::None) {
        const Cue c = pending_;
        pending_ = Cue::None;
        return c;
    }
    if (aboie_cue_) { // apres le Gemit, une seule fois
        aboie_cue_ = false;
        return Cue::Aboie;
    }
    if (state_ != State::Reclame) return Cue::None;
    Cue c = Cue::None;
    if (reason_ == Reason::Rappel) c = rappel_esc_.tick(mono_s);
    else if (reason_ == Reason::Promenade) c = inact_esc_.tick(mono_s);
    // Relance d'une reclamation en laisse ; le Wouf du debut et le repas n'en sont pas.
    const bool laisse = reason_ == Reason::Promenade || (reminder_ && reminder_->cat != Category::Repas);
    if (c == Cue::Aboie && laisse) aboie_jusqu_a_ = mono_s + ABOIE_RELANCE_S;
    return c;
}

std::string Pup::save() const
{
    if (!sortie_) return "-";
    const Reminder r = sortie_->rappel.value_or(Reminder{Category::Repas, 0, 0});
    char buf[64];
    std::snprintf(buf, sizeof buf, "P %d %d %d %d %d %d", static_cast<int>(sortie_->type), sortie_->absence_max,
                  redemandes_, sortie_->rappel ? static_cast<int>(r.cat) : -1, r.minute, r.days);
    return buf;
}

bool Pup::load(const std::string &s)
{
    if (s == "-") return true;
    int type = 0, absence = 0, redem = 0, cat = 0, minute = 0, days = 0;
    char fin = 0;
    if (std::sscanf(s.c_str(), "P %d %d %d %d %d %d%c", &type, &absence, &redem, &cat, &minute, &days, &fin) != 6)
        return false;
    if (type < 0 || type > 3 || absence < 0 || redem < 0 || redem > kRedemandesMax || cat < -1 || cat > 3 ||
        minute < 0 || minute > 1439 || days < 0 || days > 127)
        return false;
    std::optional<Reminder> r;
    if (cat >= 0) r = Reminder{static_cast<Category>(cat), minute, static_cast<uint8_t>(days)};
    sortie_ = Sortie{static_cast<Balade>(type), -1, absence, r};
    redemandes_ = redem;
    state_ = State::Promenade;
    reason_ = Reason::None;
    return true;
}

} // namespace pup
