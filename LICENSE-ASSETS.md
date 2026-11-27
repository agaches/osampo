# Licenses and credits

*[Version française plus bas](#licences-et-crédits).*

Osampo's **source code** is under the [MIT License](LICENSE). Some **artwork** and the
**user-interface toolkit** come with their own licenses, listed here.

## Artwork

| Files | Author | License | Source |
|---|---|---|---|
| Front-facing puppy poses: `widgets/osampo/ui/sprites/` `repos.png`, `patte.png`, `content.png`, `aboie.png`, `boude.png`, `tournis.png`, `laisse.png`, `lache.png`, `bain.png`, `bain_mousse.png`, `bain_fin.png`, `os.png`, `os_fin.png`; the contact sheet `widgets/osampo/tools/planche_contact.png`; the demo clips in `docs/media/` that show them; the original `widgets/osampo/assets/third_party/strawheart_animals_and_tea/pixel-ani-dog.gif`; their copy in `claude-mod/hooks/sprites.gen.ts`, `claude-mod/.claude-plugin/icon.png` and `docs/media/claude-mod.webp` | **strawheart** ([mtsids.com](http://mtsids.com/)), *Animals and Tea* | [CC-BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/) | [OpenGameArt](https://opengameart.org/content/animals-and-tea-by-strawheart) |
| Puppy sounds: `widgets/osampo/sounds/wouf.wav`, `aboie.wav`, `gemit.wav`; copies in `claude-mod/assets/` | **AntumDeluge** | [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/) | [Freesound #583142](https://freesound.org/s/583142/) |
| All other sprites (side-view puppy, walk/trot/gallop cycles, kennel, backgrounds, icons), tray icons in `crates/osampo-pc/icons/`, the synthesized yap `widgets/osampo/sounds/jappe.wav` | Osampo contributors | MIT | drawn in code: `widgets/osampo/tools/gen_assets.py`, `crates/osampo-pc/tools/gen_icones.py` |

The front-facing poses are **derivative works** of strawheart's puppy and stay under
CC-BY-SA 3.0: you may reuse them if you credit strawheart and share your changes under the
same license. The firmware and the desktop app embed these images. Details of what was
derived and how: [`widgets/osampo/assets/third_party/strawheart_animals_and_tea/SOURCE.md`](widgets/osampo/assets/third_party/strawheart_animals_and_tea/SOURCE.md).

**References, nothing copied:**
- the [*dog* sprite pack by rmazanek / inmenus](https://rmazanek.itch.io/dog) (CC0) was
  examined and set aside ([why](widgets/osampo/assets/third_party/rmazanek_dog/SOURCE.md));
- the walk cycle follows the principles of a pixel-art dog walk-cycle tutorial by
  **Slynyrd** ([slynyrd.com](https://www.slynyrd.com/)): straight legs on the ground, bent
  only in the air. No pixels were taken from it.

## Software

| Component | Used for | License |
|---|---|---|
| [Slint](https://slint.dev) | UI of the firmware, the desktop simulator and the settings app | GPL-3.0-only **or** Slint Royalty-free 2.0 **or** Slint Software 3.0 |
| [ESP-IDF](https://github.com/espressif/esp-idf) and Espressif components (`esp_lcd_touch`, `esp_lcd_touch_cst816s`) | firmware | Apache-2.0 |
| [cJSON](https://github.com/DaveGamble/cJSON) | JSON lines (desktop build) | MIT |
| Rust crates: tokio, zbus, serde, toml, anyhow, uuid… | daemon and desktop apps | mostly MIT and/or Apache-2.0; some indirect dependencies use BSD-3-Clause, Unicode-3.0, Zlib, ISC or BSL-1.0 (`cargo metadata` lists each) |
| [btleplug](https://github.com/deviceplug/btleplug) | Bluetooth link | MIT / Apache-2.0 / BSD-3-Clause |
| [ksni](https://github.com/iovxw/ksni) | tray icon | Unlicense |
| [LikeC4](https://likec4.dev) | architecture diagrams (run with `npx`, not shipped) | MIT |

**About Slint on the board.** For an embedded system (a device that does one job, like this
puppy), the only free Slint license is the **GPLv3**: the Royalty-free license excludes
embedded systems, and the Slint Software License needs a paid plan. Osampo's own files stay
MIT, as Slint allows, but a **firmware binary you distribute** is a GPLv3 work: distribute it
with its source. The desktop apps may use Slint under the GPLv3 or the Royalty-free license
(which asks you to show that the app uses Slint). See
[Slint's license FAQ](https://github.com/slint-ui/slint/blob/master/FAQ.md#licensing).

---

# Licences et crédits

Le **code source** d'Osampo est sous [licence MIT](LICENSE). Certains **dessins** et la
**bibliothèque d'interface** ont leur propre licence :

- **Poses de face du chiot** (13 planches listées plus haut, la planche de contact et le GIF
  d'origine, leur copie dans `claude-mod/hooks/sprites.gen.ts`, `claude-mod/.claude-plugin/icon.png` et
  `docs/media/claude-mod.webp`) : dérivées du chiot de **strawheart** ([mtsids.com](http://mtsids.com/)),
  *Animals and Tea*, sous [CC-BY-SA 3.0](https://creativecommons.org/licenses/by-sa/3.0/).
  Réutilisables en citant strawheart et en partageant les modifications sous la même licence.
- **Sons** : enregistrement de **AntumDeluge** sur
  [Freesound #583142](https://freesound.org/s/583142/), [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).
- **Tous les autres dessins** (chiot de profil, cycles de marche, trot et galop, niche,
  fonds, icônes) et le jappement de synthèse `jappe.wav` : faits dans le code, licence MIT.
- **Références sans reprise** : le pack *dog* de rmazanek / inmenus (CC0, examiné et écarté)
  et le tutoriel de cycle de marche de **Slynyrd** (principes seulement).
- **Logiciels** : tableau ci-dessus. Sur la board, la seule licence Slint gratuite est la
  GPLv3 (la licence Royalty-free exclut l'embarqué, la licence Slint Software est payante) :
  le code d'Osampo reste MIT, mais un firmware distribué est une œuvre GPLv3, à distribuer
  avec ses sources.
