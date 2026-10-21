---
name: interface-texts
description: Add or change a text of the editor's interface (menus, buttons, tooltips, log messages, guide). Use whenever C++ code shows text to the user, or a label must change.
---

# Interface texts and translations

Every text the interface shows goes through `tr("key")` (or `tr_id("key")` for a widget, which adds
an ImGui id, or `strf(tr("key"), ...)` with arguments). The texts are in `lang/en.json` (English,
the reference) and `lang/fr.json` (French); a language is a file of `lang/` mapping keys to texts.

## Adding a key

1. Use it in the code: `ImGui::Button(tr_id("group.make").c_str())`.
2. Add it to **every** file of `lang/`, with the same `%s`, `%zu`, `%d`, `%.1f` in the same order
   (`size_t` is `%zu`). Edit the files as text, near related keys or at the end: do not rewrite the
   whole file with a JSON library (it reorders and loses the blank lines that group the keys).
3. Rebuild (the editor reads the copy in `build/bin/lang/`) and run `--selftest`: it fails on a key
   missing from a language or on different arguments.

## Conventions

- Names of keys: `<area>.<thing>[.tip]` (`menu.edit.group`, `game.clouds_tip`, `catalog.effects`).
- Tooltips say what it does and give a concrete example; the guide's entries use
  `guide.<id>.title/.text/.example/.keys` (`guide_tip("<id>")`).
- Plain words: the editor is for people who never used a level editor. Say what happens in the
  game, not how the code does it.
- The user writes in French: French texts must be real French (accents, « », espaces avant : ; ? !).
- Documentation (README, docs/) is in English.
