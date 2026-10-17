# JOURNAL — Linstruct
<!-- Project journal (French), newest first. One entry = a date, a title, what was decided, by whom, what was set aside, the proof. -->

---

## 2026-10-17 — premier passage réel, commit 1 en ligne

- Dépôt `blackberrylabs/linstruct` créé par Alexandre (`gh repo create`), commit 1 `3234f50`
  poussé. bbl-hostrules 0.4.0 taguée et en ligne.
- Passage réel sur la machine d'Alexandre : 4 hôtes présents (Claude Code, Cursor, Copilot via
  l'extension VS Code, OpenCode), 49 fichiers dans 29 projets avec `-p ~/Bureau/Logiciels/*
  ~/Bureau/Projets/*`, 19 constats — tous justes à la lecture : deux CLAUDE.md au-dessus de
  200 lignes, un à 544 lignes (Cursor recommande 500), un commentaire HTML envoyé au modèle
  par trois hôtes, 17 paragraphes répétés entre deux AGENTS.md d'un monorepo, un AGENTS.md
  identique à `~/.config/opencode/AGENTS.md`.
- Trois défauts vus et corrigés : (1) les identifiants et sujets ne portaient que le chemin
  dans le projet → deux projets avec un `CLAUDE.md` se dédoublonnaient l'un l'autre ; le sujet
  est maintenant `Projet/chemin`, l'identifiant porte le chemin du projet ; (2) l'ombre de
  Claude Code (« AGENTS.md lu seulement sans CLAUDE.md dans le dossier courant ou au-dessus »)
  était évaluée au dossier du fichier imbriqué au lieu du dossier courant — un monorepo avec
  un `CLAUDE.md` dans un sous-dossier mettait à tort tous ses AGENTS.md dans l'ombre ; (3) `-p`
  n'acceptait qu'un dossier par occurrence → `num_args = 1..`, le joker du shell marche.
- Appris : par défaut Linstruct ne connaît que les projets de `~/.claude.json` (2 chez
  Alexandre sur une trentaine). Décision L5 à prendre : dossiers de projets mémorisés dans
  `~/.config/linstruct/settings.json`, ou découverte des dépôts git sous un dossier donné.

## 2026-10-17 — cœur, CLI, fiches, maquette

- Cœur écrit depuis le gabarit : 13 constats, 19 fiches sourcées, scan sur un faux HOME.
  Preuve : 15 tests verts, `linstruct --home <fixture> -p <proj>` sort les treize sortes.
- Choix techniques : `kind` (pas `role`) pour la nature d'une location — `role` est un champ
  commun de bbl-hostrules et l'aplatissement serde le consommait ; front matter YAML lu avec
  tolérance pour `globs: *.ts` (écriture courante des règles Cursor, acceptée par Cursor) ;
  totaux des piles = fichiers chargés au lancement seulement, les « à la demande » comptés
  dans les fichiers de l'hôte mais hors cumul ; doublons cherchés entre niveau utilisateur et
  projet d'un même hôte ; un secret, un fichier vide ou illisible signalé une fois (premier
  hôte qui le charge), pas une fois par hôte.
- Écarté : signaler `.cursor/rules` comme « ignoré » par Codex — un fichier écrit pour un autre
  hôte n'est pas une attente trahie ; `ignored` ne vise que les fichiers qu'un utilisateur
  pouvait croire lus (`AGENTS.local.md` pour Claude, `CLAUDE.md` pour Codex, `.md` dans
  `.cursor/rules`, `agents.md` minuscules pour Warp).
- bbl-hostrules 0.4.0 (`Format::Markdown`) prêt sur le PC d'Alexandre ; commit, tag et push
  à lui.
- Maquette A1 envoyée (trois colonnes). En attente de validation.

## 2026-10-17 — naissance

- Idée parquée dans `00_moi/IDEES.md` (linter des CLAUDE.md / AGENTS.md / .cursor/rules),
  confirmée par les douleurs de la communauté : limites, troncature silencieuse de Codex.
  Alexandre : « On y va, go! ».
- Étape 0 : quatre recherches en parallèle (A Claude Code / Codex / Cursor / Copilot ; B Gemini
  / Qwen / Cline / Roo / Kilo ; C OpenCode / Goose / Zed / Kiro / Amp ; D Aider / Devin /
  JetBrains / Warp / Trae + convention AGENTS.md + communauté + outils existants), chaque fait
  cité (URL, date, citation ou fichier:ligne). Synthèse dans `00_cadrage/`.
- Décisions : **L1** nom Linstruct (« Tu peux l'appeler linstruct ») ; **L2** coupe
  silencieuse en gravité 1 (« 1 ») ; **L3** les treize constats dès la 0.1 (« 13. ») ;
  **L4** hôtes absents en « what if » (« What if? »). BRIEF rev. 1 validé (« Valide, go. »).
- Écarté : juger le sens des consignes (contradictions sémantiques) ; corriger quoi que ce
  soit ; les règles qui ne sont pas des fichiers (User Rules Cursor, Global Rules Warp).
