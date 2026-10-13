#pragma once

// The editor application: window, main loop, viewport interaction and interface. Split over
// app.cpp (loop, camera, viewport), actions.cpp (document operations: levels, projects, clipboard,
// prefabs), panels.cpp (menus, toolbar, panels, windows) and extract.cpp (extraction of the
// game's assets with ogle-extract).

#include <atomic>
#include <deque>
#include <functional>
#include <future>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <string>
#include <unordered_map>
#include <vector>

#include "core/history.h"
#include "io/asset_library.h"
#include "io/asset_store.h"
#include "io/catalog.h"
#include "io/game_data.h"
#include "io/game_export.h"
#include "io/library_index.h"
#include "io/prefab_recipes.h"
#include "io/prefabs.h"
#include "game_view/game_view.h"
#include "render/camera.h"
#include "render/renderer.h"
#include "render/texture_palette.h"
#include "render/tool_demo.h"
#include "render/thumbnails.h"
#include "scene/gaps.h"
#include "scene/pick.h"
#include "scene/scene.h"
#include "scene/shapes.h"
#include "scene/level_style.h"
#include "scene/skeleton.h"
#include "scene/terrain.h"
#include "scene/topology.h"
#include "scene/story.h"
#include "app/remote.h"
#include "tools/gizmo.h"
#include "tools/navmesh.h"

struct SDL_Window;
struct SDL_GLContextState;
struct SDL_Process;

namespace ogle {

enum class ToolMode {
  Select = 0,  // select, move, rotate
  Place,       // put the chosen prefab, object or decor part where the cursor points
  Topology,    // the points and faces of one element's mesh (topology_tool.cpp)
  Terrain,     // the terrain brush: ground painted, raised, dug, walls (terrain_tool.cpp)
};

enum class DialogAction {
  OpenProject,
  ExtractSourceIso,
  ExtractSourceFolder,
  ExtractOutput,
  ExportNavMeshes,
  ExportFolder,
  ImportModel,   // a .glb made elsewhere (Blender...), placed like a model
  TextureImage   // an image for the material being edited (m_texture_target)
};

// What the Place tool puts in the level: a prefab, an object of the catalog or a decor part of a
// level.
struct Placement {
  enum class Kind { Prefab, Object, Model, Part, Shape };
  Kind kind = Kind::Prefab;
  std::string path;  // the prefab file, the model file (object, model), the part's level background
  std::string name;
  CatalogObject object;  // Object
  CatalogPart part;      // Part
  std::string game;      // Part, Model: "jak2" for Jak II's (io/jak2_import.h), else Jak 3's
  std::string level;     // Model: the level it comes from
  ShapeParams shape;     // Shape
  int material = -1;     // Shape: its material in the document (a level's style), -1 the grey grid
  float yaw = 0.f;    // radians (R, Ctrl + wheel)
  Quat tilt;          // the turn the right button gives it, in the world (applied after the yaw)
  bool once = false;  // dragged from a panel: one placement, then back to selecting
  std::string key() const;  // of its preview and of the meshes merged into the document
};

// The extraction window (extract.cpp): ogle-extract, next to the editor, reads the user's disc of
// Jak 3 and writes its levels to <library>/jak3/.
struct ExtractState {
  bool show = false;
  std::string source;  // the user's .iso or disc folder
  std::string output;  // the library folder
  // what ogle-extract --detect said about `detected_source`
  std::string detected_source;
  std::string detected_game, detected_name, detected_version, detected_serial;
  std::string detect_error;
  // the running ogle-extract
  SDL_Process* process = nullptr;
  bool detecting = false;  // --detect, else an extraction
  std::string pending;     // output not yet cut in lines
  std::vector<std::string> log;
  std::string step;        // last OGLE_STEP, translated
  size_t done = 0, total = 0;
  std::string current;     // last level written
  std::string error;       // OGLE_ERROR, or why it stopped
  bool finished = false;
  double started = 0, ended = 0;
};

// Nodes copied with Copy, pasted with Paste: the copied roots with their descendants.
struct Clipboard {
  std::vector<Node> nodes;
  std::vector<uint32_t> roots;
  std::map<uint32_t, Mat4> root_worlds;  // world matrix of each root when it was copied
  bool empty() const { return roots.empty(); }
};

struct AppOptions {
  std::string open_path;  // a project to open
  std::string library;    // the library folder for this session
  std::string language;   // the interface language for this session
  // --camera x,y,z,yaw,pitch (degrees): where captures look from instead of framing the scene
  bool camera_set = false;
  Vec3 camera_pos;
  float camera_yaw = 0, camera_pitch = 0;
  // --story off|<step>|<node>: the story view of captures (every state, a step, just after a node)
  std::string story;
  // --select a,b,...: the elements captures select (by name) instead of choosing one
  std::string select;
  // --add a,b,...: the levels captures add to the one loaded (its neighbors)
  std::string add;
  // --game <hour>: captures show the game view (the game's renderer) at that hour of the day
  float game_hour = -1;
  int remote_port = kRemotePort;  // the MCP server's connection (0: none)
  bool hidden = false;            // --hidden: no window shown (tests)
};

// ---- document operations usable without a window (tests) ----
bool open_project(Scene& scene, const std::string& path, std::string* error);
bool save_project(const Scene& scene, const std::string& path, std::string* error);
// A level of the library: its decor, its actors with the model the library has for each, and its
// collision (kept, not shown). The terrain is locked: clicks go to the objects on it. With the
// game's data (may be null): without the creatures and logic actors, the water actors with the
// model of their look, and the levels the game shows with it (companions, the layers the story
// lends it: see scene/story.h).
// Without `actors`, the level comes without any actor (its objects too): its decor alone.
bool open_level(Scene& scene, const AssetLibrary& library, const GameData* data, const LevelEntry& level,
                std::string* error, bool actors = true);
// Adds a level of the library to a document, at its place in the game's world (the levels' files
// share the game's coordinates, so levels side by side in the game meet here too): under a group
// (extras.og_level), with the levels the game shows with it and the layers of its missions under
// that group. Returns the group, 0 when the level cannot be read.
uint32_t add_level(Scene& scene, const AssetLibrary& library, const GameData* data, const LevelEntry& level,
                   std::string* error, bool actors = true);
// Whether the Levels panel lists a level: not a layer of the story (the actors a mission adds to
// another level), not actors alone (no collision: cutscenes...), not a copy of a level found in
// another DGO.
bool level_listed(const LevelEntry& level, const GameData* data);
// Where the land of other levels hides the ocean of `map` in the game (its water at `ocean_y`): per
// square of 3 m of the map (rows along z, map.size * 32 squares a side), 1 under a surface of a
// render piece of `land` lying above the water (walls left out).
std::vector<uint8_t> ocean_land(const OceanMap& map, float ocean_y, const Scene& land);
// The squares of the map with water (its mid ocean's cells, its near ocean's squares), as above.
std::vector<uint8_t> ocean_water(const OceanMap& map);
// The land hiding the ocean around the document's levels, from such masks (side x side): `doc` its
// levels', `around` the levels the game shows with them, `far` views of the map from afar (the one
// placed like the document's levels, if one is); with `water` (ocean_water), the water the land
// closes in too. Never in the box of the document's own land.
std::vector<uint8_t> ocean_land_around(int side, const std::vector<std::vector<uint8_t>>& doc,
                                       const std::vector<std::vector<uint8_t>>& around,
                                       const std::vector<std::vector<uint8_t>>& far,
                                       const std::vector<uint8_t>& water = {});
Clipboard copy_nodes(const Scene& scene, const std::vector<uint32_t>& roots);
// Adds copies of the clipboard's nodes (under their old parent when it is still there, at the
// same place). Returns the new roots.
std::vector<uint32_t> paste_nodes(Scene& scene, const Clipboard& clip);
// Groups (groups.cpp): the group an element is the root of (extras og_group), 0 none; a group given
// to it (0: none); a group id the document has not given yet
uint32_t group_tag(const Node& n);
void set_group_tag(Node& n, uint32_t id, const std::string& name, bool prefab);
uint32_t new_group_id(const Scene& scene);

class App {
 public:
  int run(const AppOptions& opts);
  // Runs the whole editor (interface included) in a hidden window and saves a capture once what
  // it shows is loaded. `setup`: see ui_screenshot() in app.cpp.
  int ui_screenshot(const AppOptions& opts, const std::string& in, const std::string& out, int w,
                    int h, const std::string& setup);

