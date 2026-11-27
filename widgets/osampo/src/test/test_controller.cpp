// Les assert doivent rester actifs, meme en Release.
#undef NDEBUG
#include <cassert>
#include <string>
#include <utility>
#include <vector>

#include "logic/controller.h"
#include "logic/settings.h"

namespace {

using pup::Category;
using pup::Controller;
using pup::Cue;
using pup::EtatBoard;
using pup::Input;
using pup::Reminder;

constexpr int kStale = 30;
constexpr int64_t kDay = 20000; // jour local de reference

int64_t at(int h, int m, int s = 0, int64_t day = kDay)
{
    return day * 86400 + h * 3600 + m * 60 + s;
}

DataFrame frame(int64_t epoch)
{
    DataFrame d;
    d.epoch = epoch;
    d.online = true;
    return d;
}

// Controleur en ligne, horloge reglee a epoch (UTC = local) au temps mono 1000.
Controller started(int64_t epoch, std::vector<Reminder> list = pup::default_reminders())
{
    Controller c(kStale, std::move(list));
    c.on_link(true);
    c.on_frame(frame(epoch), 1000);
    return c;
}

void sans_horloge_aucun_rappel()
{
    Controller c(kStale);
    c.on_link(true);
    for (int64_t t = 0; t < 100000; t += 60) assert(c.tick(t) == Cue::None);
    assert(!c.view().clock_ok);
    assert(c.view().state == "repos");
    assert(c.view().reminder.empty());
}

void rappel_a_l_heure()
{
    Controller c = started(at(11, 59, 59));
    assert(c.tick(1000) == Cue::None);
    assert(c.view().state == "repos");
    assert(c.tick(1001) == Cue::Wouf);
    assert(c.view().state == "reclame");
    assert(c.view().reason == "rappel");
    assert(c.view().reminder == "repas");
}

void donnees_perimees_eteignent_f2_et_f7()
{
    // F2 : inactivite
    Controller c = started(at(15, 0));
    DataFrame d = frame(at(15, 0));
    d.sage = 0;
    c.on_frame(d, 1000);
    assert(c.tick(1000) == Cue::Wouf);
    assert(c.view().state == "reclame" && c.view().reason == "promenade");
    d.age = kStale + 1;
    c.on_frame(d, 1001);
    c.tick(1001);
    assert(c.view().state == "repos");

    // F7 : Claude attend
    Controller e = started(at(15, 0));
    DataFrame w = frame(at(15, 0));
    w.claude_wait = true;
    e.on_frame(w, 1000);
    assert(e.tick(1000) == Cue::Gemit);
    assert(e.view().state == "reclame" && e.view().reason == "claude");
    w.age = kStale + 1;
    e.on_frame(w, 1001);
    e.tick(1001);
    assert(e.view().state == "repos");

    // hors ligne : meme effet
    Controller f = started(at(15, 0));
    w.age = 0;
    f.on_frame(w, 1000);
    f.tick(1000);
    assert(f.view().reason == "claude");
    f.on_link(false);
    f.tick(1001);
    assert(f.view().state == "repos");
}

// R9 : l'age grandit avec le temps, meme sans nouvelle frame.
void age_effectif_grandit_sans_frame()
{
    Controller c = started(at(15, 0));
    DataFrame d = frame(at(15, 0));
    d.claude_wait = true;
    c.on_frame(d, 1000);
    c.tick(1000);
    assert(c.view().state == "reclame" && c.view().reason == "claude");
    c.tick(1000 + kStale);
    assert(c.view().state == "reclame" && c.view().reason == "claude");
    c.tick(1000 + kStale + 1);
    assert(c.view().state == "repos");

    Controller e = started(at(15, 0));
    DataFrame f = frame(at(15, 0));
    f.sage = 0;
    e.on_frame(f, 1000);
    e.tick(1000);
    assert(e.view().reason == "promenade");
    e.tick(1000 + kStale);
    assert(e.view().reason == "promenade");
    e.tick(1000 + kStale + 1);
    assert(e.view().state == "repos");
}

// SP5h : sage, actif et absence_min vont au chiot ; une absence sans laisse rend Bouger.
void jauge_transmise_au_chiot()
{
    Controller c = started(at(15, 0));
    const std::string before = c.save();
    DataFrame d = frame(at(15, 0));
    d.sage = 0;
    c.on_frame(d, 1001);
    c.tick(1001);
    assert(c.view().state == "reclame" && c.view().reason == "promenade" && c.view().laisse);
    d.sage = 50; // absence : la jauge remonte
    d.absence_min = 8;
    c.on_frame(d, 1002);
    c.tick(1002);
    assert(c.view().state == "content");
    assert(c.save() != before); // Bouger remonte
    // retour au PC pendant une promenade : fin automatique
    Controller e = started(at(15, 0));
    e.on_input(Input::Laisse, 1001);
    assert(e.view().state == "promenade" && e.view().anim == "promenade");
    DataFrame f = frame(at(15, 0));
    f.sage = 80;
    f.actif = true;
    for (int64_t t = 1002; t <= 1002 + 120; ++t) {
        e.on_frame(f, t);
        e.tick(t);
    }
    assert(e.view().state != "promenade");
}

// SP5h : la liste [[rappel]] de la config remplace la liste compilee.
void rappels_de_la_config_appliques()
{
    Controller c = started(at(10, 0), {});
    pup::Settings s;
    s.rappels = {Reminder{Category::Repas, 10 * 60, 0x7F}};
    c.set_settings(s);
    assert(c.tick(1000) == Cue::Wouf);
    assert(c.view().reminder == "repas");
    // reglages identiques renvoyes : les rappels deja sonnes ne reviennent pas
    c.on_input(Input::Tap, 1001);
    for (int64_t t = 1002; t < 1040; ++t) c.tick(t);
    c.on_input(Input::AnimDone, 1040);
    c.set_settings(s);
    for (int64_t t = 1041; t < 2000; ++t) c.tick(t);
    assert(c.view().state == "repos");
}

void batterie_faible_sous_le_seuil()
{
    Controller c = started(at(15, 0));
    pup::Settings s;
    s.batterie_faible_mv = 3500;
    c.set_settings(s);
    c.on_battery(3499);
    assert(c.view().batterie_faible);
    c.on_battery(3500);
    assert(!c.view().batterie_faible);
    c.on_battery(-1);
    assert(!c.view().batterie_faible);
    s.batterie_faible_mv = 0;
    c.set_settings(s);
    c.on_battery(3000);
    assert(!c.view().batterie_faible);
}

void laisse_visible_sauf_dort()
{
    Controller c = started(at(15, 0));
    c.tick(1000);
    assert(c.view().laisse);
    c.on_input(Input::FaceDown, 1001);
    assert(c.view().state == "dort" && !c.view().laisse);
}

void bulle_dodo_pendant_la_reclamation_dodo()
{
    Controller c = started(at(21, 45), {Reminder{Category::Dodo, 21 * 60 + 45, 0x7F}});
    assert(!c.view().bulle_dodo);
    c.tick(1000);
    assert(c.view().bulle_dodo && c.view().anim == "laisse");
    c.on_input(Input::Laisse, 1001);
    assert(!c.view().bulle_dodo && c.view().state == "promenade");
}

// La promenade en cours survit au redemarrage.
void promenade_survit_au_redemarrage()
{
    Controller a = started(at(15, 0));
    a.on_input(Input::Laisse, 1001);
    const std::string s = a.save();
    Controller b(kStale);
    assert(b.load(s));
    assert(b.view().state == "promenade");
    assert(b.save() == s);
    Controller v(kStale);
    assert(v.load(s.substr(0, s.find(" # ")))); // ligne d avant SP5h : sans promenade
    assert(v.view().state == "repos");
}

void retourne_la_nuit_remonte_dormir()
{
    Controller c = started(at(23, 0));
    c.tick(1000);
    const std::string before = c.save();
    c.on_input(Input::FaceDown, 1001);
    assert(c.view().state == "dort");
    const std::string once = c.save();
    assert(once != before);
    c.on_input(Input::FaceUp, 1002);
    c.on_input(Input::FaceDown, 1003); // meme jour : sans effet
    assert(c.save() == once);

    Controller j = started(at(15, 0)); // en journee : pas de besoin
    j.tick(1000);
    const std::string b = j.save();
    j.on_input(Input::FaceDown, 1001);
    assert(j.save() == b);
}

void nuit_et_icones()
{
    Controller c(kStale);
    assert(!c.view().night);
    c.on_link(true);
    c.on_frame(frame(at(6, 59)), 1000);
    c.tick(1000);
    assert(c.view().night && c.view().clock_ok);
    c.tick(1060); // 07:00
    assert(!c.view().night);
    c.tick(1000 + 13 * 3600); // 19:59
    assert(!c.view().night);
    c.tick(1000 + 13 * 3600 + 60); // 20:00
    assert(c.view().night);
    assert(c.view().bouger == 2 && c.view().jouer == 2 && c.view().dormir == 2);

    // sans satisfaction, les besoins decroissent de jour en jour
    c.tick(1000 + 86400); // jour suivant : 55
    assert(c.view().bouger == 1 && c.view().jouer == 1 && c.view().dormir == 1);
    c.tick(1000 + 3 * 86400); // 25
    assert(c.view().bouger == 0 && c.view().jouer == 0 && c.view().dormir == 0);
    assert(c.view().state == "dort"); // 06:59 : heures calmes
}

void etat_corrompu_ignore()
{
    Controller c = started(at(15, 0));
    c.on_input(Input::Stroke, 1000); // Jouer
    const std::string s = c.save();
    const auto v = c.view();
    assert(!c.load("x"));
    assert(!c.load(""));
    assert(!c.load("tgm1 garbage"));
    assert(!c.load("tgm1"));
    assert(!c.load("needs1 70 70 70 0 0 x x x")); // sans notre tag
    assert(c.save() == s);
    const auto w = c.view();
    assert(w.state == v.state && w.bouger == v.bouger && w.jouer == v.jouer && w.dormir == v.dormir);
}

void aller_retour_save_load()
{
    Controller a = started(at(15, 0));
    a.tick(1000);
    a.tick(1000 + 3 * 86400); // besoins au plus bas
    assert(a.view().bouger == 0);
    const std::string s = a.save();
    Controller b(kStale);
    assert(b.load(s));
    assert(b.save() == s);
    assert(b.view().bouger == 0 && b.view().jouer == 0 && b.view().dormir == 0);
}

// A3 : pas de cue des 22:00, meme au premier tick calme.
void a3_pas_de_cue_en_heures_calmes()
{
    Controller c = started(at(21, 59, 30), {Reminder{Category::Dodo, 21 * 60 + 45, 0x7F}});
    assert(c.tick(1000) == Cue::Wouf); // le rappel demarre a 21:59:30
    for (int64_t t = 1001; t < 1030; ++t) c.tick(t); // jusqu'a 21:59:59
    for (int64_t t = 1030; t < 1030 + 3600; ++t) assert(c.tick(t) == Cue::None);
    // SP5h : la reclamation de la promenade du soir survit au calme, en silence.
    assert(c.view().state == "reclame" && c.view().reminder == "dodo");
}

// R-n : le besoin rendu par Pup est applique.
void rn_besoin_du_rappel_applique()
{
    Controller c = started(at(12, 30), {Reminder{Category::Sortie, 12 * 60 + 30, 0x7F}});
    const std::string before = c.save();
    assert(c.tick(1000) == Cue::Wouf);
    c.on_input(Input::Laisse, 1001); // SP5h : la sortie se fait en laisse
    c.tick(1499);                    // donnees du PC perimees
    c.on_input(Input::Laisse, 1500); // sans donnees fraiches : reussie
    assert(c.view().state == "content");
    assert(c.save() != before);
}

// R-y : la planche a jouer vient du controleur (anim).
void anim_selon_la_cause()
{
    Controller c = started(at(15, 0));
    c.tick(1000);
    assert(c.view().anim == "repos");
    c.on_input(Input::Stroke, 1001);
    assert(c.view().state == "content");
    assert(c.view().anim == "roulade");
    c.on_input(Input::AnimDone, 1002);
    assert(c.view().anim == "repos");
    c.on_input(Input::Throw, 1003);
    assert(c.view().anim == "rapporte");
    c.on_input(Input::AnimDone, 1004);
    c.on_input(Input::Shake, 1005);
    assert(c.view().anim == "tournis");
    c.on_input(Input::AnimDone, 1006);
    c.on_input(Input::Brosse, 1007);
    assert(c.view().state == "bain" && c.view().anim == "bain");
    c.on_input(Input::AnimDone, 1008);
    c.on_input(Input::Os, 1009);
    assert(c.view().state == "mange" && c.view().anim == "os");

    Controller r = started(at(12, 0), {Reminder{Category::Repas, 12 * 60, 0x7F}});
    r.tick(1000);
    assert(r.view().anim == "aboie");
    r.on_input(Input::Tap, 1001);
    assert(r.view().state == "content");
    assert(r.view().anim == "content");

    Controller i = started(at(15, 0));
    DataFrame d = frame(at(15, 0));
    d.sage = 0;
    i.on_frame(d, 1000);
    i.tick(1000);
    assert(i.view().anim == "laisse");

    Controller w = started(at(15, 0));
    DataFrame e = frame(at(15, 0));
    e.claude_wait = true;
    w.on_frame(e, 1000);
    w.tick(1000);
    assert(w.view().anim == "patte");

    Controller n = started(at(23, 0));
    n.tick(1000);
    assert(n.view().anim == "dort");
}

// F1 : un rappel deja sonne ne reboule pas apres redemarrage.
void f1_rappel_deja_sonne_persiste()
{
    Controller a = started(at(12, 0, 0));
    assert(a.tick(1000) == Cue::Wouf);
    a.on_input(Input::Tap, 1002); // valide Repas
    assert(a.view().state == "content");
    const std::string s = a.save();
    Controller b(kStale);
    assert(b.load(s));
    b.on_link(true);
    b.on_frame(frame(at(12, 10)), 5000);
    for (int64_t t = 5000; t < 5100; t++) assert(b.tick(t) == Cue::None);
    assert(b.view().reason.empty());
    // ancienne ligne (besoins seuls) : chargee, memoire vide
    const std::string old = "tgm1 " + std::string(s, 5, s.find(" | ") - 5);
    Controller c(kStale);
    assert(c.load(old));
    c.on_link(true);
    c.on_frame(frame(at(12, 10)), 5000);
    assert(c.tick(5000) == Cue::Wouf);
}

// F2 : meme epoch re-envoye -> l'horloge continue d'avancer.
void f2_epoch_inchange_ne_resynchronise_pas()
{
    Controller c(kStale);
    c.on_link(true);
    c.on_frame(frame(at(11, 59, 0)), 1000);
    c.on_frame(frame(at(11, 59, 0)), 1010); // meme epoch, 10 s plus tard
    assert(c.tick(1010) == Cue::None);
    c.on_frame(frame(at(11, 59, 0)), 1060); // 12:00:00 attendu
    assert(c.tick(1060) == Cue::Wouf);
}

// F3 : deux rappels dus ensemble : le second attend la validation du premier.
void f3_un_rappel_a_la_fois()
{
    Controller c = started(at(12, 40));
    assert(c.tick(1000) == Cue::Wouf);
    assert(c.view().reminder == "repas");
    c.tick(1001);
    assert(c.view().reminder == "repas");
    c.on_input(Input::Tap, 1002);
    c.tick(1003);
    for (int64_t t = 1004; t < 1020; t++) c.tick(t); // fin de Content
    c.tick(1021);
    assert(c.view().reminder == "sortie" && c.view().state == "reclame");
}

void heures_calmes_reglees()
{
    pup::Settings s;
    s.calme_debut = 23 * 60;
    s.calme_fin = 6 * 60;
    Controller c(kStale, {});
    c.set_settings(s);
    c.on_link(true);
    c.on_frame(frame(at(22, 30)), 1000);
    c.tick(1000);
    assert(c.view().state == "repos"); // 22:30 n est plus calme
    c.tick(1000 + 30 * 60);            // 23:00
    assert(c.view().state == "dort");
    c.tick(1000 + 90 * 60);            // 00:00
    assert(c.view().state == "dort");
    c.tick(1000 + 450 * 60 - 60);      // 05:59
    assert(c.view().state == "dort");
    c.tick(1000 + 450 * 60);           // 06:00
    assert(c.view().state == "repos");
}

void heures_calmes_debut_egal_fin()
{
    pup::Settings s;
    s.calme_debut = s.calme_fin = 3 * 60;
    Controller c(kStale, {});
    c.set_settings(s);
    c.on_link(true);
    c.on_frame(frame(at(2, 0)), 1000);
    for (int m = 0; m < 24 * 60; m += 30) {
        c.tick(1000 + m * 60);
        assert(c.view().state == "repos");
    }
}

void nuit_reglee()
{
    pup::Settings s;
    s.nuit_debut = 22 * 60;
    s.nuit_fin = 5 * 60;
    Controller c(kStale);
    c.set_settings(s);
    c.on_link(true);
    c.on_frame(frame(at(21, 59)), 1000);
    c.tick(1000);
    assert(!c.view().night);
    c.tick(1060); // 22:00
    assert(c.view().night);
    c.on_frame(frame(at(5, 0, 0, kDay + 1)), 2000);
    c.tick(2000);
    assert(!c.view().night);

    s.nuit_debut = s.nuit_fin = 0;
    c.set_settings(s);
    c.tick(2001);
    assert(!c.view().night);
    assert(c.settings().nuit_debut == 0);
}

// I4 (relecture finale) : l'etat est charge avant la config (board : NVS ;
// desktop : ligne cfg du daemon). Les rappels deja sonnes de la liste
// [[rappel]] ne doivent pas resonner apres un redemarrage.
void rappels_sonnes_gardes_quand_la_config_arrive_apres_l_etat()
{
    pup::Settings s;
    s.rappels = {Reminder{Category::Sortie, 17 * 60 + 30, 0x7F}, Reminder{Category::Repas, 19 * 60, 0x7F}};
    Controller a = started(at(17, 30));
    a.set_settings(s);
    assert(a.tick(1000) == Cue::Wouf); // la sortie sonne
    const std::string etat = a.save();

    Controller b(kStale); // redemarrage : liste compilee, puis etat, puis config
    b.on_link(true);
    assert(b.load(etat));
    b.set_settings(s);
    b.on_frame(frame(at(18, 0)), 2000);
    for (int64_t t = 2000; t < 2100; ++t) assert(b.tick(t) == Cue::None);
    assert(b.view().state == "repos");
}

} // namespace

