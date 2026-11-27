#include "logic/controller.h"

#include <cstdio>
#include <iterator>
#include <sstream>
#include <utility>

namespace pup {

namespace {
constexpr const char *kTag = "tgm1";

const char *state_name(State s)
{
    switch (s) {
    case State::Repos: return "repos";
    case State::Dort: return "dort";
    case State::Reclame: return "reclame";
    case State::Rapporte: return "rapporte";
    case State::Content: return "content";
    case State::Tournis: return "tournis";
    case State::Boude: return "boude";
    case State::Bain: return "bain";
    case State::Mange: return "mange";
    case State::Promenade: return "promenade";
    }
    return "repos";
}

const char *reason_name(Reason r)
{
    switch (r) {
    case Reason::None: return "";
    case Reason::Rappel: return "rappel";
    case Reason::Promenade: return "promenade";
    case Reason::Claude: return "claude";
    }
    return "";
}

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

const char *anim_name(State s, const char *pose, bool by_stroke)
{
    switch (s) {
    case State::Reclame:
        return pose[0] == '-' ? "aboie" : pose;
    case State::Content: return by_stroke ? "roulade" : "content";
    case State::Repos:
    case State::Dort:
    case State::Rapporte:
    case State::Tournis:
    case State::Boude:
    case State::Bain: return state_name(s);
    case State::Mange: return "os";
    case State::Promenade: return "promenade";
    }
    return "repos";
}
} // namespace

Controller::Controller(int stale_s, std::vector<Reminder> list)
    : stale_s_(stale_s), sched_(std::move(list))
{
}

void Controller::set_settings(const Settings &s)
{
    if (!(s.rappels == cfg_.rappels)) {
        sched_.set(s.rappels);
        if (sched_attente_) sched_.load(*sched_attente_); // echoue sans dommage si la liste differe
    }
    sched_attente_.reset();
    cfg_ = s;
    sched_.set_settings(s);
    pup_.set_settings(s);
    needs_.set_settings(s);
}

const Settings &Controller::settings() const
{
    return cfg_;
}

void Controller::avancer(int64_t mono_s)
{
    mono_ = mono_s;
    if (debut_ < 0) debut_ = mono_s;
}

bool Controller::usable() const
{
    return online_ && frame_.age + (mono_ - frame_mono_) <= stale_s_;
}

std::optional<int64_t> Controller::today(int64_t mono_s) const
{
    const auto t = clock_.now(mono_s);
    if (!t) return std::nullopt;
    return t->day;
}

void Controller::on_link(bool online)
{
    online_ = online;
}

void Controller::on_battery(int mv)
{
    battery_mv_ = mv;
}

void Controller::on_frame(const DataFrame &d, int64_t mono_s)
{
    avancer(mono_s);
    // Ne resynchroniser que si la source a une nouvelle valeur : un epoch
    // identique re-envoye ferait reculer l'horloge (dents de scie).
    if (d.epoch != 0 && (!clock_.is_set() || d.epoch != synced_epoch_ || d.tz_s != synced_tz_)) {
        clock_.sync(d.epoch, d.tz_s, mono_s);
        synced_epoch_ = d.epoch;
        synced_tz_ = d.tz_s;
    }
    frame_ = d;
    frame_mono_ = mono_s;
}

void Controller::on_input(Input in, int64_t mono_s)
{
    entree(in, mono_s, "board");
}

// Evenements du chiot -> file du lien ; source : "board" ou "pc" pour os et chatouille.
void Controller::recolter(const char *source)
{
    for (const Evt &e : pup_.take_events()) {
        Evenement v{e.kind, "", e.rappel ? id_rappel(*e.rappel) : ""};
        if (e.kind == EvtKind::Os || e.kind == EvtKind::Chatouille || e.kind == EvtKind::Bain) v.source = source;
        else if (e.kind != EvtKind::Abandonne && !e.rappel) v.rappel = "promenade"; // jauge
        events_.push_back(std::move(v));
    }
}

std::vector<Evenement> Controller::take_events()
{
    std::vector<Evenement> e;
    e.swap(events_);
    return e;
}

void Controller::on_command(const Commande &c, int64_t mono_s)
{
    avancer(mono_s);
    switch (c.kind) {
    case Commande::Kind::Chatouille: entree(Input::Stroke, mono_s, "pc"); return;
    case Commande::Kind::Os: entree(Input::Os, mono_s, "pc"); return;
    case Commande::Kind::Bain: entree(Input::Brosse, mono_s, "pc"); return;
    case Commande::Kind::Accepter:
    case Commande::Kind::Reporter: break;
    }
    const bool accepter = c.kind == Commande::Kind::Accepter;
    const bool jauge = c.rappel == "promenade";
    const auto rem = pup_.current_reminder();
    const bool rappel_ok = rem && rem->cat != Category::Repas && id_rappel(*rem) == c.rappel;
    if (pup_.state() == State::Reclame) {
        if (jauge ? pup_.reason() == Reason::Promenade : (pup_.reason() == Reason::Rappel && rappel_ok))
            entree(accepter ? Input::Laisse : Input::LongPress, mono_s, "pc");
        return;
    }
    // Rappel deja reporte : on peut encore l'accepter (pas le reporter une seconde fois).
    if (accepter && !jauge && rappel_ok && pup_.accepter_reporte(mono_s)) recolter("pc");
}

void Controller::entree(Input in, int64_t mono_s, const char *source)
{
    avancer(mono_s);
    const State before = pup_.state();
    const std::optional<Need> need = pup_.input(in, mono_s);
    recolter(source);
    if (pup_.state() == State::Content && before != State::Content)
        content_by_stroke_ = in == Input::Stroke;
    const auto t = clock_.now(mono_s);
    if (!t) return;
    if (need) needs_.satisfy(*need, t->day);
    // Retourne pendant les heures calmes : le chiot dort, une fois par jour (F8).
    if (in == Input::FaceDown && in_range(t->minute, cfg_.calme_debut, cfg_.calme_fin) && dormir_day_ != t->day) {
        dormir_day_ = t->day;
        needs_.satisfy(Need::Dormir, t->day);
    }
}

Cue Controller::tick(int64_t mono_s)
{
    avancer(mono_s);
    if (const auto t = clock_.now(mono_s)) {
        needs_.on_day(t->day);
        pup_.set_quiet(in_range(t->minute, cfg_.calme_debut, cfg_.calme_fin)); // avant poll : pas de rappel des 22:00 (A3)
        // Un rappel a la fois : le suivant reste du dans sa fenetre de rattrapage.
        if (!pup_.current_reminder()) {
            if (const auto r = sched_.poll(*t)) pup_.reminder_due(*r, mono_s);
        }
        pup_.set_sulking(needs_.sulking());
    }
    const bool fresh = usable();
    pup_.jauge(frame_.sage, frame_.actif, frame_.absence_min, fresh, mono_s, frame_.absence_finie_min,
               frame_.absence_finie_age_min);
    pup_.claude_wait(fresh && frame_.claude_wait, mono_s);
    const Cue cue = pup_.tick(mono_s);
    // Besoin rendu hors d'un input : absence sans laisse, fin automatique de promenade.
    if (const auto need = pup_.take_need()) {
        if (const auto day = today(mono_s)) needs_.satisfy(*need, *day);
    }
    recolter("board"); // abandon d'un rappel a l'entree en Dort (calme)
    return cue;
}

EtatBoard Controller::etat_board() const
{
    EtatBoard e;
    e.etat = state_name(pup_.state());
    if (pup_.reason() != Reason::None) e.raison = reason_name(pup_.reason());
    e.pose = pup_.pose();
    if (const auto r = pup_.current_reminder()) e.rappel = id_rappel(*r);
    if (const auto d = pup_.promenade_debut()) {
        if (const auto t = clock_.now(*d)) {
            char buf[16];
            std::snprintf(buf, sizeof buf, "%02d:%02d", t->minute / 60, t->minute % 60);
            e.promenade_debut = buf;
        }
    }
    e.pile = pile_pourcent(battery_mv_);
    e.bouger = needs_.level(Need::Bouger);
    e.jouer = needs_.level(Need::Jouer);
    e.dormir = needs_.level(Need::Dormir);
    return e;
}

ViewModel Controller::view() const
{
    ViewModel v;
    v.state = state_name(pup_.state());
    v.reason = reason_name(pup_.reason());
    v.anim = anim_name(pup_.state(), pup_.pose(), content_by_stroke_);
    v.laisse = pup_.laisse_visible();
    v.bulle_dodo = pup_.dodo_reclame();
    v.batterie_faible = cfg_.batterie_faible_mv > 0 && battery_mv_ >= 0 && battery_mv_ < cfg_.batterie_faible_mv;
    if (pup_.reason() == Reason::Rappel) {
        if (const auto r = pup_.current_reminder()) v.reminder = category_name(r->cat);
    }
    v.clock_ok = clock_.is_set();
    v.en_ligne = usable() || (debut_ >= 0 && mono_ - debut_ <= stale_s_);
    if (const auto t = clock_.now(mono_)) {
        v.night = in_range(t->minute, cfg_.nuit_debut, cfg_.nuit_fin);
        v.heure = t->minute / 60;
        v.minute = t->minute % 60;
    }
    v.bouger = needs_.icon(Need::Bouger);
    v.jouer = needs_.icon(Need::Jouer);
    v.dormir = needs_.icon(Need::Dormir);
    return v;
}

std::string Controller::save() const
{
    std::string out = std::string(kTag) + ' ' + needs_.save() + " | " + sched_.save() + ' ';
    out += dormir_day_ ? std::to_string(*dormir_day_) : "x";
    out += " # " + pup_.save(); // promenade en cours (SP5h)
    return out;
}

bool Controller::load(const std::string &s)
{
    const std::string prefix = std::string(kTag) + ' ';
    if (s.compare(0, prefix.size(), prefix) != 0) return false;
    std::string body = s.substr(prefix.size());
    // Promenade en cours apres " # " (SP5h) ; une ligne plus ancienne n'en a pas.
    std::string balade = "-";
    if (const size_t h = body.find(" # "); h != std::string::npos) {
        balade = body.substr(h + 3);
        body.resize(h);
    }
    Pup p = pup_;
    if (!p.load(balade)) return false;
    // Ancienne ligne (besoins seuls, sans " | ") : memoire des rappels vide.
    const size_t bar = body.find(" | ");
    if (!needs_.load(body.substr(0, bar))) return false;
    pup_ = p;
    if (bar == std::string::npos) return true;
    std::istringstream in(body.substr(bar + 3));
    std::string sched, dormir;
    // La partie rappels contient des espaces : le dernier jeton est dormir_day_.
    std::string all(std::istreambuf_iterator<char>(in), {});
    const size_t sp = all.rfind(' ');
    if (sp == std::string::npos) return true;
    sched = all.substr(0, sp);
    dormir = all.substr(sp + 1);
    sched_.load(sched);
    sched_attente_ = sched;
    dormir_day_.reset();
    if (dormir != "x") {
        std::istringstream d(dormir);
        int64_t v;
        char extra;
        if (d >> v && !(d >> extra)) dormir_day_ = v;
    }
    return true;
}

} // namespace pup
