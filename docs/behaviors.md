# Behaviors of the decor in Jak 3

What a piece of decor (a wall, a slab, a vent grate, a shape made in the editor, a structure of
Jak II) can do in the game once a level is exported: how it collides, whether it throws Jak up,
whether it sends Jak away, whether it can break or bend. This page is the study of the game's code
these settings come from, and what the editor makes of it.

The paths are those of a [jak-project](https://github.com/open-goal/jak-project) checkout:
`goal_src/jak3/...` is the game's decompiled code, `goalc/build_level/...` OpenGOAL's level
builder, `goalc/build_actor/...` its actor builder. Line numbers are those of the checkout studied
and move with its versions. Distances are in game units in the code: 4096 per meter.

## What the editor offers

Select a piece of decor: the inspector's **In the game** section
(`ui/components/inspector/DecorBehavior.svelte`) sets it, for every selected piece at once, undone
like any change. It is kept in the node's extras (`og_decor`, `crates/core/src/io/decor_behaviors.rs`),
saved with the project and in prefabs, and written by **Level > Export for the game**
(`crates/core/src/io/game_export.rs`). The MCP tool
`set_decor` sets the same.

| Behavior | How it is made | Code generated |
|---|---|---|
| **Collision**: by its shape, none (walked through), invisible (an invisible wall); its material, what it does to Jak, its mode and its flags | the level builder's own glTF extras on the piece | none |
| **Bounce**: Jak landing on its top is thrown up *height* meters, on foot, in a dive and on the jetboard | a `bouncer` of the game without a model, as the board vents of the Industrial Zone are, on top of the piece | a type of the level deriving `bouncer` |
| **Propulsion**: Jak stepping on its top is thrown along an arc to a point *distance* meters away, *height* above, turned *yaw* degrees from the piece's facing, in *time* seconds | the game's directed launch (`'launch-dir`), sent as the Metal Head puffers send it | a small `process-drawable` type of the level |
| Breaking, squashing, dipping, swaying | not offered for the decor: they need a model of its own that the game draws as an actor (see [Breakable](#breakable) and [Deformation](#deformation)) | |

## Collision through the level builder

The builder reads these extras in `goalc/build_level/common/gltf_mesh_extract.cpp`; extras on a
material override those on its node (505-513).

| Extra | Effect |
|---|---|
| `set_invisible` (node or material) | not drawn: skipped by the tfrag pass (106-118) and the tie pass (262-273). **Its collision is still built**: the collision pass (489-568) never checks it |
| `set_collision` | must be 1, or none of the keys below are read (442-446) |
| `ignore` | no collision for that node or primitive (450-453, 515-517) |
| `collide_material`, `collide_event`, `collide_mode` | Jak 1's numbering, checked against each maximum (456-476); a missing key reads as 0 |
| `nolineofsight`, `noedge`, `nocamera`, `noentity` | flags (464-484) |

Two settings of the level's `.jsonc` matter too: `automatic_wall_detection` (on by default, with
`automatic_wall_angle` 30 degrees) makes the steep faces walls after the extras are applied, so it
wins over `collide_mode` (593-606; `goalc/build_level/jak3/build_level.cpp:27-29`);
`double_sided_collide` doubles every face (582-589).

The surfaces are converted to Jak 3's by `jak3_pat`
(`goalc/build_level/collide/common/collide_common.h:668-792`, called from
`goalc/build_level/collide/jak3/collide.cpp:142-155`). Only what Jak 1 has gets through:

- **Materials 0 to 22**: stone, ice, quicksand, waterbottom, tar, sand, wood, grass, pcmetal, snow,
  deepsnow, hotcoals, lava, crwood, gravel, dirt, metal, straw, tube, swamp, stopproj, rotate,
  neutral. Jak 3's crmetal, carpet, grmetal, shmetal, hdwood, squish, mhshroom, forest, mhswamp and
  dmaker cannot be reached.
- **Events 0 to 6**: none, deadly, endlessfall, burn, deadlyup, burnup, melt. Slide, lip, lipramp,
  shock, shockup, hide, rail, slippery, drag, waterfloor, hang, fry and slime cannot.
- **Modes**: ground, wall, obstacle (no halfpipe).
- **Flags**: the four above. nojak, noboard, nopilot, noproj and noendlessfall cannot be set.

So, for a piece of decor:

| Wanted | Extras the editor writes |
|---|---|
| seen, walked through (cables, light shafts, foliage) | `set_collision: 1, ignore: 1` |
| not seen, solid (an invisible wall) | `set_collision: 1, set_invisible: 1` |
| deadly, endless fall, burns, melts | `collide_event` 1, 2, 3, 6 (4 and 5: deadly and burn throwing Jak up) |
| its surface (the sound and dust of the steps; ice slides) | `collide_material`, in the builder's numbering (`builder_material`, `crates/core/src/io/decor_behaviors.rs`) |

How Jak 3 reacts (`react-to-pat!`, `goal_src/jak3/engine/collide/collide-shape.gc:737-894`): deadly
is an `'attack` of 2 damage that shoves Jak 3 m up (777-792), burn the same in the burn mode
(793-808), deadlyup and burnup throw Jak up (`target-attack-up`, 809-822), melt and endlessfall are
an `'attack-invinc` (823-834, 859-873). Ice and gravel give their slippery surfaces, quicksand its
own, tube a surface Jak cannot walk on (741-760).

**There is no bouncing surface in Jak 3**, and the slide and slippery events cannot be reached
through the builder: a bounce has to be an object.

### How much collision the game takes

Before it moves Jak, the game gathers the collision around him in a cache
(`engine/collide/collide-cache-h.gc`: `(tris collide-cache-tri 461 :inline)`), from a box built
around his collision shape (`fill-cache-for-shape`, `engine/collide/collide-shape.gc:1859`): his
root sphere, of 2.2 m radius, 1.2 m above his feet (`root-radius`, `engine/target/target-util.gc:402`),
widened by his speed and 1 m (`fill-cache-integrate-and-collide`, `logic-target.gc:2549`). The
background's triangles past the 460th are dropped (`collide-cache.gc:69`, and the same test in the
code that reads the level's collide hash, `game/mips2c/jak3_functions/collide_hash.cpp`, which
prints `print-exceeded-max-cache-tris` in debug mode): Jak then falls through the floor there.

The game's collision is made for it: `ctyindb`'s has 23,928 triangles, at most 156 in that box on
any of its floors. Its decor has 451,162: made into collision by the builder (and doubled by
`double_sided_collide`), more than 461 are in the box on 18.9 % of the floors (median 188, up to
2,862 near the pipes). So the editor keeps the game's collision where the decor is the game's,
gives the pieces placed or moved a collision from their shape only when they are 4 m across or
more (or when told to), and its export report says the places still over the limit.

## Bounce

Every bouncer of the game derives from `bouncer` (`goal_src/jak3/engine/common-obs/bouncer.gc`,
loaded with the game's common code):

| Types | Model | Collision | Throws Jak |
|---|---|---|---|
| `industrial-board-vent`, `industrialb-board-vent`, `port-board-vent` (`levels/city/industrial/ctyinda-obs.gc:65-94`, `ctyindb-obs.gc:10-39`, `levels/city/port/ctyport-scenes.gc:1662-1691`) | **none**: `init-skel!` does nothing | a sphere, prim-id 8, 2.5 m around a point 1 m up, not solid | on the board only |
| `jump-pad` (abstract, `levels/sewer/jump-pad.gc:10`), `for-jump-pad`, `sew-jump-pad` | its own, a fan joint | a solid mesh (prim-id 2) and a sphere (prim-id 8) | on the board only |
| `tpl-bouncer` (`levels/temple/temple-obs.gc:36-224`), `vol-bouncer` (`levels/volcano/volcano-obs.gc:1145`) | its own | a solid mesh, prim-id 1 | on foot, in a dive, on the board |
| `wascity-awning-a`, `-b` (`levels/wascity/waswide-obs.gc:674-775`) | its own | a solid mesh, prim-id 1 | on foot, in a dive, on the board |

How `bouncer` works:

- `init-from-entity!` (240-250) calls `init-collision!`, then `process-drawable-from-entity!`, then
  `init-skel!`, and reads two lumps: `spring-height` (a float, 45056 = 11 m by default) and
  `behavior-type` (1: the alternate jump's camera, 180-183).
- A subtype overrides both: the base `init-skel!` is `(break!)` (212-216), and the base
  `init-collision!` makes a mesh prim with prim-id 0, which no handler ever matches (218-238).
- What touches it is told by the prim-id's bits (`prims-touching?`,
  `engine/collide/collide-touch.gc:369-396`): `'bonk` with mask 1 (37-48), sent by Jak landing on a
  **solid** prim (vertical speed change, contact normal with y above 0.75:
  `engine/target/target-handler.gc:1723-1785`); `'attack` in a dive with mask 1, on the board with
  mask 9 (90-118); `'touch` on a solid prim with bit 1 squashes it (`smush`), bit 4 forbids the
  double jump (49-89).
- It throws Jak with `(send-event proc 'jump spring-height spring-height mods)`: `target-jump` on
  foot (`target-handler.gc:1786-1793`), only in `target-board-stance` on the board
  (`engine/target/board/board-states.gc:577-585`).
- It works without a model: every state checks `(nonzero? (-> self draw))` (121, 150, 165, 186).

The board vents are exactly a bouncer without a model. Jak touches prims that are not solid too:
his post-update calls `find-overlapping-shapes` every frame (`engine/target/logic-target.gc:2699-2704`),
and the `'touched` it finds is forwarded as `'touch` (`target-handler.gc:1005, 1041`); his
collide-with includes `crate`. The vent types themselves live in the DGOs of the Industrial Zone
and the port, not loaded with a custom level: the editor derives `bouncer` itself.

**What the editor generates** (`bounce_code`, `crates/core/src/io/decor_behaviors.rs`): for each piece set to
bounce, a type `<level>-<piece>-bounce` deriving `bouncer`, whose `init-skel!` does nothing and
whose `init-collision!` makes one sphere (prim-id 9: dive and board; collide-as `crate`,
collide-with `jak player-list`, overlap only) of the piece's half width (1 to 20 m), 0.5 m above
its top. Its `idle` state adds what the board vents lack, a bounce on foot: on `'touch`, when Jak
is above it (less than 0.3 m below its top) and not going up, it sends `'jump` with the
`spring-height` of its lump (the height set, in game units) and goes to `fire`; the rest goes to
the bouncer's own handler (the dive and the board). The actor is written in the level's `.jsonc` at
the middle of the piece's top; the piece keeps the collision the builder makes from its shape.

## Propulsion

What the game has:

- **Board vents**: they draw nothing and spawn no particle. They throw Jak only on `'attack
  'board`, which the board sends on any touch (`engine/target/board/target-board.gc:718-781`). The
  steam comes from separate particle groups (`group-ctyinda-red-floor-vent`,
  `levels/city/industrial/ctyinda-part.gc:1020`); the grate is decor.
- **`launcher`** (`engine/common-obs/generic-obs.gc:2066-2671`): no model, a sphere of 3 m; active
  only while Jak has a blue eco power-up (2521, 2579), and it sends Jak 1's `'launch` parameters
  (2550, 2594) where Jak 3 reads `'launch` as height, height, surface and velocity
  (`target-handler.gc:1202-1210`). A leftover that cannot be used. Lumps: `spring-height` (40 m by
  default), `mode`, `alt-vector`.
- **`glider-thermal`** (`levels/glider/glider-ring.gc:244, 687-721`): particles, spawned by
  the glider task's manager, lifting the glider only (1213-1227).
- **`warp-gate`, `air-train`** (`engine/common-obs/warp-gate.gc:457-770, 1056-1175`): teleporters
  with their own models. **`precur-jump-plate`** (`levels/precursor/precura-obs.gc:208-350`): a
  turning platform with its own model.
- **The directed launch**, `'launch-dir`: a displacement to the landing point, a flight time (300 =
  1 second) and a gravity (for example -327680). It puts Jak in `target-launch-dir`
  (`target-handler.gc:1211-1217`, `engine/target/target-launch.gc:10`), and is ignored on the
  board. `mhcity-puffer` sends it from a purely geometric test
  (`levels/mhcity/mhcity-obs2.gc:160-206`).

**What the editor generates** (`launch_code`): for each piece set to propel, a type
`<level>-<piece>-launch` deriving `process-drawable`, with no model and no collision, holding the
landing point (in the world: the middle of the piece's top, plus *distance* along its facing turned
by *yaw*, plus *height*). Each frame, when Jak is within the piece's half width of its middle
(horizontally) and within 1.5 m of its top's height, it sends `'launch-dir` with the way from Jak
to the landing point, the time set (in 300ths of a second) and the gravity -327680 (as its bits,
-929038336, as the puffers send it). The board ignores it, as in the game.

## Breakable

| Types | Break on | Then |
|---|---|---|
| `fac-break-fence-a`, `-b`, `-c` (`levels/factory/factoryc-obs2.gc:185-600`) | `'impact-impulse` above 16384 (vehicles) | a `joint-exploder` on a second skeleton (`-explode`), particles |
| `krimson-wall-break` (`levels/city/common/krimson-wall.gc:98-260`) | `'break` (scripted) | a streamed animation, explosions |
| `tpl-break-alcove` (`temple-obs.gc:2009-2172`) | `'attack` with Dark Jak's dark-smack (2069) | its animation, particles, its `on-exit` script |
| `tpl-stone-break` (`levels/temple/templex-obs.gc:96`) | `'ridden` (116) | falls; 20 mesh prims |
| `vol-break-ground` (`levels/volcano/volcano-obs2.gc:246`) | ridden for 0.1 s (295) | hides a tie of the background with `prototypes-game-visible-set!` (357-366, `engine/draw/drawable.gc:377-395`) and plays its own model |
| `cav-break-bridge` (`levels/mine/prebot-extras.gc:10-206`) | `'attack` | falls, `joint-exploder` |
| `for-break-bridge-board-a` to `-d` (`levels/forest/forest-bridges.gc:90-170`) | `'trigger`, or `'attack` by an uppercut, a dive or the board | `joint-exploder`, dust; completes a subtask |
| `wascity-cactus` (`waswide-obs.gc:272-560`) | `'attack`: light hits shake its joints and take 0.1 of its 1.0 health; knockbacks or Dark Jak break it | `joint-exploder`, `drop-pickup` |
| `des-cactus-a`, `-b` (`levels/desert/des-cactus.gc:10-300`) | damage of 2 or more against its health; `'impact-impulse` above 204800 | `joint-exploder` |

The tools of the common code: `joint-exploder` (`engine/anim/joint-exploder.gc:37-51, 730-769`: a
skeleton group, an animation, a list of joints), `debris-spawn` (`engine/common-obs/debris.gc:478`:
needs its parent's joints, and its art groups live in level DGOs), `group-crate-explode` and
`group-crate-steel-explode` (`engine/common-obs/crates.gc:245, 258`: particles only).

**The decor the builder makes cannot break**: it is merged into one tfrag and tie with no
visibility per prototype (`goalc/build_level/common/Tie.cpp:37-38`), and its collision is the
level's static collide hash. Nothing of it can be hidden while the game runs.

The way, for a later version: export the piece as an actor of its own with `build_actor`
(`goalc/build_actor/jak3/build_actor.cpp:578-654`). Without a skin it gets the joints align,
prejoint and main and a default `<name>-idle` animation; with a skin, real joints and the glTF's
animations. `:gen-mesh #t` (`-m`) makes its collision meshes from the same extras
(`goalc/build_actor/common/MercExtract.cpp:381-478`), at most 255 vertices per mesh. It is listed in the level's
`custom_models` (`goalc/build_level/jak3/build_level.cpp:234-239`), its `-ag.go` goes into the DGO,
and `def-actor` declares it (`engine/data/art-h.gc:489-514`); `levels/test-zone/test-zone-obs.gc`
is the working example (a mesh prim and `initialize-skeleton *test-actor-sg*`). The level's code
can then make it breakable: a solid mesh prim; on `'attack` (once per attack id, as
`wascity-cactus` does), take health; at the end, hide it, clear its collide-as and collide-with,
`part-tracker-spawn group-crate-explode` and `process-entity-status! ... dead`. Breaking into pieces
needs a skinned model, one bone per piece, for `joint-exploder`.

## Deformation

Everything that bends needs a skeleton of its own:

- **The awnings and the temple and volcano bouncers**: their own animation; `smush` seeks the
  `idle` animation between frames 6 and 2 by Jak's horizontal distance (`bouncer.gc:146-171`),
  `fire` plays from frame 6 to the end (186-194).
- **`base-plat`, `plat`** (`engine/common-obs/base-plat.gc:46-110`): a `'bonk` dips the whole
  platform 0.2 m, a rigid movement that needs a mesh prim and an art group.
- **`rigid-body-platform`** (`engine/common-obs/rigid-body-plat.gc:44-389`, in the VOCA DGO, not the
  common code): a body floating at the ocean's or a `water-actor`'s height, tilted by Jak's weight
  (`'ridden`) and `'bonk`s; it sinks without water.
- **`vol-balance-plat`** (`volcano-obs.gc:571-800`): a body hanging from a chain of joints (4 to
  12), with two collision meshes.
- **`tpl-banner`** (`levels/temple/temple-obs2.gc:1930-1975`), **`wascity-flag-a`** to **`-d`**
  (`waswide-obs.gc:73-170`): a looping animation, no collision, no reaction to Jak.
- Joints turned by hits: the cactus of Spargus (290-317), the sway of `des-plant`
  (`des-cactus.gc:37-95`).

For a piece of decor: a squash needs an actor of its own with a skin and an `idle` animation; a dip
or a tilt an actor of its own with a collision mesh (no skin); a sway a skin and a looping
animation. The route is the one of [Breakable](#breakable).

## Summary

| Behavior | The game's types | A model of its own? | For a piece of decor | Parameters |
|---|---|---|---|---|
| collision | the builder's extras | no | **offered**: extras on the piece | material 0-22, event 0-6, mode 0-2, 4 flags |
| deadly, endless fall, burn, melt | `collide_event` 1, 2, 3, 6 | no | **offered** | |
| bounce on the board | the board vents | no | **offered**: a `bouncer` subtype, a sphere (prim-id 9) | `spring-height` (11 m by default), `behavior-type` |
| bounce on foot | `tpl-bouncer`, `vol-bouncer`, the awnings | yes, for the game's types | **offered**: the same subtype, which jumps Jak on `'touch` from above | `spring-height` |
| directed launch | `'launch-dir` (`mhcity-puffer`) | no | **offered**: a trigger without a model | the way (game units), the time (300 = 1 s), the gravity |
| breakable | fences, bridges, cacti | yes | an actor of its own (`build_actor`) and generated code | health, particles; pieces need a skin |
| squash | the awnings | yes | an actor of its own, skinned, with an `idle` animation | its frames 2 to 6 |
| dip, tilt | `plat`, `rigid-body-platform`, `vol-balance-plat` | yes (a collision mesh) | an actor of its own | a dip of 819.2 (0.2 m); `player-weight` 6.6 m |
| sway | `tpl-banner`, `wascity-flag` | yes | an actor of its own, skinned, with a looping animation | |

## What was checked

- The engine's tests (`the_decors_behaviors`, `crates/core/src/io/game_export.rs`) export a
  level with a slab of deadly ice, a cable Jak goes through, a pad that bounces and a ramp that
  launches, and check the extras, the actors (their place, their `spring-height`) and the code the
  export writes.
- The study above was made by reading jak-project's code (read only; nothing of it was changed).
- **Not checked yet**: the generated bounce and propulsion code compiled by OpenGOAL's compiler and
  played in the game. The behaviors of the game's own objects (doors, board vents, elevators) were
  compiled against Jak 3's code before; these two new types follow the same path but have not been.