// SP5d2 B11 : en ligne = donnees du PC utilisables (age <= stale_s), quel que soit le lien.
void en_ligne_suit_la_peremption()
{
    Controller c(kStale);
    c.tick(500);
    assert(c.view().en_ligne); // demarrage : delai de grace, pas d'icone tout de suite
    c.tick(500 + kStale);
    assert(c.view().en_ligne);
    c.tick(500 + kStale + 1);
    assert(!c.view().en_ligne); // rien recu depuis stale_s : hors ligne
    c.on_link(true);
    c.on_frame(frame(at(15, 0)), 1000);
    c.tick(1000);
    assert(c.view().en_ligne);
    c.tick(1000 + kStale);
    assert(c.view().en_ligne);
    c.tick(1000 + kStale + 1);
    assert(!c.view().en_ligne);
    c.on_frame(frame(at(15, 1)), 1000 + kStale + 2);
    assert(c.view().en_ligne);
}

// Heure affichee en haut a gauche : HH:MM local, -1 sans horloge.
void heure_affichee()
{
    Controller sans(30, {});
    assert(sans.view().heure == -1 && sans.view().minute == -1);
    Controller c = started(at(9, 5, 0));
    assert(c.view().heure == 9 && c.view().minute == 5);
    c.tick(1000 + 60 * 60 + 56 * 60); // 1 h 56 plus tard, sans nouvelle trame
    assert(c.view().heure == 11 && c.view().minute == 1);
}

