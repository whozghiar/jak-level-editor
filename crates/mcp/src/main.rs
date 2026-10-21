//! jak-level-editor-mcp [--remote-port <port>]: an MCP server (Model Context Protocol: JSON-RPC
//! 2.0, one message per line on stdin and stdout) that gives an assistant such as Claude the tools
//! of the editor: open a level or start a new one, find what it holds, place objects of the
//! catalog, decor parts and prefabs, move, turn, copy and delete elements, look through the
//! camera, save the project (src/app/mcp.cpp).
//!
//! It forwards each tool call to the editor running on this computer (its remote control,
//! crates/core/src/editor/remote.rs), and starts the editor when none is running: everything
//! happens in the window the user sees, and can be undone there. Nothing but the protocol goes to
//! stdout.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Map, Value, json};

/// the editor's remote control (jle_core::editor::remote::K_REMOTE_PORT)
const K_REMOTE_PORT: u16 = 47821;

fn vec3(description: &str) -> Value {
  json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3,
         "description": description})
}

fn vec2(description: &str) -> Value {
  json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2,
         "description": description})
}

fn ids(description: &str) -> Value {
  json!({"type": "array", "items": {"type": "integer"}, "description": description})
}

fn typed(ty: &str, description: &str) -> Value {
  json!({"type": ty, "description": description})
}

fn str(description: &str) -> Value {
  typed("string", description)
}

fn num(description: &str) -> Value {
  typed("number", description)
}

fn integer(description: &str) -> Value {
  typed("integer", description)
}

fn boolean(description: &str) -> Value {
  typed("boolean", description)
}

/// any JSON, described
fn any(description: &str) -> Value {
  json!({ "description": description })
}

fn tool(name: &str, description: &str, properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
  let properties: Map<String, Value> = properties
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
  let mut schema = json!({"type": "object", "properties": properties});
  if !required.is_empty() {
    schema["required"] = json!(required);
  }
  json!({"name": name, "description": description, "inputSchema": schema})
}

