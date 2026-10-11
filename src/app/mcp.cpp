// open-goal-level-editor --mcp: an MCP server (Model Context Protocol: JSON-RPC 2.0, one message
// per line on stdin and stdout) that gives an assistant such as Claude the tools of the editor:
// open a level or start a new one, find what it holds, place objects of the catalog, decor parts
// and prefabs, move, turn, copy and delete elements, look through the camera, save the project.
//
// It forwards each tool call to the editor running on this computer (remote.h), and starts the
// editor when none is running: everything happens in the window the user sees, and can be undone
// there. Nothing but the protocol goes to stdout.

#include <SDL3/SDL.h>

#include <chrono>
#include <cstdio>
#include <iostream>
#include <string>
#include <thread>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <windows.h>
#else
#include <unistd.h>
#endif

#include "app/remote.h"
#include "core/log.h"

namespace ogle {

namespace {

constexpr const char* kVersion = "0.4";

json vec3(const char* description) {
  return {{"type", "array"}, {"items", {{"type", "number"}}}, {"minItems", 3}, {"maxItems", 3},
          {"description", description}};
}

json ids(const char* description) {
  return {{"type", "array"}, {"items", {{"type", "integer"}}}, {"description", description}};
}

json str(const char* description) { return {{"type", "string"}, {"description", description}}; }
json num(const char* description) { return {{"type", "number"}, {"description", description}}; }
json integer(const char* description) { return {{"type", "integer"}, {"description", description}}; }
json boolean(const char* description) { return {{"type", "boolean"}, {"description", description}}; }

json tool(const char* name, const char* description, json properties = json::object(),
          std::vector<std::string> required = {}) {
  json schema = {{"type", "object"}, {"properties", std::move(properties)}};
  if (!required.empty()) schema["required"] = required;
  return {{"name", name}, {"description", description}, {"inputSchema", schema}};
}

json tools() {
  const json discard = boolean("true to drop the unsaved changes of the document");
  return json::array({
      tool("get_status",
           "What the editor shows now: the library and its number of levels, the document (level or project, number "
           "of elements, bounds, unsaved changes), the story view, the camera and the selection. Call it first."),
      tool("list_levels",
           "The levels of the game that can be opened or added, with their DGO file and whether the document has "
           "them. With near: only the levels the game displays together with that one (its neighbors).",
           {{"filter", str("part of a level name or DGO (e.g. 'des' for the desert)")},
            {"near", str("a level: its neighbors in the game only")}}),
      tool("get_level_info",
           "What is known of a level: its neighbors in the game (the levels displayed with it), its companions, its "
           "continue points (where the game starts the player: position in meters, levels shown there), and when the "
           "document has it, its role (opened, added, companion), its group and its bounds.",
           {{"name", str("a level, the one opened by default")}}),
      tool("add_level",
           "Adds a level of the library to the document, at its place in the game's world: levels side by side in "
           "the game meet in the editor too (to edit the junction between two levels, or to build a level next to "
           "one of the game's). Its companions and mission layers come with it, under its group. Waits until it is "
           "added; returns its group and bounds. Undoable.",
           {{"name", str("a level from list_levels")},
            {"actors", boolean("false: its decor alone, without any actor (true by default: with its objects)")},
            {"reference", boolean("true: shown to compare with (washed in blue), never selected nor exported: the "
                                  "game's level the document replaces")}},
           {"name"}),
      tool("remove_level", "Removes a level added to the document, and what is under its group. Undoable.",
           {{"name", str("a level added with add_level")}, {"reference", boolean("the one added as a reference")}},
           {"name"}),
      tool("set_decor",
           "What the game does with elements of the decor (pieces, models, shapes, Jak II structures), written by "
           "export_level: collision {kind: shape|none|invisible, material: stone|ice|quicksand|waterbottom|tar|"
           "sand|wood|grass|pcmetal|snow|deepsnow|hotcoals|lava|crwood|gravel|dirt|metal|straw|tube|swamp|"
           "stopproj|rotate|neutral, event: none|deadly|endlessfall|burn|deadlyup|burnup|melt, mode: auto|ground|"
           "wall|obstacle, noedge, nocamera, noentity, nolineofsight}; bounce {height: meters}: Jak landing on its "
           "top is thrown up (the game's bouncer without a model); launch {distance, height (meters), yaw "
           "(degrees from the element's facing), time (seconds)}: Jak stepping on it is thrown along an arc. "
           "false for a part: back to the game's. Undoable.",
           {{"ids", ids("elements of the decor")},
            {"collision", {{"description", "an object as above, or false"}}},
            {"bounce", {{"description", "{height} or false"}}},
            {"launch", {{"description", "{distance, height, yaw, time} or false"}}}},
           {"ids"}),
      tool("replace_level",
           "Makes the document a replacement of a level of the game (the game then loads it in that level's place, "
           "with that level's continue points, music and missions; export_level writes it so). level: the game's "
           "level ('' for none); show_original: adds that level as a reference (blue, not exported) and waits; "
           "align: moves the document's own level so that its middle and bottom are where the original's are; "
           "turn_deg: turns it around its middle. Returns where the game starts the player there: the ground "
           "under each continue point of the level replaced (null: none, the player would fall). Undoable.",
           {{"level", str("a level of the game, e.g. 'ctyindb'")},
            {"show_original", boolean("add the original as a reference")},
            {"align", boolean("place the document's level where the original is")},
            {"turn_deg", num("turn the document's level around its middle (degrees)")}}),
      tool("open_level",
           "Opens a level of the game (its decor, its objects and water, without its creatures) and waits until it "
           "is loaded. Refuses when the document has unsaved changes, unless discard_changes is true.",
           {{"name", str("a level name from list_levels")},
            {"actors", boolean("false: its decor alone, without any actor (true by default: with its objects)")},
            {"discard_changes", discard}},
           {"name"}),
      tool("new_level",
           "Starts an empty level, to build one from the catalog, the decor of the game's levels and prefabs "
           "(nothing to stand on at first: place decor pieces or a prefab).",
           {{"discard_changes", discard}}),
      tool("list_catalog",
           "The objects that can be placed, by family, with how many the game's levels have: the objects that make up "
           "a place (group 'objects': props that only stand there, doors and airlocks, water, dark eco and lava, the "
           "particle effects of the levels: lights, neon signs, steam, drawn as markers) and the interactive ones "
           "(group 'interactive': crates, platforms, elevators, bouncers, buttons, breakables, hazards). Never the "
           "living (enemies, characters, animals), the collectibles nor the technical types.",
           {{"group", str("'objects' or 'interactive' (both by default)")},
            {"family", str("objects: prop, door, water, effect; interactive: crate, platform, movement, button, "
                           "hazard, breakable, light, weapon, door-like, other")},
            {"filter", str("part of a name")},
            {"offset", integer("first result (paging)")},
            {"limit", integer("number of results, 100 by default")}}),
      tool("list_models",
           "The other models of the game (parts of objects, debris, props: everything but the characters), placed "
           "as decor with place (kind 'model').",
           {{"filter", str("part of a name or of the level it comes from")},
            {"offset", integer("first result (paging)")},
            {"limit", integer("number of results, 100 by default")}}),
      tool("list_decor",
           "The decor parts of the levels of the game, to reuse anywhere: their prototypes (pillars, "
           "arches, catwalks, lamp posts, props) and the pieces of their shell (walls, floors, bridges), with their "
           "triangle count. Without level: searches every level by name (filter), each prototype once "
           "with the first level that has it. With a level: its whole decor (the first call reads it, a few "
           "seconds).",
           {{"level", str("a level name from list_levels; omitted: every level")},
            {"game", str("'jak2': the structures of Jak II (when Jak II was extracted too), placed with place's game "
                         "'jak2': converted for Jak 3 (textures and names apart, lighting compared)")},
            {"filter", str("part of a name (e.g. 'catwalk')")},
            {"offset", integer("first result (paging)")},
            {"limit", integer("number of results, 100 by default")}}),
      tool("list_prefabs",
           "The prefabs: the user's (groups of elements saved together) and the game's (game_prefabs: ready-made "
           "assemblies of Jak 3's decor by category: buildings, houses, bridges, platforms, vents, generators, "
           "industrial structures; made from the extracted levels the first time, which this waits for). Place "
           "either with place kind 'prefab' and its name.",
           {{"rebuild", boolean("make the game's prefabs again from the levels")}}),
      tool("place",
           "Puts in the document an object of the catalog (kind 'object', name from list_catalog), a model (kind "
           "'model', from list_models, or a .glb file made elsewhere with path), a decor part (kind 'decor', name and "
           "level from list_decor), a prefab (kind 'prefab') or a new shape (kind 'shape', name box, floor, wall, "
           "doorway, window, pillar, ramp, stairs, roof, roof-hip or cylinder, with size, and opening [width, "
           "height] for a doorway or a window; its base's middle at the position; ramps and stairs rise towards +z "
           "before the turn, a roof's ridge runs along z), at a position in meters (Y is up). Returns the new elements with their id. Undoable "
           "in the editor.",
           {{"kind", {{"type", "string"}, {"enum", {"object", "model", "decor", "prefab", "shape"}}}},
            {"name", str("the object, part, prefab or shape")},
            {"path", str("kind 'model': a .glb or .gltf file instead of a model of the game")},
            {"size", vec3("kind 'shape': [width, height, depth] in meters")},
            {"tile", num("kind 'shape': meters per repeat of its texture (2 by default)")},
            {"steps", integer("kind 'shape', stairs: the number of steps")},
            {"level", str("kind 'decor': the level the part comes from (omitted: the first that has it)")},
            {"game", str("kind 'decor': 'jak2' for a structure of Jak II (list_decor game 'jak2')")},
            {"position", vec3("where, [x, y, z] in meters")},
            {"yaw_deg", num("turn around the vertical axis, in degrees")},
            {"rotation_deg", vec3("instead of yaw_deg: [x, y, z] rotation in degrees")},
            {"scale", {{"description", "[x, y, z] or one number: the scale along the element's own axes"}}},
            {"on_ground", boolean("true: dropped onto the first surface below the position")}},
           {"kind", "name", "position"}),
      tool("place_many",
           "Places many elements at once, each described like the arguments of place (kind, name, level, position, "
           "yaw_deg or rotation_deg, scale, on_ground): to build a large area in a few calls. One undo entry for "
           "all. Returns the new elements, and an error for each item that could not be placed.",
           {{"items", {{"type", "array"}, {"description", "up to 2000 placements, each an object like place's arguments"},
                       {"items", {{"type", "object"}}}}}},
           {"items"}),
      tool("find_nodes",
           "Finds elements of the document by part of their name, actor type (etype), kind (actor, render, group) "
           "or near a point. Returns id, name, kind, type, position, size, and whether it is locked (terrain) or "
           "hidden by the story view. Locked terrain is left out unless include_locked.",
           {{"name", str("part of the name")},
            {"etype", str("an actor type, e.g. 'crate'")},
            {"kind", {{"type", "string"}, {"enum", {"actor", "render", "group", "navmesh"}}}},
            {"near", vec3("a point, [x, y, z] in meters")},
            {"radius", num("distance from `near`, 20 m by default")},
            {"include_locked", boolean("also the terrain")},
            {"limit", integer("number of results, 50 by default")}}),
      tool("get_node",
           "Everything about one element: position, rotation (degrees) and scale, size, mesh, children, and for an "
           "actor its type, model and game data (lump).",
           {{"id", integer("the element")}}, {"id"}),
      tool("transform_nodes",
           "Moves, turns or scales elements: position sets the world position, translate adds an offset (meters), "
           "rotation_deg sets the rotation (degrees around x, y, z), rotate_y_deg turns around the vertical axis, "
           "scale sets the scale along the element's own axes ([x, y, z] or one number) and scale_by multiplies it "
           "(to stretch a platform: scale_by [2, 1, 1]). The game ignores the scale of most actors: scale decor "
           "(parts, models, pieces). Undoable.",
           {{"ids", ids("the elements")},
            {"position", vec3("[x, y, z] in meters")},
            {"translate", vec3("[dx, dy, dz] in meters")},
            {"rotation_deg", vec3("[x, y, z] in degrees")},
            {"rotate_y_deg", num("degrees")},
            {"scale", {{"description", "[x, y, z] or one number: the scale"}}},
            {"scale_by", {{"description", "[x, y, z] or one number: multiplies the scale"}}}},
           {"ids"}),
      tool("set_actor",
           "Sets what an object does: its behavior (when a door opens, when a board vent or jump pad throws Jak "
           "up, when an elevator leaves, whether the object is there at all), written at export as the level's own "
           "code, and its parameters (the data its type's code reads: get_node lists them with their kind and "
           "default, e.g. spring-height of a vent, elevator-move-rate, crate-type, eco-info [pickup, amount]). "
           "Distances in the game's units (4096 per meter). A condition is {\"when\": \"always\"|\"never\"|"
           "\"task\"|\"pressed\"|\"gone\", \"arg\": a task node of the story (task) or an actor's name (pressed: "
           "a floor switch was pressed; gone: a crate broken, a pickup taken)}. Undoable.",
           {{"id", integer("the actor")},
            {"behavior", {{"type", "object"},
                          {"description", "merged into its behavior (null removes a key): open (a door): "
                                          "approach|locked|always|when, with when (a condition) and distance "
                                          "(meters); active (a bouncer), leave (an elevator), present (any): a "
                                          "condition"}}},
            {"params", {{"type", "object"}, {"description", "tag -> value (null: back to the game's)"}}}},
           {"id"}),
      tool("set_shape",
           "Changes a shape made in the editor (place kind 'shape'): its size in meters, how many meters one repeat "
           "of its texture covers, its steps (stairs) or sides (cylinder). Undoable.",
           {{"id", integer("the shape")},
            {"size", vec3("[width, height, depth] in meters")},
            {"tile", num("meters per repeat of the texture")},
            {"steps", integer("stairs")},
            {"sides", integer("cylinder")}},
           {"id"}),
      tool("set_material",
           "Gives elements (shapes, models, decor) a texture and/or a color: an image file (PNG, JPG, TGA, BMP) or "
           "the name of a texture of the document (list_textures), saved with the project and exported for the "
           "game. Only these elements change, not the others drawing the same material. Undoable.",
           {{"ids", ids("the elements")},
            {"texture", str("an image file path, or a texture name from list_textures (palette ones too)")},
            {"color", {{"description", "[r, g, b] or [r, g, b, a]: multiplies the texture (the game's materials are at 2)"}}},
            {"material", integer("only this material of theirs (get_node: materials); omitted: all")}},
           {"ids"}),
      tool("list_textures",
           "The textures of the document (those of the levels opened and the images added), by name and size, to "
           "reuse with set_material; or with palette: the textures Jak 3's levels use again and again (city walls, "
           "metal floors, sandstone, wood...), the most shared first, usable with set_material even when no level "
           "having them is open.",
           {{"palette", boolean("the palette of the game's recurring textures")},
            {"filter", str("part of a name, or a theme (the first word: city, wascity, des...)")},
            {"offset", integer("first result (paging)")},
            {"limit", integer("number of results, 100 by default")}}),
      tool("set_visibility",
           "Shows or hides elements (a level's group: the whole level). Hidden elements are not drawn nor picked. "
           "Undoable.",
           {{"ids", ids("the elements")}, {"visible", boolean("true: shown, false: hidden")}}, {"ids", "visible"}),
      tool("delete_nodes", "Deletes elements and what is under them. Undoable.", {{"ids", ids("the elements")}},
           {"ids"}),
      tool("duplicate_nodes", "Copies elements, moved by an offset. Returns the copies. Undoable.",
           {{"ids", ids("the elements")}, {"offset", vec3("[dx, dy, dz] in meters")}}, {"ids"}),
      tool("raycast",
           "Where a ray hits the document (terrain included): the point, the surface's normal and the element. By "
           "default straight down: the ground under a point.",
           {{"origin", vec3("[x, y, z] in meters")},
            {"direction", vec3("[dx, dy, dz], [0, -1, 0] by default")},
            {"actors", boolean("also the actors' markers")}},
           {"origin"}),
      tool("generate_navmesh",
           "Generates nav meshes (where the creatures of the game walk) from the walkable surfaces of the decor "
           "shown, in the game's limits (convex polygons of 3 or 4 corners, at most 255 per mesh, each mesh in one "
           "piece), replacing the nav meshes of the region. Waits until they are made; returns them (polygons, area, "
           "problem if the game could not use one); with from, only the ground connected to given points. Undoable.",
           {{"region", {{"description", "\"all\" (the default), \"selection\" or {\"min\": [x, y, z], \"max\": [x, y, z]} in meters"}}},
            {"agent_radius", num("meters the creature keeps from walls (0.6 by default)")},
            {"agent_height", num("meters of room it needs above the floor (2)")},
            {"max_climb", num("highest step in meters (0.8)")},
            {"max_slope", num("steepest walkable slope in degrees (45)")},
            {"cell_size", num("voxel size in meters (0.5): finer follows walls closer")},
            {"max_edge", num("longest polygon edge along walls in meters (60)")},
            {"max_error", num("how far outlines may stray from walls in meters (0.8)")},
            {"min_region", num("smallest walkable island kept, in square meters (16)")},
            {"from", {{"description", "keep only the pieces of ground next to these points: [[x, y, z], ...] on the "
                                      "streets where creatures go, or \"selection\" (where the selected elements "
                                      "stand); roofs, ledges and closed rooms are left out"}}}}),
      tool("export_navmesh",
           "Writes the document's nav meshes for the game: entity-nav-mesh objects as the game's levels hold them "
           "(polygons, route table, polygon grid) in a nav_data file (.json) for OpenGOAL's level builder.",
           {{"path", str("the .json file to write")},
            {"first_id", integer("the entity id of the first nav mesh (60000 by default): unique in the game, not an actor's")}},
           {"path"}),
      tool("export_level",
           "Writes what the view shows as a custom level of OpenGOAL for Jak 3: <folder>/<name>/ with the .glb, "
           "the .jsonc and the .gd its level builder takes, level-info.gc and INSTALL.md (the lines to add to the "
           "game's code); notes says what changed or was left out. Hidden levels and elements are left out.",
           {{"name", str("the level's name in the game: 3 to 10 lower case letters, digits or '-' (not a level of "
                         "the game)")},
            {"nickname", str("3 lower case letters: its DGO's name (made from the name by default)")},
            {"iso_name", str("up to 8 upper case letters or digits (made from the name by default)")},
            {"base_id", integer("the first actor id (above the game's own actors' by default)")},
            {"folder", str("where <name>/ is written: custom_assets/jak3/levels of a jak-project checkout, "
                           "or by default the last folder used, else exports/ next to the extracted game")},
            {"actors", boolean("false: the decor alone (true by default)")},
            {"collision", str("'game' (default): the levels' own collision, exact, where they have it, the rest "
                              "made by the builder from the decor; 'decor': all of it made from the decor")},
            {"start", vec3("where the player starts, meters (a continue point of the level opened by default)")},
            {"replaces", str("a level of the game this one replaces (replace_level's by default): only the "
                             "document's own level is written, its DGO has that level's code, and level-info.gc "
                             "says the one line to change in the game's definition of that level")}},
           {"name"}),
      tool("set_camera",
           "Moves the editor's camera: position and look_at (points in meters), or yaw_deg and pitch_deg, or frame "
           "('all', 'selection', a list of ids, {\"level\": name} or {\"junction\": [level, level]}: where two "
           "levels of the document meet).",
           {{"position", vec3("[x, y, z] in meters")},
            {"look_at", vec3("[x, y, z] in meters")},
            {"yaw_deg", num("0 looks toward -Z")},
            {"pitch_deg", num("negative looks down")},
            {"frame", {{"description", "'all', 'selection' or a list of ids"}}}}),
      tool("capture_view",
           "A picture of the 3D view from the editor's camera (PNG), to see the result of what was placed. With "
           "game_hour, the level as the game draws it (the game's renderer, its lighting at that hour of the day, its "
           "fog): what the exported level will look like in the game.",
           {{"width", integer("pixels, 960 by default")},
            {"height", integer("pixels, 600 by default")},
            {"game_hour", num("0..24: the game view at that hour (12: noon, 22: night)")}}),
      tool("select",
           "Selects elements in the editor, to show them to the user; or a whole level (level: its name), to move "
           "it with transform_nodes on the selection's ids.",
           {{"ids", ids("the elements")}, {"level", str("the level opened or a level added: all of it")}}),
      tool("set_story",
           "The story view of the game's levels: enabled false shows every state of the level at once; step "
           "(1 = start of the game) or after_node (a task node name) shows it at that point of the story.",
           {{"enabled", boolean("the story view")},
            {"step", integer("1 = the start of the game")},
            {"after_node", str("a task node, e.g. 'arena-fight-1-resolution'")}}),
      tool("create_prefab", "Saves elements as a prefab, to place again with place (kind 'prefab').",
           {{"ids", ids("the elements")}, {"name", str("the prefab's name")}}, {"ids", "name"}),
      tool("save_project",
           "Saves the document as a project (.glb). path is needed the first time (an absolute path of a .glb "
           "file).",
           {{"path", str("where to save")}}),
      tool("open_project", "Opens a project (.glb) saved before.",
           {{"path", str("the .glb file")}, {"discard_changes", discard}}, {"path"}),
      tool("restore_effects",
           "Adds back the particle effects of the document's levels as the game has them (steam, sparks, lights), "
           "where the document has none (a project made before the editor kept them, effects deleted); not those "
           "whose decor was taken out of the level. Returns how many were added and left out. Undoable."),
      tool("undo", "Undoes the last change of the document."),
      tool("redo", "Redoes the last change undone."),
  });
}

const char* kInstructions =
    "Tools of the OpenGOAL Level Editor (levels of Jak 3), acting on the editor window the "
    "user sees (started when needed). Positions are in meters, Y is up. A typical session: get_status; open_level "
    "(or new_level); find_nodes / raycast to know where things are; list_catalog, list_decor and list_prefabs to "
    "choose what to put; place; capture_view to check; save_project. Every change can be undone.";

void write(const json& message) {
  const std::string text = message.dump(-1, ' ', false, json::error_handler_t::replace) + "\n";
  std::fwrite(text.data(), 1, text.size(), stdout);
  std::fflush(stdout);
}

json tool_error(const std::string& text) {
  return {{"content", json::array({{{"type", "text"}, {"text", text}}})}, {"isError", true}};
}

// this program's own file: the editor to start
std::string own_executable() {
#ifdef _WIN32
  wchar_t buf[32768];
  const DWORD n = GetModuleFileNameW(nullptr, buf, 32768);
  if (!n) return {};
  const int len = WideCharToMultiByte(CP_UTF8, 0, buf, (int)n, nullptr, 0, nullptr, nullptr);
  std::string out(len, '\0');
  WideCharToMultiByte(CP_UTF8, 0, buf, (int)n, out.data(), len, nullptr, nullptr);
  return out;
#else
  char buf[4096];
  const ssize_t n = readlink("/proc/self/exe", buf, sizeof(buf) - 1);
  return n > 0 ? std::string(buf, (size_t)n) : std::string();
#endif
}

bool start_editor(int port, std::string* error) {
  const std::string exe = own_executable();
  if (exe.empty()) {
    if (error) *error = "cannot find the editor's program";
    return false;
  }
  const std::string port_text = std::to_string(port);
  const char* args[] = {exe.c_str(), "--remote-port", port_text.c_str(), nullptr};
  SDL_PropertiesID props = SDL_CreateProperties();
  SDL_SetPointerProperty(props, SDL_PROP_PROCESS_CREATE_ARGS_POINTER, (void*)args);
  SDL_SetBooleanProperty(props, SDL_PROP_PROCESS_CREATE_BACKGROUND_BOOLEAN, true);
  SDL_Process* process = SDL_CreateProcessWithProperties(props);
  SDL_DestroyProperties(props);
  if (!process) {
    if (error) *error = std::string("cannot start the editor: ") + SDL_GetError();
    return false;
  }
  SDL_DestroyProcess(process);  // it goes on running
  return true;
}

json call_tool(RemoteClient& client, int port, const std::string& name, const json& args) {
  static int next_id = 1;
  std::string err;
  if (!client.connected() && !client.connect(port, &err)) {
    // no editor running: start one and wait for it
    if (!start_editor(port, &err)) return tool_error(err);
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(40);
    while (!client.connect(port, &err)) {
      if (std::chrono::steady_clock::now() > deadline) return tool_error("the editor did not start: " + err);
      std::this_thread::sleep_for(std::chrono::milliseconds(300));
    }
  }
  const json request = {{"id", next_id++}, {"tool", name}, {"args", args.is_object() ? args : json::object()}};
  // the ones that may wait for a level to load or for the editor to read the levels
  const bool slow = name == "open_level" || name == "open_project" || name == "list_decor" || name == "place" ||
                    name == "list_catalog" || name == "list_models" || name == "place_many" ||
                    name == "generate_navmesh" || name == "add_level" || name == "export_level" ||
                    name == "replace_level" || name == "list_prefabs";
  json answer;
  if (!client.call(request, &answer, slow ? 300000 : 60000, &err)) return tool_error(err);
  if (!answer.value("ok", false)) return tool_error(answer.value("error", std::string("the editor refused")));
  json content = json::array();
  const json result = answer.contains("result") ? answer["result"] : json::object();
  content.push_back({{"type", "text"}, {"text", result.dump(1, ' ', false, json::error_handler_t::replace)}});
  if (answer.contains("image") && answer["image"].is_string())
    content.push_back({{"type", "image"}, {"data", answer["image"]}, {"mimeType", "image/png"}});
  return {{"content", content}, {"isError", false}};
}

}  // namespace

int run_mcp_server(int port) {
#ifdef _WIN32
  _setmode(_fileno(stdin), _O_BINARY);
  _setmode(_fileno(stdout), _O_BINARY);
#endif
  Log::to_stderr = true;  // stdout is the protocol's
  RemoteClient client;
  std::string line;
  while (std::getline(std::cin, line)) {
    if (!line.empty() && line.back() == '\r') line.pop_back();
    if (line.find_first_not_of(" \t") == std::string::npos) continue;
    const json msg = json::parse(line, nullptr, false);
    if (!msg.is_object()) {
      write({{"jsonrpc", "2.0"}, {"id", nullptr}, {"error", {{"code", -32700}, {"message", "parse error"}}}});
      continue;
    }
    if (!msg.contains("id")) continue;  // a notification (initialized, cancelled...): no answer
    const json id = msg["id"];
    const std::string method = msg.value("method", std::string());
    const json params = msg.contains("params") && msg["params"].is_object() ? msg["params"] : json::object();
    json result;
    if (method == "initialize") {
      // the client's version when known, else the latest this server speaks
      static const char* known[] = {"2025-06-18", "2025-03-26", "2024-11-05"};
      std::string version = known[0];
      const std::string asked = params.value("protocolVersion", std::string());
      for (const char* k : known)
        if (asked == k) version = k;
      result = {{"protocolVersion", version},
                {"capabilities", {{"tools", {{"listChanged", false}}}}},
                {"serverInfo", {{"name", "open-goal-level-editor"}, {"version", kVersion}}},
                {"instructions", kInstructions}};
    } else if (method == "ping") {
      result = json::object();
    } else if (method == "tools/list") {
      result = {{"tools", tools()}};
    } else if (method == "tools/call") {
      result = call_tool(client, port, params.value("name", std::string()),
                         params.contains("arguments") ? params["arguments"] : json::object());
    } else {
      write({{"jsonrpc", "2.0"}, {"id", id}, {"error", {{"code", -32601}, {"message", "method not found: " + method}}}});
      continue;
    }
    write({{"jsonrpc", "2.0"}, {"id", id}, {"result", result}});
  }
  return 0;
}

}  // namespace ogle
