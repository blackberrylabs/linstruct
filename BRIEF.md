# BRIEF — Linstruct
<!-- Project brief (French). Code, comments, commits and README are in English. -->

Rev. 1 — 2026-10-17 — validé par Alexandre Do (« Valide, go. »).

Module n° 5 de la famille Blackberrylabs. Verbe : **lint**. Une phrase :

> **Linstruct montre vos fichiers d'instructions IA (CLAUDE.md, AGENTS.md, .cursor/rules,
> copilot-instructions, GEMINI.md…) tels que chaque agent les lira réellement — et ce qu'il
> laissera tomber sans rien dire.**

Tagline (anglais, carte de la tour) : *Your AI instruction files, as each agent will actually
read them.*

## 1. Le problème

Un développeur ou un vibe codeur écrit ses consignes dans des fichiers Markdown. Chaque hôte
les lit **à sa manière** — pas les mêmes noms, emplacements, limites, ordre — et quand une
limite est franchie, il **coupe ou ignore sans prévenir** (`synthese_sources.md`, 19 hôtes) :

- **Codex** : 32 768 octets **au total** pour les AGENTS.md d'un projet ; le fichier qui dépasse
  est **coupé au milieu**, les suivants sautés ; seul un log `tracing` le dit.
- **Claude Code** : > 4 MiB sauté ; avertissement au-delà de 40 000 caractères (seuil codé en
  dur, non documenté) ; `AGENTS.md` lu **seulement sans CLAUDE.md** au-dessus ;
  `AGENTS.local.md`, `AGENTS.override.md`, `.agents/` jamais lus ; `@import` à 4 niveaux, un
  chemin entre guillemets **n'est pas importé** ; dans `.claude/rules/`, seul `paths` compte,
  YAML invalide → la règle s'applique **partout**.
- **Cursor** : `.md` dans `.cursor/rules/` **ignoré** ; `.mdc` sans `alwaysApply`/`globs`/
  `description` appliqué **seulement à la main** ; frontmatter mal fermé → rien ne charge.
- **Devin Desktop** : 12 000 caractères par règle, 6 000 pour le fichier global — au-delà,
  comportement **non documenté**.
- **Goose** : imports non dépliés au-delà de 128 Ko, import hors budget **sauté**. **Kiro CLI** :
  au-delà de 75 % de la fenêtre, **abandonné**. **Trae** : 4e niveau de dossier « unreadable ».
- **Ombres** : `WARP.md` remplace `AGENTS.md` (Warp) ; `.devin/rules` remplace
  `.windsurf/rules` ; `.junie/AGENTS.md` fait ignorer tout le reste (Junie) ; Zed ne prend que
  le **premier** de neuf noms ; `AGENTS.override.md` remplace `AGENTS.md` (Codex).
- **Tous** : contradictions → « Claude may pick one arbitrarily », Copilot « does not define a
  general precedence order » ; les paragraphes copiés utilisateur + projet comptent deux fois,
  **à chaque requête**.

La communauté le dit (dossier D) : « There's no indication that your rules aren't being
loaded » ; « Codex CLI silently truncates any AGENTS.md over 32 KiB without warning ». Les
outils existants sont mono-hôte (agents-doctor : Codex ; claudelint, cclint : Claude ;
cursor-doctor : Cursor) ou **écrivent** dans les fichiers (agnix `--fix`, rulesync). Aucun ne
fait, en lecture seule, sur plusieurs hôtes, la simulation des budgets, le rapport d'ombre et
le coût par requête. C'est une vue, pas une action : le travail de la famille.

## 2. Ce que fait Linstruct

Pour chaque **hôte présent sur la machine** et chaque **projet connu**, Linstruct reconstitue
la **pile d'instructions effective** : les fichiers que l'hôte chargerait au démarrage, dans
son ordre (système → utilisateur → racine → dossier courant, local après principal, imports
dépliés), avec pour chacun octets, caractères, lignes, tokens estimés (≈ octets / 4, annoncé
comme estimation), le cumul, et la **jauge du budget** de l'hôte quand il en a un. De là, les
constats.

Le dossier courant d'un hôte est pris à la racine du projet ; les fichiers de sous-dossiers
« chargés à la demande » sont montrés comme tels, hors cumul.

### Constats (`Kind`) et gravité