// ---- Lien avec le PC (SPEC-001) ----

using pup::Commande;
using pup::EvtKind;

const Reminder kSortie{Category::Sortie, 12 * 60 + 30, 0x7F};

Commande cmd(Commande::Kind k, const char *rappel = "") { return Commande{k, rappel}; }

// Controleur a 12:30:00 (mono 1000) avec une seule sortie, deja reclamee.
Controller sortie_reclamee()
{
    Controller c = started(at(12, 30), {kSortie});
    c.tick(1000);
    assert(c.view().state == "reclame" && c.view().reminder == "sortie");
    return c;
}

void commandes_chatouille_et_os()
{
    Controller c = started(at(15, 0));
    c.on_command(cmd(Commande::Kind::Chatouille), 1001);
    auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Chatouille && e[0].source == "pc");
    assert(c.view().state == "content" && c.view().anim == "roulade");
    assert(c.take_events().empty());
    c.on_input(Input::AnimDone, 1002);
    c.on_command(cmd(Commande::Kind::Os), 1003);
    e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Os && e[0].source == "pc");
    assert(c.view().state == "mange");
    // Le besoin Jouer est satisfait par la chatouille du PC comme par celle de la board.
    assert(c.etat_board().jouer == 90);

    // Sur la board : source "board".
    Controller b = started(at(15, 0));
    b.on_input(Input::Stroke, 1001);
    b.on_input(Input::AnimDone, 1002);
    b.on_input(Input::Os, 1003);
    e = b.take_events();
    assert(e.size() == 2 && e[0].kind == EvtKind::Chatouille && e[0].source == "board");
    assert(e[1].kind == EvtKind::Os && e[1].source == "board");
}