 private:
  // setup
  bool init_window(bool hidden, int w, int h);
  void shutdown_window();
  void apply_options(const AppOptions& opts);
  void new_document();
  void frame_all();
  void frame_selection();
  void set_title();

  // loop
  void frame(float dt);
  void process_event(const void* sdl_event);
  void update_camera(float dt);
  // The right button held on the selection (or on what is being placed) turns it on every axis with
  // the mouse: across around the vertical, up and down around the camera's horizontal (Shift finer,
  // the snap's steps of turn with the snap); elsewhere it looks around (update_camera).
  bool start_free_rotation();
  void update_free_rotation(const Vec2& rel);
  void finish_free_rotation();
  bool m_free_rotating = false;
  bool m_free_rotating_placement = false;  // what is being placed, not the selection
  Vec3 m_free_pivot, m_free_right;
  float m_free_yaw = 0, m_free_pitch = 0;
  Quat m_free_tilt0;
  void viewport_interaction();
  void select_tool(bool clicked, bool released);
  // the mouse's drag (pixels) as a move in the world at `at`: left and right along the view's
  // horizontal, up and down vertical (Ctrl: forward and back), with the snap's steps
  Vec3 drag_offset(Vec2 px, const Vec3& at) const;
  std::vector<uint32_t> box_nodes(Vec2 a, Vec2 b) const;  // what a box of the view selects
  // what the renderer outlines: the selection with what is under it, what the cursor points at
  void update_outlines();
  void outline_subtree(uint32_t id, std::vector<uint32_t>& out, size_t cap) const;
  void draw_cursor_info();  // next to the cursor: what it points at, its group, what a click does
  // what a click on `node` selects: the actor of an actor's model, nothing on the locked ground
  // (unless select_terrain)
  uint32_t selectable(uint32_t node) const;
  void place_tool(bool clicked);
  void draw_tool_overlays();
  void draw_navmesh_overlay();
  void handle_shortcuts();
  void poll_level_opening();

  // nav meshes (actions.cpp): generated on a worker thread from the walkable surfaces of `region`,
  // replacing the nav meshes there; applied (undoable) when ready. With points, only the pieces of
  // ground next to one of them are kept.
  bool start_navmesh_generation(const AABB& region, std::string* error, std::vector<Vec3> keep_from = {});
  void poll_navmesh_generation();
  bool navmesh_busy() const { return m_nav_future.valid(); }
  std::vector<uint32_t> navmesh_nodes() const;  // the document's nav meshes
  AABB navmesh_region_of_selection() const;     // around the selection, invalid without one
  std::vector<Vec3> navmesh_points_of_selection() const;  // where the selected elements stand
  // the nav meshes for the game (nav_export.h), ids from `first_aid`
  bool export_navmeshes(const std::string& path, uint32_t first_aid, std::string* error);
  // the document's nav meshes where they are now, each around the center of its corners
  void world_navmeshes(std::vector<NavMeshData>* meshes, std::vector<std::string>* names) const;

  // Level > Export for the game (actions.cpp, io/game_export.h): what the view shows, as a custom
  // level of OpenGOAL for Jak 3
  bool export_for_game(const LevelExportOptions& opts, LevelExportReport* report, std::string* error);
  // the export's options for the document (names, folder, where the player starts)
  LevelExportOptions default_export_options();
  // `nickname`, or one like it that no level of the game has
  std::string untaken_nickname(const std::string& nickname);
  // where the player starts: a continue point of the level opened (moved with it), else the ground
  // at the middle of the document
  Vec3 export_start_point();
  void open_export_window();
  bool show_export_window = false;
  LevelExportOptions export_options;  // the window's

