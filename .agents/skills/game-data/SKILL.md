---
name: game-data
description: Regenerate what the editor knows of the game (data/jak3/game-data.json, particles, the game's prefab recipes, Jak II's creatures) or extract a disc. Use when the decompiled code changed, when the catalog or the game's prefabs need updating, or when the extraction is involved.
---

# The game's data

No asset of the game is in the repository: `data/` holds names and rules read from OpenGOAL's
decompiled code, and the levels are extracted from the user's disc into `library/` (ignored by git).

| File | Made by | From |
|---|---|---|
| `data/jak3/game-data.json` | `python tools/gen_game_data.py <jak-project>/goal_src` | the decompiled code (read only), `tools/object_categories.json` for the catalog's choices |
| `data/jak3/particles.json`, `extractor/data/jak3/sprite-pages.json` | `python tools/gen_particles.py <jak-project>/goal_src` | the `defpartgroup` / `defpart` of the code |
| `data/jak2/creatures.json` | `python tools/gen_jak2_creatures.py` | `extractor/data/jak2/all-types.gc` |
| `data/jak3/prefabs.json`, `data/jak2/prefabs.json` | `python tools/gen_prefab_recipes.py library --game jak3` (and `jak2`) | an extracted library, through a hidden editor; `tools/prefab_review.json` keeps what was reviewed |

Rules:

- Read a checkout of the game's own code (not a level branch: a level replacing one of the game's
  changes that level's data). Never write into jak-project.
- Rebuild after changing `data/` (the editor reads the copy in `build/bin/data/`), then
  `--selftest`.
- The recipes written by hand stay first in `prefabs.json`; the generated ones are marked `auto`.

## Extraction

`File > Extract game assets...` in the editor, or:

```bash
build/bin/ogle-extract <game.iso | disc folder> library [--levels CIA,CIB] [--textures]
build/bin/ogle-extract --detect <game.iso | folder>
```

Jak 3 goes to `library/jak3/` (Jak II to `library/jak2/`, for its structures). Prefabs are in
`library/prefabricated/` (the user's) and `library/prefabricated/game/` (the game's, made level
by level from the recipes). All of `library/` but `prefabricated/.gitkeep` stays out of git.