fn tools() -> Value {
  let discard = || boolean("true to drop the unsaved changes of the document");
  json!([
    tool(
      "get_status",
      "What the editor shows now: the library and its number of levels, the document (level or \
       project, number of elements, bounds, unsaved changes), the story view, the camera and the \
       selection. Call it first.",
      vec![],
      &[],
    ),
    tool(
      "list_levels",
      "The levels of the game that can be opened or added, with their DGO file and whether the \
       document has them. With near: only the levels the game displays together with that one \
       (its neighbors).",
      vec![
        (
          "filter",
          str("part of a level name or DGO (e.g. 'des' for the desert)")
        ),
        ("near", str("a level: its neighbors in the game only")),
        (
          "game",
          str("'jak2': Jak II's levels (once extracted), to open or add as decor")
        ),
      ],
      &[],
    ),
    tool(
      "get_level_info",
      "What is known of a level: its neighbors in the game (the levels displayed with it), its \
       companions, its continue points (where the game starts the player: position in meters, \
       levels shown there), and when the document has it, its role (opened, added, companion), \
       its group and its bounds.",
      vec![("name", str("a level, the one opened by default"))],
      &[],
    ),
    tool(
      "add_level",
      "Adds a level of the library to the document, at its place in the game's world: levels \
       side by side in the game meet in the editor too (to edit the junction between two levels, \
       or to build a level next to one of the game's). Its companions and mission layers come \
       with it, under its group. Waits until it is added; returns its group and bounds. Undoable.",
      vec![
        ("name", str("a level from list_levels")),
        (
          "actors",
          boolean("false: its decor alone, without any actor (true by default: with its objects)"),
        ),
        (
          "reference",
          boolean(
            "true: shown to compare with (washed in blue), never selected nor exported: the \
             game's level the document replaces",
          ),
        ),
        (
          "game",
          str(
            "'jak2': a level of Jak II (list_levels with game 'jak2'): its decor alone, its \
             textures renamed jak2-..., named jak2-<name> in the document",
          ),
        ),
      ],
      &["name"],
    ),
    tool(
      "remove_level",
      "Removes a level added to the document, and what is under its group. Undoable.",
      vec![
        ("name", str("a level added with add_level")),
        ("reference", boolean("the one added as a reference")),
      ],
      &["name"],
    ),
    tool(
      "match_lighting",
      "Scales the vertex colors (the light) of pieces and of what is under them: to the \
       brightness of the document's Jak 3 decor by default (the inspector's Match the lighting, \
       for structures of Jak II lit darker or brighter than the level), or by `factor`. Undoable.",
      vec![
        ("ids", ids("the elements")),
        (
          "factor",
          num("multiplies their colors instead (1.5: half as bright again)")
        ),
      ],
      &["ids"],
    ),
    tool(
      "merge_level",
      "Makes a level added to the document (add_level, a level of Jak 3 or of Jak II) part of the \
       document's own level: no longer a level next to it, its pieces and its collision go with \
       the document's in export_level (also in place of a level of the game: replace_level). \
       Undoable.",
      vec![(
        "name",
        str("the level added (as get_status lists it: jak2-<name> for Jak II's)"),
      )],
      &["name"],
    ),
    tool(
      "set_decor",
      "What the game does with elements of the decor (pieces, models, shapes, Jak II \
       structures), written by export_level: collision {kind: shape|none|invisible, material: \
       stone|ice|quicksand|waterbottom|tar|sand|wood|grass|pcmetal|snow|deepsnow|hotcoals|lava|\
       crwood|gravel|dirt|metal|straw|tube|swamp|stopproj|rotate|neutral, event: none|deadly|\
       endlessfall|burn|deadlyup|burnup|melt, mode: auto|ground|wall|obstacle, noedge, nocamera, \
       noentity, nolineofsight}; bounce {height: meters}: Jak landing on its top is thrown up (the \
       game's bouncer without a model); launch {distance, height (meters), yaw (degrees from the \
       element's facing), time (seconds)}: Jak stepping on it is thrown along an arc. false for a \
       part: back to the game's. Undoable.",
      vec![
        ("ids", ids("elements of the decor")),
        ("collision", any("an object as above, or false")),
        ("bounce", any("{height} or false")),
        ("launch", any("{distance, height, yaw, time} or false")),
      ],
      &["ids"],
    ),
    tool(
      "replace_level",
      "Makes the document a replacement of a level of the game (the game then loads it in that \
       level's place, with that level's continue points, music and missions; export_level writes \
       it so). level: the game's level ('' for none); show_original: adds that level as a \
       reference (blue, not exported) and waits; align: moves the document's own level so that \
       its middle and bottom are where the original's are; turn_deg: turns it around its middle. \
       Returns where the game starts the player there: the ground under each continue point of \
       the level replaced (null: none, the player would fall). Undoable.",
      vec![
        ("level", str("a level of the game, e.g. 'ctyindb'")),
        ("show_original", boolean("add the original as a reference")),
        (
          "align",
          boolean("place the document's level where the original is")
        ),
        (
          "turn_deg",
          num("turn the document's level around its middle (degrees)")
        ),
      ],
      &[],
    ),
    tool(
      "open_level",
      "Opens a level of the game (its decor, its objects and water, without its creatures) and \
       waits until it is loaded. Refuses when the document has unsaved changes, unless \
       discard_changes is true.",
      vec![
        ("name", str("a level name from list_levels")),
        (
          "actors",
          boolean("false: its decor alone, without any actor (true by default: with its objects)"),
        ),
        (
          "game",
          str(
            "'jak2': a level of Jak II (list_levels with game 'jak2'): its decor alone made Jak \
             3's, the document's level named jak2-<name>",
          ),
        ),
        ("discard_changes", discard()),
      ],
      &["name"],
    ),
    tool(
      "new_level",
      "Starts an empty level, to build one from the catalog, the decor of the game's levels and \
       prefabs (nothing to stand on at first: place decor pieces or a prefab).",
      vec![("discard_changes", discard())],
      &[],
    ),
    tool(
      "list_catalog",
      "The objects that can be placed, by family, with how many the game's levels have: the \
       objects that make up a place (group 'objects': props that only stand there, doors and \
       airlocks, water, dark eco and lava), the visual effects (group 'effects': the particle \
       effects of the levels, each with its effect_kind: smoke, fire, light, sparks, water, \
       other) and the interactive ones (group 'interactive': crates, platforms, elevators, \
       bouncers, buttons, breakables, hazards). Never the living (enemies, characters, animals), \
       the collectibles nor the technical types.",
      vec![
        (
          "group",
          str("'objects', 'effects' or 'interactive' (all by default)")
        ),
        (
          "family",
          str(
            "objects: prop, door, water, effect; interactive: crate, platform, movement, button, \
             hazard, breakable, light, weapon, door-like, other",
          ),
        ),
        ("filter", str("part of a name")),
        ("offset", integer("first result (paging)")),
        ("limit", integer("number of results, 100 by default")),
      ],
      &[],
    ),
    tool(
      "list_models",
      "The other models of the game (parts of objects, debris, props: everything but the \
       characters), placed as decor with place (kind 'model'). With game 'jak2': Jak II's \
       objects by their model (place with game 'jak2': converted for Jak 3, without their \
       behavior).",
      vec![
        (
          "game",
          str("'jak2': Jak II's (when Jak II was extracted too)")
        ),
        (
          "filter",
          str("part of a name or of the level it comes from")
        ),
        ("offset", integer("first result (paging)")),
        ("limit", integer("number of results, 100 by default")),
      ],
      &[],
    ),
    tool(
      "list_decor",
      "The decor parts of the levels of the game, to reuse anywhere: their prototypes (pillars, \
       arches, catwalks, lamp posts, props) and the pieces of their shell (walls, floors, \
       bridges), with their triangle count. Without level: searches every level by name \
       (filter), each prototype once with the first level that has it. With a level: its whole \
       decor (the first call reads it, a few seconds).",
      vec![
        (
          "level",
          str("a level name from list_levels; omitted: every level")
        ),
        (
          "game",
          str(
            "'jak2': the structures of Jak II (when Jak II was extracted too), placed with \
             place's game 'jak2': converted for Jak 3 (textures and names apart, lighting \
             compared)",
          ),
        ),
        ("filter", str("part of a name (e.g. 'catwalk')")),
        ("offset", integer("first result (paging)")),
        ("limit", integer("number of results, 100 by default")),
      ],
      &[],
    ),
    tool(
      "list_prefabs",
      "The prefabs: the user's (groups of elements saved together) and the game's (game_prefabs: \
       ready-made assemblies of Jak 3's decor by category: buildings, houses, bridges, \
       platforms, vents, generators, industrial structures; made from the extracted levels the \
       first time, which this waits for). Place either with place kind 'prefab' and its name.",
      vec![(
        "rebuild",
        boolean("make the game's prefabs again from the levels")
      )],
      &[],
    ),
    tool(
      "place",
      "Puts in the document an object of the catalog (kind 'object', name from list_catalog), a \
       model (kind 'model', from list_models, or a .glb file made elsewhere with path), a decor \
       part (kind 'decor', name and level from list_decor), a prefab (kind 'prefab') or a new \
       shape (kind 'shape', name box, floor, wall, doorway, window, pillar, ramp, stairs, roof, \
       roof-hip, cylinder or ground, with size, and opening [width, height] for a doorway or a \
       window; its base's middle at the position; ramps and stairs rise towards +z before the \
       turn, a roof's ridge runs along z), at a position in meters (Y is up). Returns the new \
       elements with their id. Undoable in the editor.",
      vec![
        (
          "kind",
          json!({"type": "string", "enum": ["object", "model", "decor", "prefab", "shape"]}),
        ),
        ("name", str("the object, part, prefab or shape")),
        (
          "path",
          str("kind 'model': a .glb or .gltf file instead of a model of the game")
        ),
        (
          "size",
          vec3("kind 'shape': [width, height, depth] in meters")
        ),
        (
          "tile",
          num("kind 'shape': meters per repeat of its texture (2 by default)")
        ),
        (
          "steps",
          integer("kind 'shape', stairs: the number of steps")
        ),
        (
          "bend",
          num(
            "kind 'shape': bent around a vertical axis, in degrees over its length (x; ramps and \
             stairs along their climb): a wall bent 90 is a quarter of a round tower, stairs bent \
             180 a spiral",
          ),
        ),
        (
          "round",
          num("kind 'shape', box: the radius of its upright edges in meters (rounded corners)"),
        ),
        (
          "style",
          str(
            "kind 'shape': a level of the game whose look it takes: a floor or a ground its \
             floors' texture (at their size), light and bumps, the other shapes its walls' (e.g. \
             'ctysluma')",
          ),
        ),
        (
          "bumps",
          num("kind 'shape', ground: meters its top goes up and down")
        ),
        (
          "level",
          str("kind 'decor': the level the part comes from (omitted: the first that has it)"),
        ),
        (
          "game",
          str(
            "kind 'decor' or 'model': 'jak2' for a structure or an object of Jak II (list_decor, \
             list_models with game 'jak2')",
          ),
        ),
        ("position", vec3("where, [x, y, z] in meters")),
        ("yaw_deg", num("turn around the vertical axis, in degrees")),
        (
          "rotation_deg",
          vec3("instead of yaw_deg: [x, y, z] rotation in degrees")
        ),
        (
          "scale",
          any("[x, y, z] or one number: the scale along the element's own axes"),
        ),
        (
          "on_ground",
          boolean("true: dropped onto the first surface below the position"),
        ),
      ],
      &["kind", "name", "position"],
    ),
    tool(
      "place_many",
      "Places many elements at once, each described like the arguments of place (kind, name, \
       level, position, yaw_deg or rotation_deg, scale, on_ground): to build a large area in a \
       few calls. One undo entry for all. Returns the new elements, and an error for each item \
       that could not be placed.",
      vec![(
        "items",
        json!({"type": "array",
               "description": "up to 2000 placements, each an object like place's arguments",
               "items": {"type": "object"}}),
      )],
      &["items"],
    ),
    tool(
      "find_nodes",
      "Finds elements of the document by part of their name, actor type (etype), kind (actor, \
       render, group) or near a point. Returns id, name, kind, type, position, size, box (its \
       bounds, [min, max]), and whether it is locked (terrain) or hidden by the story view. \
       Locked terrain is left out unless include_locked.",
      vec![
        ("name", str("part of the name")),
        ("etype", str("an actor type, e.g. 'crate'")),
        (
          "kind",
          json!({"type": "string", "enum": ["actor", "render", "group", "navmesh"]}),
        ),
        ("near", vec3("a point, [x, y, z] in meters")),
        ("radius", num("distance from `near`, 20 m by default")),
        (
          "box",
          json!({"type": "array",
                 "description": "[[x, y, z], [x, y, z]]: only the elements whose middle is inside"}),
        ),
        ("outside", boolean("with box: only those outside it")),
        (
          "under",
          integer(
            "only the elements inside this group (a level's group: get_status document.levels)"
          ),
        ),
        ("include_locked", boolean("also the terrain")),
        (
          "limit",
          integer("number of results, 50 by default, 20000 at most")
        ),
      ],
      &[],
    ),
    tool(
      "get_node",
      "Everything about one element: position, rotation (degrees) and scale, size, mesh, \
       children, and for an actor its type, model and game data (lump); for an actor or a model \
       of the catalog, what the inspector's preview shows of it (preview: its animations, its \
       movement, how it breaks, its picture).",
      vec![("id", integer("the element"))],
      &["id"],
    ),
    tool(
      "transform_nodes",
      "Moves, turns or scales elements: position sets the world position, translate adds an \
       offset (meters), rotation_deg sets the rotation (degrees around x, y, z), rotate_y_deg \
       turns around the vertical axis, scale sets the scale along the element's own axes ([x, y, \
       z] or one number) and scale_by multiplies it (to stretch a platform: scale_by [2, 1, 1]). \
       The game ignores the scale of most actors: scale decor (parts, models, pieces). Undoable.",
      vec![
        ("ids", ids("the elements")),
        ("position", vec3("[x, y, z] in meters")),
        ("translate", vec3("[dx, dy, dz] in meters")),
        ("rotation_deg", vec3("[x, y, z] in degrees")),
        ("rotate_y_deg", num("degrees")),
        ("scale", any("[x, y, z] or one number: the scale")),
        (
          "scale_by",
          any("[x, y, z] or one number: multiplies the scale")
        ),
      ],
      &["ids"],
    ),
    tool(
      "set_actor",
      "Sets what an object does: its behavior (when a door opens, when a board vent or jump pad \
       throws Jak up, when an elevator leaves, whether the object is there at all), written at \
       export as the level's own code, and its parameters (the data its type's code reads: \
       get_node lists them with their kind and default, e.g. spring-height of a vent, \
       elevator-move-rate, crate-type, eco-info [pickup, amount]). Distances in the game's units \
       (4096 per meter). A condition is {\"when\": \"always\"|\"never\"|\"task\"|\"pressed\"|\
       \"gone\", \"arg\": a task node of the story (task) or an actor's name (pressed: a floor \
       switch was pressed; gone: a crate broken, a pickup taken)}. Undoable.",
      vec![
        ("id", integer("the actor")),
        (
          "behavior",
          json!({"type": "object",
                 "description": "merged into its behavior (null removes a key): open (a door): \
                                 approach|locked|always|when, with when (a condition) and distance \
                                 (meters), and loads (a level: the rooms behind it, loaded and \
                                 shown when Jak comes, the door opening once it is); active (a \
                                 bouncer), leave (an elevator), present (any): a condition"}),
        ),
        (
          "params",
          json!({"type": "object", "description": "tag -> value (null: back to the game's)"}),
        ),
      ],
      &["id"],
    ),
    tool(
      "set_shape",
      "Changes a shape made in the editor (place kind 'shape'): its size in meters, how many \
       meters one repeat of its texture covers, its steps (stairs) or sides (cylinder), its bend \
       (degrees) and the radius of its corners (round, a box). Undoable.",
      vec![
        ("id", integer("the shape")),
        ("size", vec3("[width, height, depth] in meters")),
        ("tile", num("meters per repeat of the texture")),
        ("steps", integer("stairs")),
        ("sides", integer("cylinder")),
        (
          "bend",
          num("degrees around a vertical axis over its length (not a cylinder)")
        ),
        (
          "round",
          num("a box: the radius of its upright edges, meters")
        ),
      ],
      &["id"],
    ),
    tool(
      "set_material",
      "Gives elements (shapes, models, decor) a texture: an image file (PNG, JPG, TGA, BMP) or \
       the name of a texture of the document (list_textures), saved with the project and \
       exported for the game. Only these elements change, not the others drawing the same \
       material. Undoable.",
      vec![
        ("ids", ids("the elements")),
        (
          "texture",
          str("an image file path, or a texture name from list_textures (palette ones too)"),
        ),
        (
          "material",
          integer("only this material of theirs (get_node: materials); omitted: all"),
        ),
      ],
      &["ids", "texture"],
    ),
    tool(
      "list_textures",
      "The textures of the document (those of the levels opened and the images added), by name \
       and size, to reuse with set_material; or with palette: the textures Jak 3's levels use \
       again and again (city walls, metal floors, sandstone, wood...), the most shared first, \
       usable with set_material even when no level having them is open.",
      vec![
        (
          "palette",
          boolean("the palette of the game's recurring textures")
        ),
        (
          "filter",
          str("part of a name, or a theme (the first word: city, wascity, des...)"),
        ),
        ("offset", integer("first result (paging)")),
        ("limit", integer("number of results, 100 by default")),
      ],
      &[],
    ),
    tool(
      "set_visibility",
      "Shows or hides elements (a level's group: the whole level). Hidden elements are not drawn \
       nor picked. Undoable.",
      vec![
        ("ids", ids("the elements")),
        ("visible", boolean("true: shown, false: hidden")),
      ],
      &["ids", "visible"],
    ),
    tool(
      "delete_nodes",
      "Deletes elements and what is under them. Undoable.",
      vec![("ids", ids("the elements"))],
      &["ids"],
    ),
    tool(
      "duplicate_nodes",
      "Copies elements, moved by an offset. Returns the copies. Undoable.",
      vec![
        ("ids", ids("the elements")),
        ("offset", vec3("[dx, dy, dz] in meters")),
      ],
      &["ids"],
    ),
    tool(
      "raycast",
      "Where a ray hits the document (terrain included): the point, the surface's normal and the \
       element. By default straight down: the ground under a point.",
      vec![
        ("origin", vec3("[x, y, z] in meters")),
        ("direction", vec3("[dx, dy, dz], [0, -1, 0] by default")),
        ("actors", boolean("also the actors' markers")),
      ],
      &["origin"],
    ),
    tool(
      "generate_navmesh",
      "Generates nav meshes (where the creatures of the game walk) from the walkable surfaces of \
       the decor shown, in the game's limits (convex polygons of 3 or 4 corners, at most 255 per \
       mesh, each mesh in one piece), replacing the nav meshes of the region. Waits until they \
       are made; returns them (polygons, area, problem if the game could not use one); with \
       from, only the ground connected to given points. Undoable.",
      vec![
        (
          "region",
          any(
            "\"all\" (the default), \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]} in \
             meters",
          ),
        ),
        (
          "agent_radius",
          num("meters the creature keeps from walls (0.6 by default)")
        ),
        (
          "agent_height",
          num("meters of room it needs above the floor (2)")
        ),
        ("max_climb", num("highest step in meters (0.8)")),
        ("max_slope", num("steepest walkable slope in degrees (45)")),
        (
          "cell_size",
          num("voxel size in meters (0.5): finer follows walls closer")
        ),
        (
          "max_edge",
          num("longest polygon edge along walls in meters (60)")
        ),
        (
          "max_error",
          num("how far outlines may stray from walls in meters (0.8)")
        ),
        (
          "min_region",
          num("smallest walkable island kept, in square meters (16)")
        ),
        (
          "from",
          any(
            "keep only the pieces of ground next to these points: [[x, y, z], ...] on the \
             streets where creatures go, or \"selection\" (where the selected elements stand); \
             roofs, ledges and closed rooms are left out",
          ),
        ),
      ],
      &[],
    ),
    tool(
      "export_navmesh",
      "Writes the document's nav meshes for the game: entity-nav-mesh objects as the game's \
       levels hold them (polygons, route table, polygon grid) in a nav_data file (.json) for \
       OpenGOAL's level builder.",
      vec![
        ("path", str("the .json file to write")),
        (
          "first_id",
          integer(
            "the entity id of the first nav mesh (60000 by default): unique in the game, not an \
             actor's",
          ),
        ),
      ],
      &["path"],
    ),
    tool(
      "export_level",
      "Writes what the view shows as a custom level of OpenGOAL for Jak 3, in the project's \
       export folder (<library>/export/<project>/ by default) laid out like the jak-project \
       checkout: custom_assets/jak3/levels/<name>/ (the .glb, the .jsonc and the .gd its level \
       builder takes, level-info.gc), goal_src/jak3/levels/<name>/ (the behaviors' code) and \
       README.md (how to bring it into the game); notes says what changed or was left out, \
       densest the places of the floors where Jak would meet more collision than the game \
       gathers at once (461 triangles: he can fall through), the densest first. Hidden levels \
       and elements are left out.",
      vec![
        (
          "name",
          str(
            "the level's name in the game: 3 to 10 lower case letters, digits or '-' (not a \
             level of the game)",
          ),
        ),
        (
          "nickname",
          str("3 lower case letters: its DGO's name (made from the name by default)"),
        ),
        (
          "iso_name",
          str("up to 8 upper case letters or digits (made from the name by default)"),
        ),
        (
          "base_id",
          integer("the first actor id (above the game's own actors' by default)"),
        ),
        (
          "folder",
          str(
            "the export's folder (README.md, custom_assets/..., goal_src/... in it): by default \
             <library>/export/<project>/",
          ),
        ),
        (
          "actors",
          boolean("false: the decor alone (true by default)")
        ),
        (
          "collision",
          str(
            "'game' (default): the levels' own collision, exact, where they have it, the rest \
             made by the builder from the decor; 'decor': all of it made from the decor",
          ),
        ),
        (
          "start",
          vec3("where the player starts, meters (a continue point of the level opened by default)"),
        ),
        (
          "replaces",
          str(
            "a level of the game this one replaces (replace_level's by default): only the \
             document's own level is written, its DGO has that level's code, and level-info.gc \
             says the one line to change in the game's definition of that level",
          ),
        ),
      ],
      &["name"],
    ),
    tool(
      "set_camera",
      "Moves the editor's camera: position and look_at (points in meters), or yaw_deg and \
       pitch_deg, or frame ('all', 'selection', a list of ids, {\"level\": name} or \
       {\"junction\": [level, level]}: where two levels of the document meet).",
      vec![
        ("position", vec3("[x, y, z] in meters")),
        ("look_at", vec3("[x, y, z] in meters")),
        ("yaw_deg", num("0 looks toward -Z")),
        ("pitch_deg", num("negative looks down")),
        ("frame", any("'all', 'selection' or a list of ids")),
      ],
      &[],
    ),
    tool(
      "set_view",
      "How the 3D view draws the level: game_view true draws it with the game's own renderer (its \
       lighting at the hour of the day and in the weather, its fog, its sky with the sun, the \
       moon and the clouds), as the exported level will look in the game; effects: the particle \
       effects of the level (steam, sparks, lights). The weather is the game's: cloud 0 clear, \
       0.5 overcast, 1 storm; fog 0 haze, 0.5 clear, 1 rain or smog. Answers the view's state.",
      vec![
        ("game_view", boolean("the game's renderer")),
        (
          "hour",
          num("0..24: the time of the day of the game view (12: noon, 22: night)"),
        ),
        (
          "weather",
          str(
            "one of the game's weathers: clear, few_clouds, haze, smog, overcast, overcast_fog, \
             rain, storm, storm_rain, storm_fog",
          ),
        ),
        (
          "cloud",
          num("0..1: the clouds (0 clear, 0.5 overcast, 1 storm)")
        ),
        (
          "fog",
          num("0..1: 0 haze, 0.5 clear, 1 rain (smog under a clear sky)")
        ),
        ("rain", num("0..1: rain falling around the camera")),
        (
          "lightning",
          boolean("a storm's flashes (with cloud above 0.75)")
        ),
        ("level_lights", boolean("the level's lamps (on at night)")),
        ("effects", boolean("the particle effects")),
      ],
      &[],
    ),
    tool(
      "capture_view",
      "A picture of the 3D view from the editor's camera (PNG), to see the result of what was \
       placed. By default it shows what the user's view shows: the game rendering (the level as \
       the game draws it, its lighting at the view's hour, its fog: what the exported level will \
       look like in the game) when it is on. game_hour chooses the hour of the game rendering; \
       -1 the editor's own view.",
      vec![
        ("width", integer("pixels, 960 by default")),
        ("height", integer("pixels, 600 by default")),
        (
          "game_hour",
          num(
            "0..24: the game rendering at that hour (12: noon, 22: night); -1: the editor's view",
          ),
        ),
        (
          "only",
          ids(
            "these elements alone (and what is under them): the others are not drawn in the \
             picture",
          ),
        ),
      ],
      &[],
    ),
    tool(
      "select",
      "Selects elements in the editor, to show them to the user; or a whole level (level: its \
       name), to move it with transform_nodes on the selection's ids; or the assembly an element \
       is part of (assembly: its id): a house with its facades, roof and windows, a bridge with \
       its pillars; or a group (group: its id, the 'group' of an element: a prefab placed, a \
       group made with the tool group).",
      vec![
        ("ids", ids("the elements")),
        ("level", str("the level opened or a level added: all of it")),
        ("assembly", integer("an element: all of its assembly")),
        ("group", integer("a group's id: all of its elements")),
      ],
      &[],
    ),
    tool(
      "group",
      "Makes a named group of elements (Groups panel): they are selected together (a double \
       click on one of them), moved together, listed by list_groups. A prefab placed is a group \
       already.",
      vec![("ids", ids("the elements")), ("name", str("its name"))],
      &["ids", "name"],
    ),
    tool(
      "ungroup",
      "Undoes a group (its elements stay where they are).",
      vec![("group", integer("the group's id"))],
      &["group"],
    ),
    tool(
      "list_groups",
      "The groups of the document: the prefabs placed (prefab true) and the groups named, with \
       their elements and their box.",
      vec![],
      &[],
    ),
    tool(
      "find_gaps",
      "Finds where a player walking the level would see the void (nothing drawn: a visual bug in \
       the game): from points of the ground every few meters, at the height of Jak's eyes, rays \
       around and downward; a ray that meets nothing is a view into the void. Each spot: kind \
       'gap' (a hole between pieces, the void right behind it: fill it) or 'edge' (where the \
       ground ends, seen directly or through a passage: close it unless the player cannot go \
       near), where it is (at), where it is seen from (seen_from, direction: give them to \
       set_camera to look at it) and how many rays went through. Run it after building and fix \
       every gap (rule 1 of the guide). Shown in the editor too.",
      vec![
        (
          "box",
          json!({"type": "array",
                 "description": "[[x, y, z], [x, y, z]]: the region the player can walk"}),
        ),
        (
          "ids",
          ids("or around these elements (their box and a margin)")
        ),
        ("margin", num("meters around ids, 12 by default")),
        (
          "spacing",
          num("meters between the points looked from (0 by default: about 400 points)"),
        ),
        ("eye", num("meters above the ground, 1.6 by default")),
        ("limit", integer("spots returned, 40 by default")),
        (
          "show",
          boolean("listed and drawn in the editor (true by default)")
        ),
      ],
      &[],
    ),
    tool(
      "list_assemblies",
      "The assemblies of the document's decor, the most elements first: the elements touching \
       each other (a house with its facades, roof and windows; a bridge with its pillars; a tower \
       with its balconies), not the ground. Each: its element ids, its box, its triangles, its \
       main element and its parts (name, how many, their volume in m3). A whole structure to \
       select, to copy or to save as a prefab (create_prefab).",
      vec![
        ("min_elements", integer("2 by default")),
        ("min_size", num("meters across at least, 4 by default")),
        (
          "max_size",
          num("meters: a larger one is cut in parts, 60 by default")
        ),
        ("limit", integer("at most this many, 200 by default")),
      ],
      &[],
    ),
    tool(
      "terrain",
      "Draws natural ground with the terrain brush (B): a square `size` meters across at \
       position (along to `to` when given: a road, a wall, a ditch). mode paint lays ground at \
       `height` (the position's y by default) with the relief of the level style; erase takes it \
       away; raise and dig move it `amount` meters at the square's middle, fading to its edges \
       (laying ground first where there is none); smooth evens it, flatten brings it to `height` \
       (amount 0..1); wall raises upright walls `amount` meters high (4 by default), unwall \
       takes them down; generate makes hills over `area` [x, z] meters, `hills` high, \
       `roughness` 0..1. The cells painted (paint, wall, new ground) take the style's look, each \
       part of a terrain keeping its own: flat ground its floor texture, slopes, walls and edges \
       its wall texture. id: the terrain to change (from a previous call), else a new one. \
       Undoable.",
      vec![
        (
          "mode",
          str("paint (default), erase, raise, dig, smooth, flatten, wall, unwall or generate"),
        ),
        ("position", vec3("[x, y, z] in meters: the brush's middle")),
        (
          "to",
          vec3("[x, y, z]: the brush goes from position to there")
        ),
        ("size", num("meters, the square's side (4 by default)")),
        (
          "amount",
          num("raise, dig, wall: meters; smooth, flatten: 0..1")
        ),
        (
          "height",
          num("paint, flatten: the ground's height (meters)")
        ),
        (
          "style",
          str("a level (list_levels) whose floor and wall textures and relief it takes"),
        ),
        (
          "floor_texture",
          str(
            "a texture of the style's level for the ground instead of its floor's (list_textures)"
          ),
        ),
        (
          "wall_texture",
          str("a texture of the style's level for the walls and slopes instead of its walls'"),
        ),
        (
          "relief",
          boolean("paint: the style's bumps (true by default)")
        ),
        (
          "area",
          vec2("generate: [x, z] meters (64 by 64 by default)")
        ),
        (
          "hills",
          num("generate: meters high at most (12 by default)")
        ),
        (
          "roughness",
          num("generate: 0 smooth .. 1 broken (0.5 by default)")
        ),
        ("seed", integer("generate, paint: a variant")),
        ("cell", num("a new terrain's cells, meters (1 by default)")),
        ("id", integer("the terrain to change")),
      ],
      &["position"],
    ),
    tool(
      "set_story",
      "The story view of the game's levels: enabled false shows every state of the level at \
       once; step (1 = start of the game) or after_node (a task node name) shows it at that \
       point of the story.",
      vec![
        ("enabled", boolean("the story view")),
        ("step", integer("1 = the start of the game")),
        (
          "after_node",
          str("a task node, e.g. 'arena-fight-1-resolution'")
        ),
      ],
      &[],
    ),
    tool(
      "create_prefab",
      "Saves elements as a prefab (in library/prefabricated/), to place again with place (kind \
       'prefab'). The elements smaller than min_size are left out (bolts, debris, stains: a \
       prefab is a structure; rule 2 of the guide). Answers its name: the one given, spaces and \
       signs made '_', with -2, -3... when a prefab has it already, and the names of the \
       elements left out.",
      vec![
        ("ids", ids("the elements")),
        ("name", str("the prefab's name")),
        (
          "min_size",
          num("meters: elements smaller across are left out (0.5 by default, 0: keep all)"),
        ),
      ],
      &["ids", "name"],
    ),
    tool(
      "save_project",
      "Saves the document as a project: <library>/projects/<name>/<name>.glb, and its nav meshes \
       for the game (<name>-nav.json) when it has some. name is needed the first time (then it \
       saves there again).",
      vec![
        ("name", str("the project's name (its folder)")),
        ("path", str("or a .glb file to save to")),
      ],
      &[],
    ),
    tool(
      "open_project",
      "Opens a project (.glb) saved before.",
      vec![
        ("path", str("the .glb file")),
        ("discard_changes", discard())
      ],
      &["path"],
    ),
    tool(
      "restore_effects",
      "Adds back the particle effects of the document's levels as the game has them (steam, \
       sparks, lights), where the document has none (a project made before the editor kept them, \
       effects deleted); not those whose decor was taken out of the level. Returns how many \
       were added and left out. Undoable.",
      vec![],
      &[],
    ),
    tool(
      "undo",
      "Undoes the last change of the document.",
      vec![],
      &[]
    ),
    tool("redo", "Redoes the last change undone.", vec![], &[]),
  ])
}