  // several levels in a document (actions.cpp): the level opened, the levels added (neighbors),
  // the companions and layers of each
  struct DocumentLevel {
    enum Role { Main, Added, Companion, Layer };
    std::string name;
    Role role = Main;
    uint32_t group = 0;  // its group; 0 for the level opened
    uint32_t host = 0;   // companions and layers: the group of the level they come with, 0 the one opened
    bool reference = false;  // Added: shown to compare with (the game's level a new one replaces)
  };
  std::vector<DocumentLevel> document_levels() const;
  bool level_in_document(const std::string& name) const;
  // a level of the library added on a worker thread, then to the document (undoable) when ready;
  // `reference`: shown to compare with, washed in blue, not selected nor exported (the level the
  // document replaces, as the game has it)
  bool add_level_to_document(const LevelEntry& level, std::string* error, bool reference = false, bool jak2 = false);
  bool m_levels_jak2 = false;  // the Levels panel lists Jak II's
  bool m_adding_reference = false;
  // replacing a level of the game (scene.settings.replaces): the document's own level moved so that
  // its middle and its bottom are where the replaced level's are (the reference shown, else its
  // continue points); turned by a quarter turn around its middle
  bool align_to_replaced(std::string* error);
  void turn_own_level(float degrees);
  std::vector<uint32_t> own_level_roots() const;  // the root elements of the document's own level
  AABB own_level_bounds() const;
  // levels are opened and added with their actors (the objects: the creatures never come), or
  // with their decor alone (Levels panel)
  bool load_actors = false;
  void poll_level_adding();
  bool level_adding() const { return m_add_future.valid(); }
  bool remove_document_level(uint32_t group, std::string* error);
  AABB document_level_bounds(uint32_t group) const;  // 0: the level opened (collision left out)
  // a whole level, to move it: an added level's group, or the root elements of the level opened
  // (with its companions and layers, without the levels added nor the nav meshes)
  std::vector<uint32_t> document_level_roots(uint32_t group) const;
  uint32_t level_of_node(uint32_t id) const;  // the added level's group it is in, 0 for the one opened
  // The game's lighting (the 8 palettes of each vertex) of the levels' decor that misses it (a
  // document made before the extraction kept it): from the levels as extracted, piece by piece
  // where its mesh is the same. How many pieces got it back.
  size_t restore_level_lighting();
  void select_level(uint32_t group);
  // the assembly the element is part of, all of it (scene/assemblies.h): a house with its facades,
  // roof and windows
  void select_assembly(uint32_t id);
  // Groups of the document (groups.cpp): a prefab placed, a selection grouped and named
  struct GroupInfo {
    uint32_t id = 0;
    std::string name;
    bool prefab = false;           // a prefab placed (named after it)
    std::vector<uint32_t> roots;   // its elements (with what is under them)
  };
  const std::vector<GroupInfo>& document_groups() const;  // by id, found again when the document changes
  const GroupInfo* group_info(uint32_t gid) const;
  uint32_t group_of(uint32_t id) const;  // the group of an element or of one above it, 0 none
  void select_group(uint32_t gid);
  // the whole group of an element (double click, the button held still), else its assembly
  bool select_whole(uint32_t id);
  uint32_t group_selection(const std::string& name);  // undoable; the new group
  void ungroup(uint32_t gid);
  void rename_group(uint32_t gid, const std::string& name);
  void ui_group_section();     // the inspector's: the selection's group
  void ui_groups_panel();      // the Groups tab
  void ui_new_group_window();  // naming a new group
  mutable std::vector<GroupInfo> m_groups;
  mutable uint64_t m_groups_revision = ~0ull;
  mutable size_t m_groups_nodes = 0;
  bool m_show_new_group = false, m_group_focus = false;
  std::string m_group_name, m_groups_filter;
  uint32_t m_renaming_group = 0;
  // roughly: within the camera's field of view and not past its far plane (a cone, not the exact
  // frustum: good enough to decide whether a point needs framing to be seen)
  bool camera_sees(const Vec3& p) const;
  // where two levels of the document meet: the overlap of their boxes (a few meters around the
  // seam), invalid when they do not touch
  AABB document_junction(uint32_t group_a, uint32_t group_b) const;
  // the levels the game displays with those of the document (its data), in the library and not
  // in the document yet
  std::vector<std::string> document_neighbors();

