---
name: level-building-mcp
description: Build or change a Jak 3 level in the OpenGOAL Level Editor through its MCP tools (place prefabs and decor, make prefabs, place facades, check for holes, light and weather, save, export). Use whenever the user asks to build, decorate or fix a level with the editor, and before creating prefabs.
---

# Building levels through the MCP server

The editor's MCP server (`jak-level-editor-mcp`, configured in `.mcp.json`) acts on the
editor window the user sees. The full guide, with examples, is `docs/mcp-agent-guide.md`: read it
the first time. Positions are meters, Y is up; every change is undone with `undo`.

## The four rules (always)

1. **No holes.** Pieces touch or overlap by 5 to 10 cm: floors reach the walls, walls reach the
   ground and each other, nothing floats. A gap lets the player see the void: a visual bug in the
   game. After building, run `find_gaps` on what you built (`ids`, or `box` the player can walk) and
   fix every spot of kind `gap`, and every `edge` the player can reach (a wall, a floor, a building
   in front). Run it again until none is left that matters.
2. **Prefabs hold structures, not crumbs.** `create_prefab` leaves out elements under 0.5 m
   (`min_size`): keep that; never save loose bolts, debris, stains or tiny props in a prefab. Check
   its `left_out` answer.
3. **Facades are not buildings.** Most buildings of the game are only a front, open behind. Place a
   facade so that only its front can be seen (against a wall or a cliff, the edge of the level,
   between other buildings, back to back with another facade), never in the middle of a place the
   player can walk around. Look at it from the back and the sides (`set_camera` 1.6 m above the
   ground + `capture_view`): if its back shows, move it, turn it, or hide its back behind other
   pieces touching it.
4. **Look before you report.** `capture_view` from the player's height (1.6 m above the ground)
   around what you built, in the game rendering, and from above. Describe what you see, problems
   included.

## A session

1. `get_status`; `open_level` (or `new_level`); `find_nodes` / `raycast` to know where things are.
2. Choose: `list_prefabs`, `list_catalog` (`group`: objects, effects, interactive), `list_decor`,
   `list_models`; the user's own prefabs are in `library/prefabricated/`.
3. Place: `place` / `place_many` (`on_ground: true` drops onto the surface below). A prefab placed is
   a group: `list_groups`, `select` with `group`.
4. Rules 3 and 1: check the backs of the facades from where the player walks, then `find_gaps`.
5. Light and weather: `set_view` (`hour`, `weather`: clear, few_clouds, haze, smog, overcast,
   overcast_fog, rain, storm, storm_rain, storm_fog), `match_lighting` for pieces of Jak II.
6. Rule 4, then `save_project` (only when the user asks to save) and `export_level` when asked.

## Do not

- Do not save, export or delete without being asked; never touch the user's own prefabs or
  projects unless asked.
- Do not put the game's files (`library/`, exports, `.glb`) in git.