| Kind | Ce que c'est | Gravité |
|---|---|---|
| `cut` | Un hôte présent **coupe ou saute** du contenu, et la source le prouve : Codex au-delà de 32 KiB (fichier, octet, ligne ; fichiers sautés derrière), Claude > 4 MiB, Goose import hors budget, Trae 4e niveau | **1 act now** |
| `secret` | Un secret en clair dans un fichier d'instructions (`bbl-hostrules::secrets`, valeur masquée) — il part dans chaque prompt | **1 act now** |
| `over-limit` | Au-delà d'une limite **documentée** dont la conséquence ne l'est pas : Devin 12 000 / 6 000 caractères | **2 review** |
| `ignored` | Un fichier écrit pour un hôte que **cet hôte ne lit pas** : `.md` dans `.cursor/rules`, `AGENTS.local.md`/`.agents/` (Claude), `CLAUDE.md` pour Codex, `agents.md` minuscules (Warp, Kilo), `AGENTS.md` pour Gemini sans `context.fileName`, frontmatter Kiro pas à l'octet 0, `.clinerules` dossier (Zed) | **2 review** |
| `dead-import` | `@chemin` vers un fichier absent, au-delà de la profondeur (4 Claude, 5 Gemini/Qwen, 3 Goose), chemin entre guillemets (Claude), `@` dans un fichier où l'hôte ne déplie pas (`GEMINI.md`, `*.instructions.md` pour Copilot ; tout fichier pour Codex, Cursor, Cline, Roo, Kilo, OpenCode, Zed) ; `#[[file:…]]` Kiro non résolu | **2 review** |
| `frontmatter` | Règle qui ne s'applique **pas comme écrit** : YAML invalide (Claude → partout ; Cursor → rien), champ ignoré (Claude lit `paths` seul), `.mdc` manuel seulement, `paths: []` (Cline), `globs`/`applyTo`/`fileMatchPattern` sans aucun fichier correspondant dans le projet, `fileMatch`/`manual` sur Kiro CLI V1/V2 | **2 review** |
| `shadowed` | Lu par l'hôte en principe, mais **un autre fichier prend sa place** : `AGENTS.md` sous un `CLAUDE.md` (Claude), `AGENTS.md` sous `WARP.md`, `.windsurf/rules` sous `.devin/rules`, racine sous `.junie/AGENTS.md`, 2e–9e nom pour Zed, `AGENTS.md` sous `AGENTS.override.md` (Codex), `.cursorrules` sous `.mdc`, `CLAUDE.md` sous `AGENTS.md` (Kilo, OpenCode, Amp) | **3 good to know** |
| `over-length` | Au-dessus d'une longueur recommandée (Claude 200 lignes, Cursor 500 lignes), du seuil d'avertissement Claude (40 000 caractères, `observed`), ou ≥ 80 % d'un budget dur (Codex) | **3 good to know** |
| `duplicate` | Même contenu (paragraphe ou fichier) chargé deux fois par le même hôte (utilisateur + projet ; `CLAUDE.md` + `AGENTS.md` identiques là où l'hôte lit les deux : Cursor, Copilot, Qwen) — Copilot et Junie dédupliquent l'identique, les autres non | **3 good to know** |
| `comments` | Commentaires HTML : retirés par Claude et les règles Qwen, **envoyés au modèle** ailleurs | **3 good to know** |
| `legacy` | Nom en fin de vie encore lu : `.cursorrules`, `.windsurfrules`, `.roorules`, `.kilocoderules`, `.junie/guidelines.md`, `WARP.md`, `CONTEXT.md` (Kilo) — avec le nom qui le remplace | **3 good to know** |
| `empty` | Fichier vide (sauté par Codex, Gemini, Roo, Zed ; chargé pour rien ailleurs) | **4 information** |
| `unreadable` | Illisible, encodage, lien mort, dossier nommé comme un fichier (`GEMINI.md/`) | **4 information** |

Hors périmètre v0.1 : juger le **sens** des consignes (contradictions sémantiques, qualité
d'écriture) — un linter de forme, pas un relecteur ; corriger quoi que ce soit ; les règles
qui ne sont pas des fichiers (User Rules Cursor, Global Rules Warp, Team Rules).

### Chiffres du résumé (cartes de la tour)

`files` (fichiers d'instructions trouvés), `hosts` (hôtes présents qui les lisent), `heaviest`
(le plus lourd en tokens estimés, par hôte), `findings`.

### Actions (affichées, jamais exécutées)

`open-file` sur le fichier et la ligne ; `hint` (« split below `##` at line 212 », « rename to
`.mdc` », « add `paths:` », « rename `WARP.md` → `AGENTS.md` ») ; `open-url` vers la page de
doc citée par la règle ; `command` pour les vérifications de l'hôte lui-même (`/context`,
`/memory`, `codex` puis `/status`, `copilot` puis `/instructions`, `gemini` puis
`/memory list`, `kiro-cli` puis `/context show`).

## 3. Hôtes (v0.1)

Chaque hôte a sa fiche `hosts/<id>.toml` **sourcée** (doc, code source ou `observed` : URL,
date, citation) — comme Permitt et MCProster. Un hôte est « présent » si son dossier de config
utilisateur existe ou si son binaire est sur le `PATH`.

| Hôte | Ce que Linstruct reconstitue | Budget simulé |
|---|---|---|
| Claude Code | managé → `~/.claude/CLAUDE.md` → `~/.claude/rules/` → ancêtres `CLAUDE.md`/`.claude/CLAUDE.md` + `CLAUDE.local.md` → `.claude/rules/**` ; `AGENTS.md` selon `instructionFiles` ; imports `@` prof. 4 ; `claudeMdExcludes` | 4 MiB / fichier (saut) ; 40 000 car. (avert., `observed`) ; 200 lignes (reco) |
| Codex CLI | `~/.codex/AGENTS.override.md`\|`AGENTS.md` → racine git → racine projet, un fichier par dossier (`override` > `AGENTS.md` > `project_doc_fallback_filenames`) ; `project_doc_max_bytes` lu dans `config.toml` | **32 768 octets total**, coupe + saut |
| Cursor | `.cursor/rules/**/*.mdc` (modes), `AGENTS.md` racine + imbriqués, `CLAUDE.md` racine, `.cursorrules` | 500 lignes (reco) |
| Copilot (CLI/VS Code) | `~/.copilot/…`, `.github/copilot-instructions.md`, `.github/instructions/**/*.instructions.md` (`applyTo`), `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, imports `@` (sauf GEMINI/instructions) ; réglages VS Code `chat.use*` | aucun documenté |
| Gemini CLI | `~/.gemini/GEMINI.md` → ancêtres jusqu'au marqueur ; `context.fileName` ; imports `@` prof. 5 | aucun documenté |
| Qwen Code | `~/.qwen/QWEN.md` → ancêtres (`QWEN.md` + `AGENTS.md`) → `.qwen/QWEN.local.md` ; `.qwen/rules/**` (`paths`) ; imports prof. 5 | aucun documenté |
| Cline | `.clinerules/`, `.cline/rules/` (non récursif, `.md .markdown .txt`), `.clinerules` fichier, `AGENTS.md`, globaux (`~/Documents/Cline/Rules`, `~/.cline/rules`, `~/.agents/AGENTS.md`) ; `paths` | aucun documenté |
| Roo Code | `~/.roo/rules*/` → `.roo/rules*/` (récursif, alpha) sinon `.roorules*`, `.clinerules*` ; `AGENTS.md`/`AGENT.md`, `AGENTS.local.md` | aucun documenté |
| Kilo Code | `~/.config/kilo/AGENTS.md` → `AGENTS.md` ancêtres (`CLAUDE.md` si aucun), `instructions` de `kilo.jsonc`, legacy `.kilo*/rules*`, `.kilocoderules` | aucun documenté |
| OpenCode | `~/.config/opencode/AGENTS.md` → `AGENTS.md` ancêtres ; `instructions` d'`opencode.json` ; `CLAUDE.md` marqué « doc seulement » | aucun documenté |
| Goose | `~/.config/goose/.goosehints`, `AGENTS.md`, `~/.agents/AGENTS.md` → cwd → racine git ; imports `@` prof. 3 | 128 Ko (imports), 1 MiB déplié |
| Zed | `~/.config/zed/AGENTS.md` → **premier** de 9 noms à la racine | aucun documenté |
| Kiro | `~/.kiro/steering/*.md` → `.kiro/steering/*.md` (`inclusion`), `AGENTS.md` ; `#[[file:…]]` | CLI 75 % de la fenêtre (info) |
| Amp | système → `~/.config/amp/AGENTS.md`, `~/.config/AGENTS.md` → `AGENTS.md` ancêtres (`AGENT.md`, `CLAUDE.md` repli) ; `@` + globs ; `AMP_IGNORE_GUIDANCE_FILES` | aucun documenté |
| Aider | `read:` de `.aider.conf.yml` (3 emplacements), dossiers dépliés | aucun |
| Devin Desktop | `global_rules.md` → `.devin/rules/*.md` \| `.windsurf/rules/*.md` (`trigger`), `.windsurfrules`, `AGENTS.md`/`agents.md` racine + sous-dossiers | **12 000 / 6 000 caractères** |
| JetBrains Junie | chemin réglé > `.junie/AGENTS.md` seul > `AGENTS.md` + `.junie/playbook.md` + `.junie/rules/*.md` > legacy ; CLI `~/.junie/AGENTS.md` | aucun documenté |
| JetBrains AI Assistant | `.aiassistant/rules/*.md` (métadonnées IDE) | aucun documenté |
| Warp | `AGENTS.md` racine + courant (majuscules), `WARP.md` prioritaire | aucun documenté |
| Trae | `~/.trae/user_rules` → `.trae/rules/**` (3 niveaux), `AGENTS.md`/`CLAUDE.md`/`CLAUDE.local.md` si activés | profondeur 3 |

Les hôtes absents ne produisent pas de constat, mais l'app les propose en lecture (« what if
Codex read this project? »). Les divergences doc/code (section 4 de la synthèse) sont
marquées dans la fiche et dites dans la note du constat.

## 4. Promesses (celles de la famille)

R1 lecture seule, sans exception ; R2 aucune sortie réseau (les URL d'`instructions` Kilo/
OpenCode sont listées, jamais suivies) ; R3 secrets masqués, jamais écrits ; R4 rien d'écrit
hors `~/.config/linstruct/` ; R5 actions affichées, jamais exécutées ; R6 chaque règle d'hôte
citée (source, date, citation) ; R7 un état, pas une note — pas de score global.

## 5. Architecture

Depuis `blackberrylabs/template` (`rename.sh linstruct "Linstruct"`).

- `bbl-hostrules` **0.4.0** : ajouter `Format::Markdown` (texte brut, pas d'arbre) — à la
  source, pas une copie. Extension de location : `role` (`main`, `local`, `rules`, `legacy`,
  `override`), `loaded` (`launch` | `on-demand`), `imports` (`syntax`, `depth`, `relative_to`),
  `frontmatter_fields`, `case_sensitive`. Extension d'hôte : `budget` (`bytes` | `chars`,
  `scope` total | file, `on_overflow` cut | skip | warn | undocumented), `recommended_lines`,
  `warn_chars`, `order` (texte sourcé), `strips_html_comments`, `shadowing` (listes « X
  remplace Y »), `detect` (dossier ou binaire).
- `crates/core` (`linstruct-core`) : `scan(roots, env)` → `Inventory { files, stacks (hôte ×
  projet), findings, not_read }` ; `report.rs` → `bbl-report/1`.
- `crates/cli` : `linstruct [path…] [--host id] [--json]`, `linstruct report`, `linstruct
  hosts` (fiches et sources).
- `crates/app` : gauche, les hôtes présents et sous chacun la pile de fichiers avec la jauge
  de budget ; droite, le constat choisi (What it means / Where / What you could do) ;
  `--select <finding-id>`. Même visage (bbl-ui), EN par défaut, FR en option.
- Tests : fixtures de projets (`tests/fixtures/projects/*`) sur un faux `HOME`, un cas par
  Kind, dont un AGENTS.md de 40 Ko pour la coupe Codex ; `tests/promises.rs`.

Ids stables : `<kind>/<host>/<chemin relatif au projet>[/<détail>]`.

## 6. Étapes

0. Sources — fait (`sources_A…D`, `synthese_sources.md`).
1. BRIEF validé ; décisions L1–L4 ; dossier `Logiciels/Linstruct`, dépôt
   `blackberrylabs/linstruct`.
2. `bbl-hostrules` 0.4.0 (`Format::Markdown`) ; 19 fiches sourcées ; `scan` + `model` ; tests.
3. CLI + `report` conforme.
4. Maquette de l'app validée, puis l'app.
5. Passage réel sur la machine d'Alexandre.
6. Release 0.1.0 (module d'abord), `modules/linstruct.toml` dans la tour, docs des 9 dépôts,
   profil de l'organisation.

## 7. Décisions prises (2026-10-17)

- **L1 — Le nom : Linstruct** (lint + instruct ; libre sur crates.io, PyPI, npm ; aucun
  projet de ce nom sur le web). Dossier `Logiciels/Linstruct`, dépôt `blackberrylabs/linstruct`,
  binaires `linstruct` et `linstruct-app`.
- **L2 — Coupe silencieuse en gravité 1** (`cut`) ; limite documentée à conséquence inconnue
  (Devin) en 2 (`over-limit`).
- **L3 — Les treize constats dès la 0.1.**
- **L4 — Hôtes absents montrés en lecture « what if »**, repliés, hors résumé et hors rapport
  envoyé à la tour.