  // document (actions.cpp)
  void set_library(const std::string& library_root);
  // the game's data, read once; null when missing
  std::shared_ptr<const GameData> game_data();
  const GameData* document_game_data();  // null without a document
  // the story view: after a new document, after the view or the document changed
  void story_document_changed();
  void set_story_view(const StoryView& view);
  void apply_story_view();
  void update_ocean();
  void set_ocean_backdrop();  // the far ocean in the render settings, before the viewport is drawn
  // jak2: a level of Jak II (library_jak2), its decor made Jak 3's (no actors)
  void load_level(const LevelEntry& level, bool jak2 = false);
  void open_project_file(const std::string& path);
  // Saves the project, only when the user asks: in its folder of <library>/projects/<name>/ (the
  // first time and with save_as, the user names it), its .glb and its nav meshes for the game
  // (<name>-nav.json) when it has some
  void save(bool save_as);
  bool save_project_as(const std::string& name, std::string* error);
  bool save_project_files(const std::string& glb_path, std::string* error);
  std::string projects_root() const;                           // <library>/projects
  std::string project_name() const;                            // its folder's name, or the level's
  static std::string project_folder_name(const std::string& name);  // a name as a folder can have it
  bool m_show_save_as = false;
  std::string m_save_name;
  void select_only(uint32_t id);
  void toggle_select(uint32_t id);
  std::vector<uint32_t> selection_roots() const;  // selected nodes without selected ancestors
  Mat4 selection_frame() const;                   // gizmo pivot and orientation (once per frame)
  // a move going on (the gizmo's, whatever its axis or plane, or the selection dragged): how far the
  // selection went, false when none; where its pivot was, drawn with the selection there
  bool move_delta(Vec3* d) const;
  Vec3 m_drag_pivot0{0, 0, 0};
  void draw_move_preview();
  mutable uint64_t m_selection_frame_key = ~0ull;
  mutable std::set<uint32_t> m_selection_frame_sel;
  mutable uint32_t m_selection_frame_primary = 0;
  mutable Mat4 m_selection_frame;
  // the world box of a node and its descendants (from the scene's cache), how many descendants
  AABB subtree_bounds(uint32_t id, size_t* descendants = nullptr) const;
  size_t subtree_size(uint32_t id) const;  // its descendants
  AABB selection_bounds() const;  // of the selection and what it holds
  // the middle of most of a large group's geometry (a whole level), in the world; kept in the
  // group's space until the document changes
  Vec3 group_center(uint32_t id) const;
  mutable std::unordered_map<uint32_t, std::pair<uint64_t, Vec3>> m_group_centers;
  PickHit pick_surface(const Vec2& mouse_vp);     // what is under a viewport point
  void delete_selection();
  // The particle effects of the document's levels as the game has them (steam, sparks, lights) added
  // back where the document has none (a project made before the editor kept them, effects deleted);
  // not those whose decor was taken out of the level. Undoable. Returns how many were added, and
  // in `left_out` how many were not.
  size_t restore_level_effects(size_t* left_out);
  void copy_selection();
  void paste();
  void duplicate_selection();
  // the selection saved as a prefab: its file ("" when it could not be); without its elements
  // smaller than min_size meters (their names to left_out)
  std::string create_prefab(const std::string& name, float min_size = 0.f, std::vector<std::string>* left_out = nullptr);
  // Building without visual bugs (building_tools.cpp): elements of `roots` smaller than `min_size`
  // left out (an actor stays); the holes through which the player would see the void
  // (scene/gaps.h), shown over the view and listed in a window
  std::vector<uint32_t> without_small(const std::vector<uint32_t>& roots, float min_size,
                                      std::vector<std::string>* left_out = nullptr) const;
  float prefab_min_size = 0.5f;   // the prefab dialog's: small pieces left out
  bool prefab_skip_small = true;
  AABB gaps_region() const;       // the selection's, else the document's own level
  void run_find_gaps(const AABB& region);
  void draw_gaps_overlay();
  void ui_gaps_window();
  bool show_gaps_window = false, m_gaps_done = false;
  GapReport m_gaps;
  int m_gap_selected = -1;
  void start_placement(const std::string& prefab_path, const std::string& name, bool once);
  void start_object_placement(const CatalogObject& object, bool once);
  void start_model_placement(const CatalogModel& model, bool once);
  void start_part_placement(const std::string& level_background, const CatalogPart& part, bool once);
  void start_shape_placement(const ShapePreset& preset, bool once);
  void stop_placement();
  Mat4 placement_matrix(const PickHit& hit, const Ray& ray) const;
  // the mesh of what is placed, merged into the document once (null while it loads)
  std::shared_ptr<Mesh> placement_mesh();
  // puts what is placed at `m`, lit like the level around it (scene/relight.h); returns the new roots
  std::vector<uint32_t> place_at(const Mat4& m);
  // the elements under `roots` lit where they stood (placed, pasted), lit again where they are now,
  // within `change` (saved before they change)
  void relight_moved(NodeChange& change, const std::vector<uint32_t>& roots);
  // the inspector's Light like here: the selection lit like the level around it (undoable)
  void relight_selection();
  void new_level();  // an empty document of the library's game
  // a level or a new one is being edited (a new level has no node yet)
  bool document_open() const { return !scene.nodes.empty() || !scene.settings.level.empty(); }
  const Scene* prefab_scene(const std::string& path);  // read once, nullptr when unreadable
  void refresh_prefabs();
  std::string prefab_folder() const;
  void open_dialog(DialogAction action);
  void process_dialog_results();
  void request_quit();
  // runs `action` now, or after the user agreed to lose the unsaved changes
  void confirm_discard(std::function<void()> action);
  void set_language(const std::string& code);

  // remote control by the MCP server (remote_tools.cpp)
  using RemoteReply = std::function<void(json answer)>;
  void remote_start(int port);
  void remote_poll();  // each frame: the requests received, the ones waiting for something
  void remote_dispatch(const std::string& tool, const json& args, RemoteReply reply);
  void remote_place(const json& args, RemoteReply reply);
  void remote_place_many(const json& items, size_t next, json placed, RemoteReply reply);
  json remote_status();
  json remote_node_json(const Node& n);
  json remote_level_json(const std::string& name);  // what get_level_info says of a level
  bool remote_unsaved(const json& args) const;
  // the 3D view from the camera, PNG in base64; with game_hour >= 0, the game view at that hour
  std::string capture_view_png(int w, int h, float game_hour = -1.f);
  // ui_input (tests of the interface): mouse events given to SDL one frame after the other, as the
  // system gives them, and pictures of the whole window
  std::deque<json> m_ui_events;
  RemoteReply m_ui_reply;
  Vec2 m_ui_mouse{0, 0};
  std::string m_ui_text;  // the text of the last typing event
  int m_ui_wait = 0;
  bool m_ui_shot_pending = false;
  std::string m_ui_shot;  // the last picture, PNG in base64
  void ui_input_step();

  // extraction (extract.cpp)
  static std::string extractor_path();  // ogle-extract next to the editor, "" when missing
  void open_extract_window();
  void extract_detect(const std::string& source);
  void start_extraction();
  bool run_extractor(const std::vector<std::string>& args, bool detecting);
  void extract_poll();  // each frame: reads ogle-extract's output
  void extract_stop();  // kills a running ogle-extract
  void extract_line(const std::string& line);
  void ui_extract_window();
  void ui_export_window();
  std::optional<LevelExportReport> m_export_report;  // the window's last export
  std::string m_export_error;
  std::string m_export_level;  // the level export_options were made for
  // the levels as extracted, read for the export (which pieces of their decor were taken out)
  std::map<std::string, std::shared_ptr<const Scene>> m_export_originals;