const K_INSTRUCTIONS: &str = "Tools of the OpenGOAL Level Editor (levels of Jak 3), acting on the \
  editor window the user sees (started when needed). Positions are in meters, Y is up. A typical \
  session: get_status; open_level (or new_level); find_nodes / raycast to know where things are; \
  list_catalog, list_decor and list_prefabs to choose what to put; place; capture_view to check; \
  save_project. Every change can be undone. Rules for what you build (docs/mcp-agent-guide.md \
  explains them): 1. No holes: pieces must touch or overlap (by 5 to 10 cm), floors reach the \
  walls, walls reach the ground and each other; a gap lets the player see the void, a visual bug \
  in the game. Run find_gaps on what you built and fix every 'gap' (and every 'edge' the player \
  can reach) before you say it is done. 2. Prefabs hold structures, not crumbs: create_prefab \
  leaves out elements under 0.5 m (min_size); never save loose bolts, debris, stains or tiny \
  props in a prefab. 3. Facades are not buildings: most buildings of the game are only a front, \
  open behind. Place a facade so that only its front can be seen (against a wall, the edge of the \
  level, other buildings, back to back with another facade); never in the middle of a place the \
  player can walk around. Look at it from the back and the sides with set_camera and \
  capture_view: what the player can reach must never show its back. 4. Look before you report: \
  capture_view from the player's height (1.6 m above the ground) around what you built.";