void commande_bain()
{
    Controller c = started(at(15, 0));
    c.on_command(cmd(Commande::Kind::Bain), 1001);
    auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Bain && e[0].source == "pc");
    assert(c.view().state == "bain" && c.view().anim == "bain");

    // La brosse de la board : source "board".
    Controller b = started(at(15, 0));
    b.on_input(Input::Brosse, 1001);
    e = b.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Bain && e[0].source == "board");

    // Il dort : bain ignore.
    Controller d = started(at(15, 0));
    d.on_input(Input::FaceDown, 1001);
    d.on_command(cmd(Commande::Kind::Bain), 1002);
    assert(d.take_events().empty() && d.view().state == "dort");
}

void commandes_ignorees_sans_evenement()
{
    // En reclamation, en dodo, en promenade : chatouille et os ignores.
    Controller r = sortie_reclamee();
    r.on_command(cmd(Commande::Kind::Chatouille), 1001);
    r.on_command(cmd(Commande::Kind::Os), 1002);
    assert(r.take_events().empty() && r.view().state == "reclame");

    Controller d = started(at(15, 0));
    d.on_input(Input::FaceDown, 1001);
    d.on_command(cmd(Commande::Kind::Chatouille), 1002);
    d.on_command(cmd(Commande::Kind::Os), 1003);
    assert(d.take_events().empty() && d.view().state == "dort");

    Controller p = started(at(15, 0));
    p.on_input(Input::Laisse, 1001);
    p.on_command(cmd(Commande::Kind::Chatouille), 1002);
    p.on_command(cmd(Commande::Kind::Os), 1003);
    p.on_command(cmd(Commande::Kind::Accepter, "sortie,1230"), 1004);
    p.on_command(cmd(Commande::Kind::Reporter, "promenade"), 1005);
    assert(p.take_events().empty() && p.view().state == "promenade");

    // Id non reclame, repas, jauge sans reclamation de jauge : ignores.
    Controller s = sortie_reclamee();
    for (const char *id : {"sortie,1231", "sport,1230", "repas,1230", "promenade", "", "n'importe quoi"}) {
        s.on_command(cmd(Commande::Kind::Accepter, id), 1001);
        s.on_command(cmd(Commande::Kind::Reporter, id), 1002);
    }
    assert(s.take_events().empty() && s.view().state == "reclame");

    Controller repas = started(at(12, 0));
    repas.tick(1000);
    assert(repas.view().reminder == "repas");
    repas.on_command(cmd(Commande::Kind::Accepter, "repas,1200"), 1001);
    repas.on_command(cmd(Commande::Kind::Reporter, "repas,1200"), 1002);
    assert(repas.take_events().empty() && repas.view().state == "reclame");
}