  // interface (panels.cpp)
  void draw_ui();
  void ui_menu();
  void ui_toolbar(float y, float h);
  void ui_left_panel(float x, float y, float w, float h);
  void ui_levels_panel();
  void ui_catalog_panel();
  void ui_catalog_objects(int tab);  // the Objects (0), the Interactive ones (3), the Effects (5)
  void ui_catalog_models();
  void ui_catalog_decor();
  void ui_catalog_decor_all();  // the decor of every level, searched by name
  void ui_catalog_materials();  // the textures of the game's levels, for the selection
  // a decor part in a grid of thumbnails: its thumbnail, its tooltip, a click or drag places it
  void ui_decor_cell(const std::string& background, int64_t stamp, const CatalogPart& part, const std::string& from,
                     int levels, float size);
  void ui_prefabs_panel();
  void ui_inspector(float x, float y, float w, float h);
  void ui_node_inspector(Node& n);
  void ui_replace_section(const std::vector<DocumentLevel>& levels);
  // an actor's behavior and parameters (behavior_panel.cpp); a condition's widgets (true: changed)
  void ui_actor_behavior(Node& n);
  void ui_actor_params(Node& n);
  bool ui_condition(const char* id, json& value, bool with_always);
  // a shape's size and a surface's materials (surface_panel.cpp)
  void ui_shape_inspector(Node& n);
  void ui_material_inspector(Node& n);
  void ui_import_inspector(Node& n);
  // The topology tool (topology_tool.cpp, scene/topology.h): one element's points or faces
  uint32_t m_topo_node = 0;
  bool m_topo_faces = false;              // picks faces, else points
  std::set<uint32_t> m_topo_points;       // into m_topo_cache
  std::set<MeshRef> m_topo_faces_sel;
  MeshPoints m_topo_cache;
  const Mesh* m_topo_cache_mesh = nullptr;
  uint64_t m_topo_cache_version = 0;
  std::unique_ptr<Node> m_topo_before;    // a gizmo drag going on: the node before it
  std::vector<std::pair<MeshRef, Vec3>> m_topo_start;  // its vertices where they were (world)
  Vec3 m_topo_pivot{0, 0, 0};
  Mat4 m_topo_axes;
  uint64_t m_topo_revision = 0;
  float m_topo_extrude = 1.f;
  bool topology_editable(uint32_t id) const;
  bool start_topology();
  void stop_topology();
  const MeshPoints& topology_points();
  std::set<MeshRef> topology_vertices();
  void topology_edit(const char* undo_key, const std::function<void(Mesh&)>& edit);
  void topology_tool(bool clicked, bool released);
  void draw_topology_overlay();
  void ui_topology_inspector();
  void topology_delete();
  // The terrain brush (terrain_tool.cpp, scene/terrain.h)
  struct TerrainBrush {
    BrushMode mode = BrushMode::Paint;
    float size = 4.f;         // meters: the square's side
    float strength = 3.f;     // raise, dig: meters per second at its middle; smooth, flatten: how fast
    float wall_height = 4.f;  // meters above the ground
    float cell = 1.f;         // a new terrain's cells (meters)
    bool relief = true;       // paint: the level style's bumps
    Vec2 gen_size{64, 64};    // generate: the rectangle (meters)
    float hills = 12.f, roughness = 0.5f;
    int seed = 1;
    // a texture of the level style's level chosen for the ground, the walls ("": the style's)
    std::string floor_texture, wall_texture;
  } brush;
  uint32_t m_terrain_node = 0;                   // the terrain painted (0: a new one at the first stroke)
  std::unique_ptr<NodeChange> m_terrain_change;  // a stroke going on
  TerrainGrid m_terrain_grid;                    // its grid while it goes on
  bool m_terrain_new = false, m_terrain_changed = false;
  // a stroke laying ground, walls or erasing: shown see-through while the button is held (its grid
  // before, the cells it reached), done when it is let go
  bool m_stroke_preview = false;
  TerrainGrid m_stroke_before;
  int m_stroke_cells[4] = {0, 0, -1, -1};  // i0, k0, i1, k1
  float m_stroke_height = 0.f;  // the ground's height where the stroke started (its node's space)
  int m_stroke_look = 0;        // the look its cells take (in the grid's looks)
  bool m_terrain_rmb = false;        // the right button held over the view, the mouse not moved far yet
  bool m_terrain_sculpting = false;  // ... then moved up or down: the ground raised or dug, shown until released
  Vec2 m_terrain_rmb_drag{0, 0};     // pixels moved since it was pressed (sideways: it looks around instead)
  float m_sculpt_px = 0.f;           // sculpting: pixels the mouse went up (down: negative)
  float m_brush_plane = 0.f;    // where the brush is when the ray meets nothing
  Vec3 m_brush_point{0, 0, 0};
  bool m_brush_valid = false;
  uint32_t m_brush_hit_node = 0;  // what the brush is over (0: the plane)
  Vec3 m_last_stamp{0, 0, 0};
  bool m_has_stamp = false;
  TerrainGrid m_terrain_grid_view;  // the element's grid, for the square drawn over it
  const Mesh* m_terrain_view_mesh = nullptr;
  bool start_terrain_brush();
  void stop_terrain_brush();
  void terrain_end_stroke();
  void terrain_cancel_stroke();  // a stroke shown, not let go: nothing done
  bool terrain_begin_stroke(BrushMode mode);
  // where a stroke starting now starts: the ground's height under the brush (the terrain's space)
  float terrain_start_height(const TerrainGrid& g, const Node* node) const;
  // a stamp of the brush as it is set, at `center` (the terrain's space)
  BrushStroke terrain_stamp(BrushMode mode, Vec3 center, float amount, float height);
  void terrain_apply(BrushMode mode, Vec3 from, Vec3 to, float amount);
  float terrain_sculpt_height() const;  // meters the right button held raises (digs: negative)
  void terrain_tool(bool clicked, bool released);
  // the right button held with the brush: up raises, down digs (sideways looks around); true while
  // it has the mouse
  bool terrain_right_button(Vec2 rel);
  void draw_terrain_overlay();
  // what a stamp would change, see-through, before it is done
  void draw_terrain_preview(const TerrainGrid& before, const BrushStroke& stamp, const Mat4& world);
  // the cells of [i0, i1] x [k0, k1] changed from `before` to `after`, see-through
  void draw_terrain_ghost(const TerrainGrid& before, const TerrainGrid& after, int i0, int k0, int i1, int k1,
                          BrushMode mode, const Mat4& world);
  void ui_terrain_window();
  void terrain_generate(Vec3 at);
  // a terrain's materials and texture sizes: the level style chosen, else the editor's grid
  TerrainGrid::Look terrain_look();
  // a level's style, measured once its decor is read (nullptr meanwhile, or when it cannot be)
  const LevelStyle* level_style(const std::string& level);
  // the textures a level's decor draws, the most used first (the brush's choices)
  const std::vector<PaletteTexture>& level_textures(const std::string& level);
  std::string m_level_textures_of;
  std::vector<PaletteTexture> m_level_textures;
  std::string m_terrain_tex_filter;
  // a material of the document drawing a texture of a level (like `like`, -1: the editor's)
  int material_with_texture(const std::string& level, const std::string& texture, int like);
  // the brush's choice of a texture among the level's: true when it changed
  bool ui_terrain_texture(const char* key, std::string* texture, const std::string& style_texture);
  // a level style's floor (or wall) material, added to the document once; -1: none
  int style_material(const std::string& level, bool floor);
  // Build panel: floors and walls in the style of a level (scene/level_style.h)
  std::string m_style_level;
  std::string m_style_filter;
  std::map<std::string, LevelStyle> m_styles;  // measured, by level
  void ui_level_style();
  // a shape (`shape`: a preset's name; "": a ground or a wall) with a level's floors' or walls' look
  void start_style_placement(const std::string& level, bool floor, const std::string& shape = "");
  // The preview of an actor's animation or effect (the inspector's): its model drawn moving in place
  // of its own mesh until stopped, the document left as it is (scene/skeleton.h)
  struct AnimPreview {
    uint32_t node = 0;  // the model node drawn moving, 0: none
    std::shared_ptr<Mesh> original, moving, bind;
    std::shared_ptr<Skeleton> skeleton;
    // an animation of its model (>= 0), else -1 broken (its -explode model), -2 the movement its
    // code gives it (motion), -3 its own model cut in pieces
    int animation = -1;
    int motion = 0;
    Vec3 base{0, 0, 0};  // the movement: around the middle of its base, its height
    float height = 0;
    float time = 0;
    std::vector<Mat4> rest;  // broken: where its pieces were, how they fly
    std::vector<Vec3> velocity, spin;
    uint64_t revision = 0;  // a change of the document ends it
    bool icon = false;      // an actor without a model: `moving` is its picture, drawn over the view
    int seed = 0;           // its lightning's zigzags, drawn again and again (they crackle)
  };
  AnimPreview m_preview;
  bool m_preview_loop = true;
  int m_preview_choice = 0;
  std::map<std::string, std::shared_ptr<Skeleton>> m_skeletons;  // model file -> its skeleton (null: none)
  const Skeleton* model_skeleton(const std::string& path);
  // What an element (an actor, or a model placed from the catalog: Jak 3's Models, Jak II's
  // Objects) can show in the preview
  struct PreviewOptions {
    const Node* model = nullptr;          // the node drawn moving (an actor's model, the model itself)
    std::string name;                     // its model's
    const ModelAsset* asset = nullptr;    // its file
    const ModelAsset* broken = nullptr;   // the model of its pieces (<model>-explode)
    const Skeleton* skeleton = nullptr;
    std::vector<int> animations;          // the ones that move
    bool fracture = false;                // broken without pieces of its own: its model cut up
    int motion = 0;                       // the movement its code gives it (preview.cpp Motion)
    bool icon = false;                    // an actor without a model: its picture doing what it does
    bool jak2 = false;
    bool any() const { return !animations.empty() || broken || fracture || motion || icon; }
    json to_json() const;
  };
  PreviewOptions preview_options(const Node& n);
  void ui_preview_inspector(const Node& n);
  void draw_preview_overlay();
  // the picture of an object of the catalog: its model, its particles, or what it does
  ThumbRequest catalog_thumb(const CatalogObject& o) const;
  bool start_preview(uint32_t model_node, const std::string& path, int animation, int motion, bool jak2 = false);
  bool start_icon_preview(uint32_t actor, int motion);
  void update_preview(float dt);
  void stop_preview();  // a structure of Jak II: what was checked when it came in
  // what the game does with an element of the decor: its collision, bounce, launch (io/decor_behaviors.h)
  void ui_decor_behavior(Node& n);
  std::unique_ptr<NodeChange> m_decor_edit;  // a value being dragged: the elements before
  // the effects of the document: (node index, group), made again when the scene cache changes
  std::vector<std::pair<uint32_t, const SpGroup*>> m_effect_nodes;
  uint64_t m_effect_cache = ~0ull;
  std::vector<ParticleSprite> m_sprites;
  struct TextureTarget {
    std::vector<uint32_t> ids;
    int material = -1;
  } m_texture_target;  // what the image dialog's texture goes to
  void ui_statusbar(float y, float h);
  void ui_windows();
  void ui_navmesh_window();
  void ui_navmesh_inspector(Node& n);
  void ui_labels();
  void ui_viewport_hint();
  void ui_story_bar();
  void edit_begin(const Node& n);
  void edit_end(const std::string& label);
  void commit_node_edit(const Node& before, const std::string& label);