/// the tools that may wait for a level to load or for the editor to read the levels
const SLOW: &[&str] = &[
  "open_level",
  "open_project",
  "list_decor",
  "place",
  "list_catalog",
  "list_models",
  "place_many",
  "generate_navmesh",
  "add_level",
  "export_level",
  "find_gaps",
  "replace_level",
  "list_prefabs",
];

/// The other end of the editor's remote control: a request sent, its answer waited for.
#[derive(Default)]
struct RemoteClient {
  stream: Option<TcpStream>,
  /// bytes read past the last answer
  pending: Vec<u8>,
}

impl RemoteClient {
  fn connect(&mut self, port: u16) -> Result<(), String> {
    self.pending.clear();
    let s = TcpStream::connect(("127.0.0.1", port))
      .map_err(|_| format!("the editor is not running (nothing on port {port})"))?;
    let _ = s.set_nodelay(true);
    self.stream = Some(s);
    Ok(())
  }

  /// sends `request` (one line), reads the answer line
  fn call(&mut self, request: &Value, timeout: Duration) -> Result<Value, String> {
    let Some(s) = self.stream.as_mut() else {
      return Err("not connected".into());
    };
    if writeln!(s, "{request}").is_err() {
      self.stream = None;
      return Err("the connection to the editor was lost".into());
    }
    let deadline = Instant::now() + timeout;
    loop {
      if let Some(eol) = self.pending.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = self.pending.drain(..=eol).collect();
        let Ok(j) = serde_json::from_slice::<Value>(&line) else {
          continue;
        };
        // a late answer (of a request that timed out) is skipped
        if !j.is_object() || j.get("id").is_some_and(|id| *id != request["id"]) {
          continue;
        }
        return Ok(j);
      }
      let left = deadline.saturating_duration_since(Instant::now());
      if left.is_zero() {
        return Err("the editor did not answer in time".into());
      }
      let s = self.stream.as_mut().unwrap();
      let _ = s.set_read_timeout(Some(left.min(Duration::from_millis(200))));
      let mut buf = [0u8; 65536];
      match s.read(&mut buf) {
        Ok(n) if n > 0 => self.pending.extend_from_slice(&buf[..n]),
        Err(e)
          if matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
          ) => {}
        _ => {
          self.stream = None;
          return Err("the editor closed the connection".into());
        }
      }
    }
  }
}