void commande_accepter_sortie()
{
    Controller c = sortie_reclamee();
    c.on_command(cmd(Commande::Kind::Accepter, "sortie,1230"), 1010);
    const auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Accepte && e[0].rappel == "sortie,1230");
    assert(c.view().state == "promenade");
    assert(c.etat_board().rappel == "-");
}

void commande_reporter_sortie_puis_accepter()
{
    Controller c = sortie_reclamee();
    c.on_command(cmd(Commande::Kind::Reporter, "sortie,1230"), 1010);
    auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Reporte && e[0].rappel == "sortie,1230");
    // Reporte : le rappel reste cite dans l'etat, le chiot est au repos.
    const pup::EtatBoard b = c.etat_board();
    assert(b.etat == "repos" && b.raison == "-" && b.rappel == "sortie,1230");
    // Un second report n'en est pas un ; l'acceptation, si.
    c.on_command(cmd(Commande::Kind::Reporter, "sortie,1230"), 1011);
    assert(c.take_events().empty());
    c.on_command(cmd(Commande::Kind::Accepter, "sortie,1230"), 1012);
    e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Accepte && e[0].rappel == "sortie,1230");
    assert(c.view().state == "promenade");
    // Le rappel reporte n'est plus la apres l'acceptation.
    c.on_command(cmd(Commande::Kind::Accepter, "sortie,1230"), 1013);
    assert(c.take_events().empty());
}