 public:
  // state shared with the interface
  Scene scene;
  History history;
  Renderer renderer;
  // the level drawn by the game's own renderer (View > Game rendering, G), when OpenGL allows it: on
  // from the start
  GameView game_view;
  bool game_view_on = true;
  std::string game_view_error;  // why there is none
  // the particle effects of the game (data/jak3/particles.json): the level's effects run in the
  // view (View > Effects) and in the catalog's pictures
  ParticleData particle_data;
  ParticleSystem particles;
  bool show_effects = true;
  void update_effects(float dt);
  void draw_effects();
  // The Game rendering window (game_window.cpp): the sky, the hour and the weather of the game view
  bool show_game_window = false;
  bool time_passes = false;   // the hour goes on by itself
  float time_speed = 20.f;    // minutes of the game per second
  bool lightning = false;     // a storm's flashes
  float m_lightning_wait = 3.f, m_lightning_time = 0.f;
  int m_lightning_frame = -1;
  void ui_game_window();
  int weather_preset_index() const;          // the weather chosen, -1 one of the user's own
  bool set_weather(const std::string& id);   // a weather of weather_presets() by its id
  void update_weather(float dt);             // the hour passing, the lightning
  void draw_rain();                          // the rain falling around the camera
  // the effects lost in the game's fog of the hour and the weather, far away
  void fog_effects(std::vector<ParticleSprite>& sprites) const;
  // what the editor draws over the game view: no grid, the models lit and fogged like the game's
  void game_view_settings(RenderSettings& out);
  Camera camera;
  RenderSettings rs;
  Gizmo gizmo;
  ToolMode tool = ToolMode::Select;
  std::set<uint32_t> selection;
  uint32_t primary = 0;
  uint32_t hovered = 0;
  std::string project_path;
  uint64_t saved_revision = 0;

