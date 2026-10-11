#pragma once

// The editor's preferences, kept between sessions in the user's settings folder
// (%APPDATA%/OpenGOAL/open-goal-level-editor on Windows, ~/.local/share/... on Linux): where the
// extracted game is, the disc it came from, the language, recent projects. Thumbnails are
// cached next to them.

#include <string>
#include <vector>

namespace ogle {

struct Prefs {
  // the folder ogle-extract writes to: the game in its subfolder jak3/
  std::string library_root;
  std::string extract_source;  // the user's .iso or disc folder
  std::string language;        // "" = the system's
  std::vector<std::string> recent_projects;
  bool guide_seen = false;     // the guided tour was shown (at the first start)
  // the command line modes (captures, tests) read the preferences, never write them
  static inline bool read_only = false;

  // folder of the preferences and of the thumbnail cache (created on demand), '/' separated
  static std::string dir();
  static Prefs& get();
  void load();
  void save() const;
  void add_recent_project(const std::string& path);
};

}  // namespace ogle
