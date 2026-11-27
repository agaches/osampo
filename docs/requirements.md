# Exigences : le chiot sur le bureau du PC

*Document de travail en français. Chaque REQ est couverte par `tests/acceptance/test_reqNNN_*.py`.*

Exigences du chien côté PC (barre des tâches, réglages, rappels), figées le 2026-11-22 à partir
d'un brouillon de travail et des réponses de l'auteur aux questions Q1-Q6.

## Contexte

Le daemon (Rust, service systemd utilisateur) calcule la jauge « sage », l'absence et les
rappels, relit `~/.config/osampo/osampo.toml` toutes les 2 s, refuse un fichier
invalide en entier, et écrit vers la board 1.83" (chiot `osampo`) par BLE, dans un seul sens.
Le PC est en X11 / Cinnamon ; `xapp-sn-watcher` tourne (zone de notification
StatusNotifierItem).

## Décisions de l'auteur (2026-11-22)

Les ADR citées sont dans [`docs/adr/`](adr/).

| Ref | Décision |
|---|---|
| Q1 | Le chien qui sort du coin de l'écran fait partie de ce lot. |
| Q2 | Le chien du PC et celui de la board sont le même chien : synchro dans les deux sens (board → PC compris). |
| Q3 | Chatouiller ou donner un os sur le PC est envoyé à la board, qui réagit. |
| Q4 | Le rappel de sortie réclame sur le PC et sur la board en même temps ; accepter ou reporter d'un côté vaut pour les deux. |
| Q5 | Réglages : programme Slint à part, qui écrit `osampo.toml` ; le daemon le relit déjà. |
| Q6 | Réglages : temps et inactivité en premier, le reste dans un onglet « avancé ». |
| QA, QB | Board hors ligne et déclencheurs du rappel de sortie : ADR-001. |
| Jauge, aide | Jauge réglable depuis le PC avec synchro du chiot, aide des réglages : ADR-002. |
| Desktop pet | Chien toujours posé sur la barre des tâches, déplaçable, taille réglable : ADR-003. |
| Pose | Même pose que la board quand il réclame (patte pour Claude) : ADR-004. |
| Coin | Plus de chien qui sort du coin : seul le chien posé réclame au rappel : ADR-008 (remplace ADR-005). |
| Escalade | Patte puis aboiement si ignoré, pose de la board copiée par le PC : ADR-006. |
| Relances | La board aboie quelques secondes à chaque relance d'un rappel en laisse : ADR-007. |

## Exigences

### REQ-001 : icône du chien dans la zone de notification

Une icône de chien dans la zone de notification de Cinnamon suit l'état du chiot : assis au
repos, dans sa niche quand il dort, qui aboie quand il réclame, en laisse en promenade. Quand il
réclame, il prend la même pose que la board : patte levée, laisse dans la gueule ou aboie
(ADR-004, ADR-006). L'icône
est dessinée pour 22-24 px (pas une réduction des sprites 1.83").

Critère : pour chacun des quatre états, l'icône affichée correspond à l'état et change en moins
de 5 s après le changement d'état côté daemon ou côté board.

### REQ-002 : statuts dans l'infobulle et le menu

L'infobulle et le menu de l'icône montrent les statuts courants.

Critère : l'infobulle ou le menu affichent la jauge sage, l'heure du prochain rappel, la
promenade en cours (oui / non, depuis quand), l'état du lien BLE (connecté / coupé) et la pile
de la board (en %, ou « inconnue » si le lien est coupé).

### REQ-003 : fenêtre de réglages

Un clic sur l'icône (ou une entrée du menu) ouvre un programme Slint à part qui édite
`osampo.toml`.

Critère : chaque clé de `osampo.toml` est modifiable, avec ses bornes affichées ; les temps et
l'inactivité sont sur le premier onglet, le reste sur un onglet « avancé » ; une valeur hors
bornes n'est pas enregistrable ; enregistrer produit un fichier que le daemon accepte et relit ;
les commentaires du fichier sont conservés ; une section `[poste]` est réécrite en
`[promenade]` avec les mêmes valeurs.

### REQ-004 : rappel de sortie sur le PC

Quand le chiot réclame une sortie, le PC envoie une notification de bureau avec le chien qui
aboie et le son wouf, en même temps que la board. Une réclamation de sortie est un rappel de
catégorie sortie, sport ou dodo (tous ceux qui partent en laisse), ou la réclamation de
promenade quand la jauge sage tombe à 0 (ADR-001).

Critère : la notification apparaît au même rappel que celui envoyé à la board ; elle propose
« accepter » et « reporter » ; l'une ou l'autre action modifie le rappel du daemon, et la board
cesse de réclamer en moins de 5 s ; inversement, une sortie acceptée ou reportée sur la board
retire la notification du PC en moins de 5 s.

### REQ-005 : au rappel de sortie, le chien de la barre réclame

À chaque réclamation de sortie (au sens de REQ-004), le chien posé sur la barre (REQ-011) prend la
pose de la board (laisse dans la gueule, aboie quelques secondes à chaque relance), et la
notification accepter/reporter (REQ-004) est affichée. Aucun autre chien n'apparaît (ADR-001,
ADR-006, ADR-008, qui remplace ADR-005).

Critère : au rappel, en moins de 5 s, le chien posé sur la barre suit la pose de la board
(laisse, « aboie » quelques secondes à chaque relance ; « aboie » si la board est hors ligne),
sans prendre le focus du clavier, et aucune autre fenêtre du chien n'apparaît ; dès que le rappel
est accepté ou reporté (sur le PC ou sur la board), il revient à l'état du chiot, sans
disparaître.