/// The editor's program: next to this one (the app's folder, or target/<profile>/ when built
/// from the repository), else the other profile's build.
fn editor_program() -> Option<PathBuf> {
  let exe = std::env::current_exe().ok()?;
  let dir = exe.parent()?;
  let name = format!("jak-level-editor{}", std::env::consts::EXE_SUFFIX);
  [
    dir.join(&name),
    dir.join("../release").join(&name),
    dir.join("../debug").join(&name),
  ]
  .into_iter()
  .find(|p| p.is_file())
}

/// The editor started in its own window, the user's (it goes on running after this program)
fn start_editor(port: u16) -> Result<(), String> {
  let exe = editor_program().ok_or("cannot find the editor's program")?;
  let mut c = Command::new(exe);
  c.args(["--remote-port", &port.to_string()])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP: not this program's console, nor its Ctrl+C
    c.creation_flags(0x0000_0008 | 0x0000_0200);
  }
  c.spawn()
    .map_err(|e| format!("cannot start the editor: {e}"))?;
  Ok(())
}

fn tool_error(text: &str) -> Value {
  json!({"content": [{"type": "text", "text": text}], "isError": true})
}

/// The MCP answer of a tool: its result as text, its picture as an image
fn tool_answer(answer: &Value) -> Value {
  if answer.get("ok").and_then(Value::as_bool) != Some(true) {
    let error = answer
      .get("error")
      .and_then(Value::as_str)
      .unwrap_or("the editor refused");
    return tool_error(error);
  }
  let result = answer.get("result").cloned().unwrap_or_else(|| json!({}));
  // indented by one space, as the C++ server wrote it
  let mut text = Vec::new();
  let f = serde_json::ser::PrettyFormatter::with_indent(b" ");
  let _ = result.serialize(&mut serde_json::Serializer::with_formatter(&mut text, f));
  let text = String::from_utf8(text).unwrap_or_default();
  let mut content = vec![json!({"type": "text", "text": text})];
  if let Some(image) = answer.get("image").and_then(Value::as_str) {
    content.push(json!({"type": "image", "data": image, "mimeType": "image/png"}));
  }
  json!({"content": content, "isError": false})
}