  // the library: the extracted game, its levels, loading and thumbnails
  std::string library_root;
  AssetLibrary library;  // empty when the game was not extracted
  AssetStore store;
  Thumbnails thumbs{store};
  std::vector<PrefabInfo> prefabs;
  // the game's prefabs (io/prefab_recipes.h): made from the extracted levels into <prefabs>/game/,
  // on a worker thread, the first time the Prefabs panel shows them (or `rebuild`)
  std::vector<PrefabRecipe> prefab_recipes;
  std::vector<PrefabInfo> game_prefabs;
  // makes the game's prefabs not made yet (all of them again: rebuild), or only those of `levels`
  // (game, level)
  void start_game_prefabs(bool rebuild = false, const std::set<std::pair<std::string, std::string>>& levels = {});
  // the prefabs of that level are wanted (shown in the panel, placed): made as soon as possible
  void want_game_prefabs(const std::string& game, const std::string& level);
  std::set<std::pair<std::string, std::string>> m_prefab_levels_wanted;
  std::string m_prefab_place_when_made;  // clicked before it was made: placed once it is
  void poll_game_prefabs();
  bool game_prefabs_busy() const { return m_game_prefab_work.valid(); }
  std::future<size_t> m_game_prefab_work;
  std::shared_ptr<std::atomic<int>> m_game_prefab_done = std::make_shared<std::atomic<int>>(0);
  int m_game_prefab_total = 0;
  uint64_t m_game_prefab_runs = 0;  // counts the builds finished (the remote waits on it)
  std::vector<CatalogObject> catalog;        // the objects of the game
  std::vector<CatalogModel> catalog_models;  // its other models (props)
  std::vector<CatalogModel> catalog_models_jak2;  // Jak II's models, placed as decor
  // what the catalog reads of every level on a worker thread: their decor, the art-names of their
  // actors (all the variants and effects); merged into the catalog once ready
  LibraryIndex library_index;
  std::vector<DecorEntry> all_parts;  // the decor of every level
  // Jak II's levels extracted next to Jak 3's (<library>/jak2): their structures, brought into Jak 3
  // levels (io/jak2_import.h); empty when Jak II was not extracted
  AssetLibrary library_jak2;
  LibraryIndex library_index_jak2;
  std::vector<DecorEntry> all_parts_jak2;
  bool m_index_jak2_applied = false;
  bool m_catalog_jak2 = false;  // the catalog shows Jak II's models, structures and materials
  // the palette of materials: the textures Jak 3's levels use again and again (render/texture_palette.h)
  TexturePalette palette;
  std::vector<PaletteTexture> palette_textures;
  // a texture of the palette made a texture of the document (-1 while its image is read; `wait`:
  // read now, for the remote control); Jak II's named "jak2-<name>"
  int palette_texture(const PaletteTexture& t, bool wait = false);
  // every texture of the games' levels (all_textures), once their index is ready
  std::vector<PaletteTexture> game_textures, game_textures_jak2;
  // The textures of the games: the game (Jak 3, Jak II when extracted), a level (or every level, or
  // the document's own textures), a part of a name; a click gives the texture to `ids` (their
  // material `material`, -1 all)
  void ui_texture_browser(const std::vector<uint32_t>& ids, int material, float width, float height,
                          bool choose_game = true);
  std::string m_tex_game, m_tex_level, m_tex_filter;  // its choices: "" Jak 3 / "jak2"; "" every level, "*" the document
  // the window of the inspector's Textures of the game... (for one material of the pieces selected:
  // its place in the first one's list, the material itself being a new one after each change)
  struct TextureWindow {
    bool open = false;
    std::vector<uint32_t> ids;
    int slot = 0;
  } m_texture_window;
  void ui_texture_window();
  void apply_library_index();  // the index, once it just became ready
  void rebuild_catalog();      // catalog, catalog_models and all_parts
  Placement placement;
  Clipboard clipboard;
  ExtractState extract;
  // the story: the point of the game the level is shown at, the steps where the document changes
  StoryView story;
  std::vector<int> story_steps;  // where the level opened changes
  // where each level added changes (its name), and the level the story bar shows ("" the opened)
  std::map<std::string, std::vector<int>> level_story_steps;
  std::string story_level;
  // the states of a level: 1, then each step where it changes ("" the level opened)
  std::vector<int> story_states(const std::string& level) const;

  // windows and panels
  bool show_log = false;
  bool show_help = false;  // the guide (F1): the tutorials, every tool, the keys
  int guide_tab_request = -1;  // 0 the tutorials, 1 every tool, 2 the keys
  int guide_tutorial = 0;      // the tutorial shown (0: Getting started)
  int guide_step = 0;          // its step
  bool select_tutorial(const std::string& id, int step = 0);  // by its id ("game_view"); false: none
  bool show_labels = true;
  bool show_navmesh = true;          // the nav meshes drawn over the level
  bool select_terrain = false;       // the ground (locked) can be clicked and box-selected
  bool show_navmesh_window = false;  // Level > Nav meshes
  NavParams nav_params;
  bool nav_keep_selected = false;    // keep only the ground the selected elements stand on
  uint32_t nav_first_aid = 60000;    // the ids the exported nav meshes take
  int left_tab_request = -1;  // select this tab of the left panel next frame (0 levels, 1 catalog, 2 prefabs, 3 build)
  // the short animations of the tools (guide, tooltips) and the Build panel's icons
  ToolDemo demos;
  // A tooltip of the guide on the last item: what it does, its keys, a concrete example and its
  // animation (the keys guide.<id>.title/.text/.example/.keys of the translations)
  void guide_tip(const char* id);
  // demo `name` at `t` seconds, w x h of the window's room, with its cursor and labels
  void ui_demo(const std::string& name, float w, float h, double t);
  void ui_guide_window();
  void ui_build_panel();
  bool m_scroll_to_replace = false;  // Level > Replace: the inspector shows its section