void gestes_board_sur_la_sortie_emettent()
{
    Controller c = sortie_reclamee();
    c.on_input(Input::LongPress, 1010);
    auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Reporte && e[0].rappel == "sortie,1230");
    // Le rappel revient apres report_min (10 min) : reclame de nouveau, id inchange.
    c.tick(1010 + 600);
    assert(c.view().state == "reclame" && c.etat_board().rappel == "sortie,1230");
    c.on_input(Input::Laisse, 1011 + 600);
    e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Accepte && e[0].rappel == "sortie,1230");
}

void commandes_de_la_jauge()
{
    auto vide = [] {
        Controller c = started(at(15, 0));
        DataFrame d = frame(at(15, 0));
        d.sage = 0;
        c.on_frame(d, 1000);
        c.tick(1000);
        assert(c.view().reason == "promenade");
        return c;
    };
    Controller a = vide();
    assert(a.etat_board().etat == "reclame" && a.etat_board().raison == "promenade");
    a.on_command(cmd(Commande::Kind::Accepter, "promenade"), 1001);
    auto e = a.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Accepte && e[0].rappel == "promenade");
    assert(a.view().state == "promenade");

    Controller r = vide();
    r.on_command(cmd(Commande::Kind::Reporter, "promenade"), 1001);
    e = r.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Reporte && e[0].rappel == "promenade");
    assert(r.view().state == "repos");
    // Pas de nouvelle reclamation de jauge avant report_min (600 s), meme jauge a 0.
    DataFrame d = frame(at(15, 0));
    d.sage = 0;
    for (int64_t t = 1002; t < 1001 + 600; t += 5) {
        r.on_frame(d, t);
        r.tick(t);
        assert(r.view().state == "repos");
    }
    r.on_frame(d, 1001 + 600);
    r.tick(1001 + 600);
    assert(r.view().state == "reclame" && r.view().reason == "promenade");

    // Appui long sur la board : meme chose.
    Controller b = vide();
    b.on_input(Input::LongPress, 1001);
    e = b.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Reporte && e[0].rappel == "promenade");
}