struct Server {
  port: u16,
  client: RemoteClient,
  next_id: u64,
}

impl Server {
  fn call_tool(&mut self, name: &str, args: &Value) -> Value {
    if self.client.stream.is_none() && self.client.connect(self.port).is_err() {
      // no editor running: start one and wait for it
      if let Err(e) = start_editor(self.port) {
        return tool_error(&e);
      }
      let deadline = Instant::now() + Duration::from_secs(40);
      while let Err(e) = self.client.connect(self.port) {
        if Instant::now() > deadline {
          return tool_error(&format!("the editor did not start: {e}"));
        }
        std::thread::sleep(Duration::from_millis(300));
      }
    }
    let args = if args.is_object() {
      args.clone()
    } else {
      json!({})
    };
    let request = json!({"id": self.next_id, "tool": name, "args": args});
    self.next_id += 1;
    let timeout = Duration::from_secs(if SLOW.contains(&name) { 300 } else { 60 });
    match self.client.call(&request, timeout) {
      Ok(answer) => tool_answer(&answer),
      Err(e) => tool_error(&e),
    }
  }

  /// The answer to one message of the client, None for a notification
  fn answer(&mut self, line: &str) -> Option<Value> {
    let msg = match serde_json::from_str::<Value>(line) {
      Ok(m) if m.is_object() => m,
      _ => {
        return Some(
          json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
        );
      }
    };
    // a notification (initialized, cancelled...): no answer
    let id = msg.get("id")?.clone();
    let method = msg
      .get("method")
      .and_then(Value::as_str)
      .unwrap_or_default();
    let params = msg
      .get("params")
      .filter(|p| p.is_object())
      .cloned()
      .unwrap_or_else(|| json!({}));
    let result = match method {
      "initialize" => {
        // the client's version when known, else the latest this server speaks
        const KNOWN: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
        let asked = params
          .get("protocolVersion")
          .and_then(Value::as_str)
          .unwrap_or_default();
        let version = KNOWN.into_iter().find(|&k| k == asked).unwrap_or(KNOWN[0]);
        json!({
          "protocolVersion": version,
          "capabilities": {"tools": {"listChanged": false}},
          "serverInfo": {"name": "jak-level-editor", "version": env!("CARGO_PKG_VERSION")},
          "instructions": K_INSTRUCTIONS,
        })
      }
      "ping" => json!({}),
      "tools/list" => json!({ "tools": tools() }),
      "tools/call" => {
        let name = params
          .get("name")
          .and_then(Value::as_str)
          .unwrap_or_default();
        let args = params
          .get("arguments")
          .cloned()
          .unwrap_or_else(|| json!({}));
        self.call_tool(name, &args)
      }
      _ => {
        return Some(json!({"jsonrpc": "2.0", "id": id,
          "error": {"code": -32601, "message": format!("method not found: {method}")}}));
      }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
  }
}

fn main() {
  let mut port = K_REMOTE_PORT;
  let mut args = std::env::args().skip(1);
  while let Some(a) = args.next() {
    if a == "--remote-port"
      && let Some(p) = args.next().and_then(|p| p.parse().ok())
    {
      port = p;
    }
  }
  let mut server = Server {
    port,
    client: RemoteClient::default(),
    next_id: 1,
  };
  let stdin = std::io::stdin();
  let mut stdout = std::io::stdout().lock();
  for line in stdin.lock().lines() {
    let Ok(line) = line else { break };
    let line = line.trim_end_matches('\r');
    if line.trim().is_empty() {
      continue;
    }
    if let Some(answer) = server.answer(line)
      && writeln!(stdout, "{answer}")
        .and_then(|_| stdout.flush())
        .is_err()
    {
      break;
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::BufReader;
  use std::net::TcpListener;

  #[test]
  fn initialize_and_tools_list() {
    let mut s = Server {
      port: 1,
      client: RemoteClient::default(),
      next_id: 1,
    };
    let init = s
      .answer(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#)
      .unwrap();
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert!(
      init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("4. Look before you report")
    );
    assert!(
      s.answer(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none()
    );
    let list = s
      .answer(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
      .unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 48);
    let place = tools.iter().find(|t| t["name"] == "place").unwrap();
    assert_eq!(
      place["inputSchema"]["required"],
      json!(["kind", "name", "position"])
    );
    assert_eq!(
      place["inputSchema"]["properties"]["position"]["minItems"],
      3
    );
    let unknown = s
      .answer(r#"{"jsonrpc":"2.0","id":3,"method":"nope"}"#)
      .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);
    assert_eq!(s.answer("{oops").unwrap()["error"]["code"], -32700);
  }

  #[test]
  fn tools_call_forwards_to_the_editors_port() {
    // a fake editor: answers each request with its tool and a picture
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let editor = std::thread::spawn(move || {
      let (stream, _) = listener.accept().unwrap();
      let mut out = stream.try_clone().unwrap();
      for line in BufReader::new(stream).lines().take(2) {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let answer = if r["tool"] == "capture_view" {
          json!({"id": r["id"], "ok": true, "result": {"width": 4}, "image": "iVBO"})
        } else {
          json!({"id": r["id"], "ok": false, "error": format!("no {}", r["tool"].as_str().unwrap())})
        };
        // a late answer of another request first: skipped
        writeln!(out, "{}", json!({"id": 999, "ok": true})).unwrap();
        writeln!(out, "{answer}").unwrap();
      }
    });
    let mut s = Server {
      port,
      client: RemoteClient::default(),
      next_id: 1,
    };
    let shot = s
      .answer(r#"{"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"capture_view","arguments":{}}}"#)
      .unwrap();
    assert_eq!(shot["id"], "a");
    let content = &shot["result"]["content"];
    assert_eq!(shot["result"]["isError"], false);
    assert!(
      content[0]["text"]
        .as_str()
        .unwrap()
        .contains("\"width\": 4")
    );
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["data"], "iVBO");
    let refused = s
      .answer(r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"undo"}}"#)
      .unwrap();
    assert_eq!(refused["result"]["isError"], true);
    assert_eq!(refused["result"]["content"][0]["text"], "no undo");
    editor.join().unwrap();
  }
}