 private:
  SDL_Window* m_window = nullptr;
  SDL_GLContextState* m_gl = nullptr;
  bool m_running = true;
  bool m_quit_confirmed = false;
  bool m_hidden = false;
  // layout (logical pixels)
  float m_left_w = 340.f, m_right_w = 320.f;
  float m_vp_x = 0, m_vp_y = 0, m_vp_w = 1, m_vp_h = 1;
  bool m_mouse_in_vp = false;
  bool m_mouse_over_vp = false;  // over the viewport, even while ImGui drags something
  Vec2 m_mouse_vp;
  // camera interaction
  bool m_looking = false;
  bool m_orbiting = false;
  bool m_panning = false;
  Vec3 m_orbit_pivot;
  float m_orbit_dist = 10.f;
  Vec2 m_mouse_rel;  // relative motion accumulated from SDL events (mouse look)
  bool m_lmb_in_vp = false;
  std::string m_title;
  // select tool: the left button pressed (a click, a box, the selection dragged, held still)
  bool m_box_selecting = false;
  Vec2 m_box_start;
  double m_press_time = 0;
  uint32_t m_press_target = 0;        // the element under it
  bool m_press_on_selection = false;  // on an element of the selection: a drag moves the selection
  bool m_skip_release = false;        // a double click was taken: its release does nothing more
  bool m_long_pressed = false;        // held still: its whole group was selected
  float m_long_press_ring = 0.f;      // 0..1: how long it was held, drawn around the cursor
  bool m_moving_selection = false;
  Vec2 m_move_px{0, 0};
  Vec3 m_move_anchor{0, 0, 0};
  std::vector<uint32_t> m_box_preview;  // what the box would select
  Vec2 m_box_preview_end{-1, -1};
  double m_box_preview_time = 0;
  double m_hover_since = 0;  // the element under the cursor since then
  uint64_t m_outline_key = ~0ull;
  std::set<uint32_t> m_outline_sel;
  // the last click (a click again there goes to the next element under the cursor)
  Vec2 m_last_click{-1e9f, -1e9f};
  Vec3 m_last_click_cam;
  float m_last_click_yaw = 0, m_last_click_pitch = 0;
  std::unique_ptr<NodeChange> m_drag_change;
  std::map<uint32_t, Mat4> m_drag_start;
  std::map<uint32_t, Vec3> m_drag_scale;  // local scale of the roots when a scale drag began
  bool m_scale_uniform = true;             // the inspector's scale field keeps the proportions
  // property editing: the node when a field became active
  std::optional<Node> m_edit_before;
  // placement
  bool m_place_valid = false;
  Mat4 m_place_matrix;
  // the left button held where it is placed: the mouse raises it (up and down) and moves it along x
  // (left and right) from there, placed when the button is released
  bool m_place_drag = false;
  Mat4 m_place_anchor;
  Vec2 m_place_drag_px;
  bool m_drag_from_panel = false;  // an ImGui drag of a prefab is going on
  std::map<std::string, std::unique_ptr<Scene>> m_prefab_scenes;  // prefabs read, by path
  // prefab meshes merged into the document, per prefab file and mesh (shared by placements)
  std::map<std::string, std::map<const Mesh*, std::shared_ptr<Mesh>>> m_prefab_meshes;
  // preview meshes of the prefabs, merged into the document once (by prefab file)
  std::map<std::string, std::shared_ptr<Mesh>> m_prefab_previews;
  std::string m_prefab_dir_override;  // captures write their prefabs elsewhere
  bool m_show_new_prefab = false;
  std::string m_new_prefab_name;
  // levels panel
  std::string m_levels_filter;
  std::string m_selected_level;
  // catalog panel: its objects, or the decor of a level
  int m_catalog_tab = 0;  // 0 objects, 1 models, 2 decor, 3 the interactive objects, 4 materials, 5 effects
  std::string m_effect_kind;  // the Effects' category shown ("effect.smoke"), "" all
  std::string m_models_level;  // the level whose models are shown, "" for every level
  std::string m_catalog_filter;
  std::string m_catalog_family;  // "" for every family
  std::string m_decor_level;     // the level whose decor is shown, "" for every level
  std::shared_ptr<Scene> m_decor_scene;
  std::vector<CatalogPart> m_decor_parts;
  bool m_index_applied = false;  // the library's index is in the catalog
  size_t m_all_prototypes = 0, m_all_pieces = 0;
  std::string m_all_parts_filter = "";  // the filter m_all_parts_shown was made for
  std::vector<size_t> m_all_parts_shown;
  // level opening (a level loads on a worker thread)
  std::future<std::unique_ptr<Scene>> m_level_future;
  std::string m_level_name;
  // a level being added to the document (a worker thread too)
  std::future<std::unique_ptr<Scene>> m_add_future;
  std::string m_adding_level;
  std::string m_add_error;       // why the last level could not be added, "" when it was
  uint64_t m_levels_added = 0;   // counts the additions finished (the remote waits on it)
  // nav mesh generation (a worker thread) and its last outcome
  struct NavJob {
    bool ok = false;
    std::string error;
    std::vector<NavMeshData> meshes;
    NavBuildStats stats;
    AABB region;
  };
  std::future<NavJob> m_nav_future;
  NavJob m_nav_last;           // meshes moved into the document once applied
  std::unordered_map<uint64_t, NavMeshData> m_nav_draw_cache;  // the polygons of each nav mesh drawn, by mesh
  uint64_t m_nav_generation = 0;  // counts the generations applied (the remote waits on it)
  // a batch of placements (place_many): one undo entry for all, lit at the end
  std::unique_ptr<NodeChange> m_batch_change;
  std::vector<uint32_t> m_batch_placed;
  // confirmation before losing unsaved changes
  std::function<void()> m_pending_discard;
  bool m_show_discard = false;
  // dialogs
  struct DialogResult {
    DialogAction action;
    std::string path;
  };
  std::mutex m_dialog_mutex;
  std::vector<DialogResult> m_dialog_results;
  float m_frame_ms = 0;               // the CPU time of a frame, averaged (get_status)
  uint64_t m_frame_revision = ~0ull;  // history revision the scene cache was computed at
  uint64_t m_story_revision = ~0ull;  // history revision the story view was applied at
  // the game's data, once read
  std::shared_ptr<const GameData> m_game_data;
  bool m_game_data_read = false;
  // remote control: the connection, the requests waiting for something (checked each frame)
  RemoteServer m_remote;
  std::vector<std::function<bool()>> m_remote_waits;
  // the ocean of the level: a mesh at height 0, drawn at the ocean's height, its waves moving
  std::shared_ptr<Mesh> m_ocean;
  WaterLook m_ocean_look;
  float m_water_time = 0;  // seconds: the water's animation
  // its look in the view: lit by the game's hour in the game view
  WaterLook ocean_look(const RenderSettings& view_rs) const;
  std::string m_ocean_map;
  float m_ocean_height = 0;
  // the land of the levels the game shows around the document's, read on a worker: no water drawn
  // there (in the game, their ground hides it); its key: the map, the height, the levels
  std::vector<uint8_t> m_ocean_land;
  std::string m_ocean_land_key, m_ocean_land_want;
  std::future<std::vector<uint8_t>> m_ocean_land_future;
  void poll_ocean_land();
  bool m_ocean_far = false;  // the far ocean drawn: a level of the document asks for it
  Vec3 m_ocean_far_color;

  friend void dialog_callback(void*, const char* const*, int);
};

}  // namespace ogle