void abandon_a_l_entree_des_heures_calmes()
{
    Controller c = started(at(21, 50), {Reminder{Category::Sortie, 21 * 60 + 50, 0x7F}});
    c.tick(1000);
    assert(c.view().state == "reclame");
    assert(c.take_events().empty());
    c.tick(1000 + 10 * 60); // 22:00 : heures calmes
    const auto e = c.take_events();
    assert(e.size() == 1 && e[0].kind == EvtKind::Abandonne && e[0].rappel == "sortie,2150");
    assert(c.view().state == "dort");
    assert(c.etat_board().rappel == "-");
    c.tick(1000 + 11 * 60);
    assert(c.take_events().empty());

    // Rappel repas : aucun evenement.
    Controller r = started(at(21, 50), {Reminder{Category::Repas, 21 * 60 + 50, 0x7F}});
    r.tick(1000);
    r.tick(1000 + 10 * 60);
    assert(r.take_events().empty());
}

void etat_pour_le_pc()
{
    Controller c = started(at(15, 0));
    pup::EtatBoard e = c.etat_board();
    assert(e.etat == "repos" && e.raison == "-" && e.rappel == "-" && e.promenade_debut == "-");
    assert(e.pile == -1 && e.bouger == 70 && e.jouer == 70 && e.dormir == 70);
    c.on_battery(3800);
    assert(c.etat_board().pile == 50);
    c.on_battery(-1);
    assert(c.etat_board().pile == -1);

    // Promenade : heure locale du debut.
    c.on_input(Input::Laisse, 1000 + 65); // 15:01:05
    e = c.etat_board();
    assert(e.etat == "promenade" && e.promenade_debut == "15:01");

    // Reclamation de rappel : raison et id.
    Controller s = sortie_reclamee();
    e = s.etat_board();
    assert(e.etat == "reclame" && e.raison == "rappel" && e.rappel == "sortie,1230");

    // Claude : raison claude, sans rappel.
    Controller w = started(at(15, 0));
    DataFrame d = frame(at(15, 0));
    d.claude_wait = true;
    w.on_frame(d, 1000);
    w.tick(1000);
    e = w.etat_board();
    assert(e.etat == "reclame" && e.raison == "claude" && e.rappel == "-");

    // Les besoins suivent les niveaux 0..100.
    Controller n = started(at(15, 0));
    n.on_input(Input::Throw, 1001);
    assert(n.etat_board().jouer == 90);

    // Etats de la planche : mange, dort.
    Controller m = started(at(15, 0));
    m.on_input(Input::Os, 1001);
    assert(m.etat_board().etat == "mange");
    m.on_input(Input::AnimDone, 1002);
    m.on_input(Input::FaceDown, 1003);
    assert(m.etat_board().etat == "dort");
}

