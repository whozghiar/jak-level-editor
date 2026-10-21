---
name: interface-texts
description: Add or change a text of the editor's interface (menus, buttons, tooltips, log messages, guide, MCP answers). Use whenever Svelte or Rust code shows text to the user, or a label must change.
---

# Interface texts and translations

The texts are in `lang/en.json` (English, the reference) and `lang/fr.json` (French); a language
is a file of `lang/` mapping keys to texts. Code never holds a text the user reads:

- **The interface** (`ui/`): `t("key")` from `ui/lib/i18n.ts`, or `t("key", arg1, arg2)` with
  printf arguments. Inside a template or an effect it follows the language when it changes.
- **The engine** (`crates/`): the log, the errors the interface shows and the MCP answers go
  through `jle_core::i18n`: `tr("key")`, `log_info!(tr("key"), args...)`, `strf!(tr("key"), ...)`.

## Adding a key

1. Use it in the code: `<button>{t("group.make")}</button>`.
2. Add it to **every** file of `lang/`, with the same `%s`, `%zu`, `%d`, `%.1f` in the same order
   (a count is `%zu`). Edit the files as text, near related keys or at the end: do not rewrite the
   whole file with a JSON library (it reorders and loses the blank lines that group the keys).
3. `cargo test -p jak-level-editor-core i18n` fails on a key missing from a language, a key
   English lacks, or different arguments. Build again to see it in the program (the interface
   embeds `lang/`; the engine reads the copy next to the program).

## Conventions

- Names of keys: `<area>.<thing>[.tip]` (`menu.edit.group`, `game.clouds_tip`, `catalog.effects`).
- Tooltips say what it does and give a concrete example; the guide's entries use
  `guide.<id>.title/.text/.example/.keys`, shown by `<Tip guide="<id>" />`
  (`ui/components/common/Tip.svelte`).
- Plain words: the editor is for people who never used a level editor. Say what happens in the
  game, not how the code does it.
- The user writes in French: French texts must be real French (accents, « », espaces avant : ; ? !).
- Documentation (README, docs/) is in English.
