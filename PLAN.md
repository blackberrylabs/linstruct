# PLAN — Linstruct
<!-- Project plan (French). -->

## Étape 0. Cadrage — fait le 2026-10-17
- [x] Quatre dossiers de sources (19 hôtes, chaque fait cité) + synthèse — `00_cadrage/`
- [x] Nom vérifié libre (crates.io, PyPI, npm, web) et validé : **Linstruct**
- [x] BRIEF rev. 1 validé ; décisions L1–L4
- [x] bbl-hostrules 0.4.0 (`Format::Markdown`) — à la source

## Étape 1. Cœur et CLI — en cours
- [x] `model.rs` : 13 Kinds sur les quatre gravités ; `Slot`, `Stack`, `InstrFile`, `Finding`
- [x] `rules.rs` : extensions des fiches (`kind`, `loaded`, `nested`, `imports`, `frontmatter`,
      `config`, `only_if_absent`, budget, warn, recommended, detect)
- [x] 19 fiches `hosts/*.toml` sourcées + `_not-read.toml`
- [x] `text.rs` : front matter (YAML tolérant pour `globs: *.ts`), code blanchi, références
      `@` / `#[[file:`, commentaires HTML, paragraphes, secrets par ligne (masqués)
- [x] `scan.rs` : présence, piles par hôte × projet, budget (total / fichier, coupe / saut),
      chaînes « when-cwd » (Codex), imports dépliés, ombres, config des hôtes
      (`project_doc_max_bytes`, `instructions`, `read:`, `context.fileName`)
- [x] `report.rs` : `bbl-report/1`, hôtes présents seulement (L4)
- [x] CLI : `linstruct`, `stacks`, `hosts`, `report`, `--json`, `--home`, `--host`, `--all-hosts`, `-p`
- [x] Tests : 6 text, 3 promesses, 6 scan sur un faux HOME (15 verts) ; clippy et fmt propres
- [x] Commit 1 `3234f50`, dépôt `blackberrylabs/linstruct` en ligne
- [x] Passage réel sur la machine d'Alexandre : 4 hôtes, 29 projets, 19 constats justes
- [x] Commit 2 : sujets et identifiants qualifiés par le projet, ombre évaluée au dossier
      courant, `-p` à plusieurs dossiers
- [ ] L5 — d'où viennent les projets par défaut (réglage mémorisé ou découverte git)

## Étape 2. Maquette validée, puis app
- [x] Maquette A1 (trois colonnes : piles par hôte avec jauge, constats, détail sourcé)
- [ ] Validation d'Alexandre
- [ ] App iced sur bbl-ui ; `--select <finding-id>` ; « what if » repliés ; EN/FR

## Étape 3. Paquets, dépôt, release 0.1.0 — sur accord
- [ ] `packaging/build.sh` (.deb, .rpm, AppImage), icône, .desktop
- [ ] Dépôt `blackberrylabs/linstruct`, push, release (module d'abord)
- [ ] Tour : `modules/linstruct.toml`, fixture `tests/fixtures/reports/linstruct.json`, release
- [ ] Docs des 9 dépôts, profil de l'organisation, `00_moi/PROJETS.md`

## Après 0.1.0
- Hôtes `not_read` : Antigravity CLI, Factory, Augment (fiches quand les sources existent)
- Contradictions sémantiques entre consignes (hors périmètre v0.1, décision BRIEF)
- Devin : comportement au-delà de 12 000 caractères à observer (`observed`)