// SPEC-012 : patte puis aboie, pose dans la ligne d'etat.
void claude_escalade_et_pose_dans_l_etat()
{
    Controller c = started(at(15, 0));
    assert(c.etat_board().pose == "-");
    DataFrame d = frame(at(15, 0));
    d.claude_wait = true;
    c.on_frame(d, 1000);
    c.tick(1000);
    assert(c.view().anim == "patte" && c.etat_board().pose == "patte");
    for (int64_t t = 1001; t < 1120; t += 10) { // donnees rafraichies : pas perimees
        c.on_frame(d, t);
        assert(c.tick(t) != Cue::Aboie);
        assert(c.etat_board().pose == "patte");
    }
    c.on_frame(d, 1119);
    assert(c.etat_board().pose == "patte");
    c.on_frame(d, 1120);
    assert(c.tick(1120) == Cue::Aboie);
    assert(c.view().anim == "aboie" && c.etat_board().pose == "aboie");
    assert(c.etat_board().raison == "claude");

    // reglage du delai pris en compte
    Controller r = started(at(15, 0));
    pup::Settings s = r.settings();
    s.patte_avant_aboie_min = 1;
    r.set_settings(s);
    r.on_frame(d, 1000);
    r.tick(1000);
    r.on_frame(d, 1059);
    assert(r.tick(1059) != Cue::Aboie && r.etat_board().pose == "patte");
    r.on_frame(d, 1060);
    assert(r.tick(1060) == Cue::Aboie && r.etat_board().pose == "aboie");

    // autres reclamations
    Controller sortie = sortie_reclamee();
    assert(sortie.etat_board().pose == "laisse");
    Controller repas = started(at(12, 0), {Reminder{Category::Repas, 12 * 60, 0x7F}});
    repas.tick(1000);
    assert(repas.etat_board().pose == "aboie");
    Controller jauge = started(at(15, 0));
    DataFrame v = frame(at(15, 0));
    v.sage = 0;
    jauge.on_frame(v, 1000);
    jauge.tick(1000);
    assert(jauge.etat_board().pose == "laisse");
}

// SPEC-012 / ADR-007 : la pose aboie 3 s a la relance, planche et ligne d'etat.
void relance_en_laisse_pose_aboie_trois_secondes()
{
    Controller c = sortie_reclamee();
    assert(c.view().anim == "laisse" && c.etat_board().pose == "laisse");
    const EtatBoard avant = c.etat_board();
    assert(c.tick(1059) != Cue::Aboie && c.view().anim == "laisse");
    assert(c.tick(1060) == Cue::Aboie);
    assert(c.view().anim == "aboie" && c.etat_board().pose == "aboie");
    assert(!(c.etat_board() == avant)); // un changement de pose fait emettre la ligne
    c.tick(1062);
    assert(c.view().anim == "aboie" && c.etat_board().pose == "aboie");
    const EtatBoard relance = c.etat_board();
    c.tick(1063);
    assert(c.view().anim == "laisse" && c.etat_board().pose == "laisse");
    assert(!(c.etat_board() == relance)); // le retour a laisse emet aussi une ligne
}

int main()
{
    claude_escalade_et_pose_dans_l_etat();
    relance_en_laisse_pose_aboie_trois_secondes();
    commandes_chatouille_et_os();
    commande_bain();
    commandes_ignorees_sans_evenement();
    commande_accepter_sortie();
    commande_reporter_sortie_puis_accepter();
    gestes_board_sur_la_sortie_emettent();
    commandes_de_la_jauge();
    abandon_a_l_entree_des_heures_calmes();
    etat_pour_le_pc();
    rappels_sonnes_gardes_quand_la_config_arrive_apres_l_etat();
    heure_affichee();
    en_ligne_suit_la_peremption();
    sans_horloge_aucun_rappel();
    rappel_a_l_heure();
    donnees_perimees_eteignent_f2_et_f7();
    age_effectif_grandit_sans_frame();
    jauge_transmise_au_chiot();
    rappels_de_la_config_appliques();
    batterie_faible_sous_le_seuil();
    laisse_visible_sauf_dort();
    bulle_dodo_pendant_la_reclamation_dodo();
    promenade_survit_au_redemarrage();
    retourne_la_nuit_remonte_dormir();
    nuit_et_icones();
    etat_corrompu_ignore();
    aller_retour_save_load();
    a3_pas_de_cue_en_heures_calmes();
    rn_besoin_du_rappel_applique();
    anim_selon_la_cause();
    f1_rappel_deja_sonne_persiste();
    f2_epoch_inchange_ne_resynchronise_pas();
    f3_un_rappel_a_la_fois();
    heures_calmes_reglees();
    heures_calmes_debut_egal_fin();
    nuit_reglee();
    return 0;
}