### REQ-006 : chatouiller, donner un os ou un bain depuis le PC

Depuis le PC (menu de l'icône ou fenêtre du chien), on peut chatouiller le chien, lui donner
un os ou un bain (la brosse de la board).

Critère : le chien du PC réagit (sprite et son) ; la commande est envoyée à la board par le
daemon et la board joue la même réaction qu'un geste fait sur elle, en moins de 5 s si le lien
BLE est connecté ; lien coupé, le PC le signale et la board ne reçoit rien.

### REQ-007 : synchro de la board vers le PC

Ce qui se passe sur la board (os donné, chatouille, dodo, réveil, promenade commencée ou
finie, besoins, niveau de pile) remonte au PC.

Critère : un geste ou un changement d'état sur la board se reflète dans l'icône et les statuts
du PC en moins de 5 s ; lien coupé, le PC garde le dernier état reçu de la board et indique que
la board est hors ligne, sauf quand une réclamation de sortie du daemon est en cours, où il
aboie (ADR-001).

### REQ-008 : heures calmes

Les heures calmes de `[heures] calme_*` valent aussi pour le PC.

Critère : pendant les heures calmes, aucun son n'est joué par le PC (notification, chatouille,
os) ; les notifications et animations restent affichées.

### REQ-009 : régler la jauge sage depuis le PC

Depuis le PC (menu de l'icône ou fenêtre de réglages), on peut remplir la jauge sage, la vider
ou la fixer à une valeur ; le chiot de la board suit (ADR-002).

Critère : « remplir » met la jauge à 100, « vider » à 0, « fixer » à une valeur entière de 0 à
100 (une valeur hors bornes est refusée) ; l'infobulle du PC affiche la nouvelle valeur en moins
de 5 s ; la board reçoit la nouvelle valeur et le chiot réagit comme si la jauge l'avait atteinte
seule, en moins de 5 s si le lien est connecté (une jauge vidée déclenche la réclamation de
promenade) ; la valeur réglée survit à un redémarrage du daemon ; ensuite la jauge reprend son
évolution normale à partir de cette valeur.

### REQ-010 : aide des réglages

Chaque réglage de la fenêtre de réglages est expliqué.

Critère : chaque clé de `osampo.toml` affichée par la fenêtre a un libellé en français et une
phrase d'aide qui dit son rôle, son unité et l'effet d'une valeur haute ou basse ; l'aide
s'affiche même si les commentaires du fichier ont été supprimés ; le mode ligne de commande
`--lister` donne les mêmes textes.

### REQ-011 : le chien posé sur la barre des tâches (desktop pet)

Le chien est toujours visible, posé au-dessus de la barre des tâches, au-dessus de la zone de
notification, et joue les animations du chiot de la board (ADR-003).

Critère : au lancement de `osampo-pc`, une fenêtre sans bord, sans entrée dans la barre des
tâches et au premier plan montre le chien, son bas posé sur le haut de la barre, au-dessus de la
zone de notification ; elle reste visible tant que le programme tourne. L'animation suit l'état
du chiot en moins de 5 s : assis au repos, dans sa niche quand il dort, aboie quand il réclame,
joue ou mange l'os après un geste, absent (laisse seule) pendant une promenade ; quand il réclame,
même pose que la board (patte levée, laisse dans la gueule ou aboie, ADR-004, ADR-006). Clic gauche =
chatouiller (comme REQ-006), clic droit = le menu de l'icône, glisser-déposer = déplacer le chien
le long de la barre, position retenue après un redémarrage. La fenêtre ne prend jamais le focus du
clavier. Sa taille se règle dans `osampo.toml` (64 px par défaut, bornes affichées dans la
fenêtre de réglages avec son aide), et une nouvelle taille s'applique sans relancer le programme.

### REQ-012 : la patte, puis l'aboiement si on l'ignore

Sur la board, quand le chiot réclame parce que Claude attend, il lève d'abord la patte, puis
aboie s'il est ignoré trop longtemps ; le chien du PC suit (ADR-006).

Critère : une réclamation « Claude attend » commence par la pose patte levée ; si elle n'est pas
traitée après le délai réglé dans `osampo.toml` (2 min par défaut, bornes et aide affichées dans
la fenêtre de réglages), la board passe à la pose « aboie » (sprite et son) ; la réclamation
traitée avant le délai ne fait jamais aboyer ; le chien du PC (icône et chien posé) passe de la
patte à l'aboiement en moins de 5 s après la board.

De même, pendant une réclamation en laisse (rappel sortie, sport, dodo, jauge vide), chaque
relance fait passer la board à la pose « aboie » quelques secondes (son compris), puis elle
reprend la laisse ; le chien du PC suit (ADR-007).

### REQ-013 : quitter Osampo depuis le PC

Le menu de l'icône (et donc celui du chien, REQ-011) se termine par « Quitter Osampo », toujours
cliquable, même daemon injoignable.

Critère : le clic arrête le daemon puis `osampo-pc` (icône et chien disparaissent) en moins de
5 s ; le socket du daemon est supprimé et le daemon sort en code 0, donc l'unité systemd
(`Restart=on-failure`) ne le relance pas ; tout repart à la prochaine ouverture de session.
Daemon injoignable : `osampo-pc` quitte quand même.

## Hors périmètre

- Wayland (le PC est en X11 / Cinnamon).
- Réglages de la board 3.49" ou d'autres widgets.
