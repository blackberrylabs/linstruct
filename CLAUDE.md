# CLAUDE.md — Linstruct
<!-- Working rules for Claude in this repository (French). Code and docs are in English. -->

Linstruct : vos fichiers d'instructions IA (CLAUDE.md, AGENTS.md, .cursor/rules…) tels que
chaque hote les lira reellement — et ce qu'il laissera tomber sans rien dire. Module n° 5 de
la famille Blackberrylabs ; fiche dans `Projets/00_moi/PROJETS.md`.

## Regles de travail

- BRIEF valide avant le code ; questions a Alexandre une par une, decisions une par une
  avec une recommandation ; PLAN et JOURNAL tenus a chaque etape ; `00_cadrage/` hors git.
- Lecture seule sans exception, aucune sortie reseau, secrets masques et jamais ecrits,
  rien d'ecrit hors `~/.config/linstruct/`. Un etat, pas une note. Actions affichees,
  jamais executees. Chaque regle cite sa source (doc, code source ou `observed`), sa date,
  sa citation : rien de memoire.
- Commits locaux libres ; `git push`, suppression de fichiers, tags, timers : accord
  explicite. Commandes donnees completes, chemin absolu
  (`cd /home/nushoko/Bureau/Logiciels/Linstruct && …`). Avant tout git lance par Alexandre :
  `find .git -name '*.lock' -delete` (le pont laisse des verrous).
- Anglais : code, commentaires, commits, README, interface par defaut. Francais : BRIEF,
  PLAN, JOURNAL, CLAUDE.md, option de langue.
- Seul nom cite : Alexandre Do.
- Un passage reel sur la machine d'Alexandre avant chaque release ; chaque release
  verifiee depuis l'exterieur (paquet telecharge, somme, version, `report` conforme).

## Identite git

Avant le **premier commit** : `git config user.name "Alexandre Do"` et
`git config user.email "277202463+Nimeria2026@users.noreply.github.com"` en config locale ;
verifier par `git log --format='%an <%ae>'` avant tout push.

## Fondations communes — ne pas recopier, completer a la source

- `bbl-hostrules` : fichiers des hotes IA, lecteurs, masquage (si le module lit des hotes).
- `bbl-ui` : palette, briques iced, reglages, i18n. Meme visage que les autres outils.
- `bbl-report` : le contrat de la tour. `linstruct report` produit un `bbl-report/1` ;
  chaque `Kind` a sa place sur les quatre gravites (`model.rs`), ecrite dans le CHANGELOG.

## Reperes

- `crates/core/src/{model,rules,text,scan,report}.rs` ; `hosts/*.toml` (19 fiches sourcees,
  schema `docs/hosts.md`) ; `tests/{promises,scan}.rs` (rien d'ecrit, rien de reseau, rapport
  conforme, un faux HOME avec un cas par constat).
- `crates/cli` (`linstruct`, `stacks`, `hosts`, `report`), `crates/app` (`linstruct-app
  --select <finding-id>`).
- Les treize constats et leur gravite : `model.rs` et CHANGELOG. Un constat de fichier
  (secret, vide, illisible) une seule fois ; un constat d'hote par hote.
- Hotes absents : calcules, montres en « what if » dans l'app, jamais dans le resume ni le
  rapport (L4).
- `docs/new-module.md` de la tour : la marche a suivre complete.
