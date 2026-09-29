#pragma once

// The editor application: window, main loop, viewport interaction and interface. Split over
// app.cpp (loop, camera, viewport), actions.cpp (document operations: levels, projects, clipboard,
// prefabs), panels.cpp (menus, toolbar, panels, windows) and extract.cpp (extraction of the
// game's assets with ogle-extract).

#include <atomic>
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
#include "io/prefabs.h"
#include "render/camera.h"
#include "render/renderer.h"
#include "render/thumbnails.h"
#include "scene/pick.h"
#include "scene/scene.h"
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
};

enum class DialogAction {
  OpenProject,
  SaveProjectAs,
  ExtractSourceIso,
  ExtractSourceFolder,
  ExtractOutput,
  ExportNavMeshes,
  ExportFolder
};

// What the Place tool puts in the level: a prefab, an object of the catalog or a decor part of a
// level.
struct Placement {
  enum class Kind { Prefab, Object, Model, Part };
  Kind kind = Kind::Prefab;
  std::string path;  // the prefab file, the model file (object, model), the part's level background
  std::string name;
  CatalogObject object;  // Object
  CatalogPart part;      // Part
  float yaw = 0.f;    // radians (R, Ctrl + wheel)
  bool once = false;  // dragged from a panel: one placement, then back to selecting
  std::string key() const;  // of its preview and of the meshes merged into the document
};

// The extraction window (extract.cpp): ogle-extract, next to the editor, reads the user's disc and
// writes the levels of one game to <library>/<game>/.
struct ExtractState {
  bool show = false;
  Game game = Game::Jak2;  // the game the user wants
  std::string source;      // the user's .iso or disc folder for that game
  std::string output;      // the library folder
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
  // --game jak1|jak2|jak3: the game of the library shown (for this session)
  std::string game;
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
Clipboard copy_nodes(const Scene& scene, const std::vector<uint32_t>& roots);
// Adds copies of the clipboard's nodes (under their old parent when it is still there, at the
// same place). Returns the new roots.
std::vector<uint32_t> paste_nodes(Scene& scene, const Clipboard& clip);

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
  void viewport_interaction();
  void select_tool(bool clicked, bool released);
  void place_tool(bool clicked);
  void draw_tool_overlays();
  void draw_navmesh_overlay();
  void handle_shortcuts();
  void autosave_tick();
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

