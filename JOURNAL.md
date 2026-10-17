# JOURNAL — Linstruct
<!-- Project journal (French), newest first. One entry = a date, a title, what was decided, by whom, what was set aside, the proof. -->

---

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