  // File > Export for the game (actions.cpp, io/game_export.h): what the view shows, as a custom
  // level of OpenGOAL for Jak 1, Jak II or Jak 3, adapted to that game
  bool export_for_game(const LevelExportOptions& opts, LevelExportReport* report, std::string* error);
  // the export's options for the document and a destination (names, folder, where the player starts)
  LevelExportOptions default_export_options(Game destination);
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
    std::string game;    // its own game: "jak1", "jak2", "jak3" (may differ from the level opened's)
  };
  std::vector<DocumentLevel> document_levels() const;
  bool level_in_document(const std::string& name) const;
  // a level of the library added on a worker thread, then to the document (undoable) when ready
  bool add_level_to_document(const LevelEntry& level, std::string* error);
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
  void select_level(uint32_t group);
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
  void set_library(const std::string& library_root, const std::string& game);
  // the data of a game ("jak2"), read once; null when missing
  std::shared_ptr<const GameData> game_data_for(const std::string& game);
  const GameData* document_game_data();  // of the document's game
  // the story view: after a new document, after the view or the document changed
  void story_document_changed();
  void set_story_view(const StoryView& view);
  void apply_story_view();
  void update_ocean();
  void set_ocean_backdrop();  // the far ocean in the render settings, before the viewport is drawn
  void load_level(const LevelEntry& level);
  void open_project_file(const std::string& path);
  void save(bool save_as);
  void select_only(uint32_t id);
  void toggle_select(uint32_t id);
  std::vector<uint32_t> selection_roots() const;  // selected nodes without selected ancestors
  Mat4 selection_frame() const;                   // gizmo pivot and orientation
  // the middle of most of a large group's geometry (a whole level), in the world; kept in the
  // group's space until the document changes
  Vec3 group_center(uint32_t id, const std::vector<uint32_t>& descendants) const;
  mutable std::unordered_map<uint32_t, std::pair<uint64_t, Vec3>> m_group_centers;
  PickHit pick_surface(const Vec2& mouse_vp);     // what is under a viewport point
  void delete_selection();
  void copy_selection();
  void paste();
  void duplicate_selection();
  void create_prefab(const std::string& name);
  void start_placement(const std::string& prefab_path, const std::string& name, bool once);
  void start_object_placement(const CatalogObject& object, bool once);
  void start_model_placement(const CatalogModel& model, bool once);
  void start_part_placement(const std::string& level_background, const CatalogPart& part, bool once);
  void stop_placement();
  Mat4 placement_matrix(const PickHit& hit, const Ray& ray) const;
  // the mesh of what is placed, merged into the document once (null while it loads)
  std::shared_ptr<Mesh> placement_mesh();
  // puts what is placed at `m`; returns the new roots
  std::vector<uint32_t> place_at(const Mat4& m);
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
  std::string remote_use_game(const json& args);  // "" or what is wrong
  json remote_node_json(const Node& n);
  // what get_level_info says of a level; game: which extracted one (else whichever has it, or
  // the browsed one when none does)
  json remote_level_json(const std::string& name, const std::string& game = std::string());
  bool remote_unsaved(const json& args) const;
  std::string capture_view_png(int w, int h);  // the 3D view from the camera, PNG in base64

  // extraction (extract.cpp)
  static std::string extractor_path();  // ogle-extract next to the editor, "" when missing
  void open_extract_window();
  void extract_select_game(Game g);
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

  // interface (panels.cpp)
  void draw_ui();
  void ui_menu();
  void ui_toolbar(float y, float h);
  void ui_left_panel(float x, float y, float w, float h);
  void ui_levels_panel();
  void ui_catalog_panel();
  void ui_catalog_objects();
  void ui_catalog_models();
  void ui_catalog_decor();
  void ui_catalog_decor_all();  // the decor of every level, searched by name
  // a decor part in a grid of thumbnails: its thumbnail, its tooltip, a click or drag places it
  void ui_decor_cell(const std::string& background, int64_t stamp, const CatalogPart& part, const std::string& from,
                     int levels, float size);
  void ui_prefabs_panel();
  void ui_inspector(float x, float y, float w, float h);
  void ui_node_inspector(Node& n);
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
  Camera camera;
  RenderSettings rs;
  Gizmo gizmo;
  ToolMode tool = ToolMode::Select;
  std::set<uint32_t> selection;
  uint32_t primary = 0;
  uint32_t hovered = 0;
  std::string project_path;
  uint64_t saved_revision = 0;

  // the library: the extracted games, the levels of the one shown, loading and thumbnails.
  // Every extracted game is always scanned and available (all_libraries), so that the catalog
  // and the document never distinguish games: `library` only picks what the Levels panel and
  // File > New level show by default (their own convenience, not a restriction on what can be
  // placed or added). A document freely mixes levels and actors of any extracted game; each
  // keeps its own game as its extras "og_game" (a level's group, or an actor placed apart), and
  // only the export to a game cares which is which.
  std::string library_root;
  std::vector<GameFolder> games;
  std::map<std::string, AssetLibrary> all_libraries;  // every extracted game, by "jak1"/"jak2"/"jak3"
  AssetLibrary library;                               // the one the Levels panel browses
  AssetLibrary* library_for(const std::string& game);  // an entry of all_libraries, else null
  // switches the browsed game without rescanning (all_libraries already has every extracted one)
  bool browse_game(const std::string& game);
  AssetStore store;
  Thumbnails thumbs{store};
  std::vector<PrefabInfo> prefabs;
  std::vector<CatalogObject> catalog;        // the objects of every extracted game
  std::vector<CatalogModel> catalog_models;  // their other models (props)
  // what the catalog reads of every level of every game on a worker thread (one per game): their
  // decor, the art-names of their actors (all the variants and effects); merged into the catalog
  // as each game's becomes ready
  std::map<std::string, LibraryIndex> library_indexes;
  std::vector<DecorEntry> all_parts;  // the decor of every level of every game
  void apply_library_index();   // any game's index that just became ready
  void rebuild_catalog();       // catalog, catalog_models and all_parts, from every extracted game
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
  bool show_help = false;
  bool show_labels = true;
  bool show_navmesh = true;          // the nav meshes drawn over the level
  bool show_navmesh_window = false;  // Tools > Nav meshes
  NavParams nav_params;
  bool nav_keep_selected = false;    // keep only the ground the selected elements stand on
  uint32_t nav_first_aid = 60000;    // the ids the exported nav meshes take
  int left_tab_request = -1;  // select this tab of the left panel next frame

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
  // select tool
  bool m_box_selecting = false;
  Vec2 m_box_start;
  std::unique_ptr<NodeChange> m_drag_change;
  std::map<uint32_t, Mat4> m_drag_start;
  std::map<uint32_t, Vec3> m_drag_scale;  // local scale of the roots when a scale drag began
  bool m_scale_uniform = true;             // the inspector's scale field keeps the proportions
  // property editing: the node when a field became active
  std::optional<Node> m_edit_before;
  // placement
  bool m_place_valid = false;
  Mat4 m_place_matrix;
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
  int m_catalog_tab = 0;  // 0 objects, 1 models, 2 decor
  std::string m_catalog_filter;
  std::string m_catalog_family;  // "" for every family
  std::string m_decor_level;     // the level whose decor is shown, "" for every level
  std::shared_ptr<Scene> m_decor_scene;
  std::vector<CatalogPart> m_decor_parts;
  bool m_index_applied = false;  // the browsed game's index (the Levels panel's progress bar)
  std::set<std::string> m_index_merged;  // every game whose index is already folded into the catalog
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
  // a batch of placements (place_many): one undo entry for all
  std::unique_ptr<NodeChange> m_batch_change;
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
  // autosave
  uint64_t m_last_seen_revision = 0;
  double m_last_change_time = 0;
  double m_last_backup_time = 0;
  uint64_t m_backup_revision = 0;
  double m_time = 0;
  uint64_t m_frame_revision = ~0ull;  // history revision the scene cache was computed at
  uint64_t m_story_revision = ~0ull;  // history revision the story view was applied at
  // game data by game
  std::map<std::string, std::shared_ptr<const GameData>> m_game_data;
  // remote control: the connection, the requests waiting for something (checked each frame)
  RemoteServer m_remote;
  std::vector<std::function<bool()>> m_remote_waits;
  // the ocean of the level: a mesh at height 0, drawn at the ocean's height
  std::shared_ptr<Mesh> m_ocean;
  std::vector<Material> m_ocean_materials;
  std::string m_ocean_map;
  float m_ocean_height = 0;
  bool m_ocean_far = false;  // the far ocean drawn: a level of the document asks for it
  Vec3 m_ocean_far_color;

  friend void dialog_callback(void*, const char* const*, int);
};

}  // namespace ogle
