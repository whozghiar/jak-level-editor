#include "core/prefs.h"

#include <SDL3/SDL.h>

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <sstream>

#include "core/log.h"
#include "json.hpp"

namespace ogle {

namespace fs = std::filesystem;
using json = nlohmann::json;

namespace {

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }

void push_recent(std::vector<std::string>& list, const std::string& v, size_t max) {
  list.erase(std::remove(list.begin(), list.end(), v), list.end());
  list.insert(list.begin(), v);
  if (list.size() > max) list.resize(max);
}

}  // namespace

std::string Prefs::dir() {
  static std::string cached;
  if (cached.empty()) {
    char* p = SDL_GetPrefPath("OpenGOAL", "open-goal-level-editor");
    if (p) {
      cached = p;
      SDL_free(p);
    } else {
      cached = "./";
    }
    for (auto& c : cached)
      if (c == '\\') c = '/';
    if (cached.back() != '/') cached += '/';
  }
  return cached;
}

Prefs& Prefs::get() {
  static Prefs prefs;
  return prefs;
}

void Prefs::load() {
  std::ifstream f(to_path(dir() + "prefs.json"));
  if (!f) return;
  std::stringstream ss;
  ss << f.rdbuf();
  try {
    json j = json::parse(ss.str());
    library_root = j.value("library_root", std::string());
    extract_source = j.value("extract_source", std::string());
    language = j.value("language", std::string());
    recent_projects = j.value("recent_projects", std::vector<std::string>());
    export_folder = j.value("export_folder", std::string());
    guide_seen = j.value("guide_seen", false);
  } catch (const std::exception& e) {
    LOG_WARN("unreadable preferences (%s): defaults used", e.what());
  }
}

void Prefs::save() const {
  if (read_only) return;
  json j = {{"library_root", library_root},
            {"extract_source", extract_source},
            {"language", language},
            {"recent_projects", recent_projects},
            {"export_folder", export_folder},
            {"guide_seen", guide_seen}};
  std::error_code ec;
  fs::create_directories(to_path(dir()), ec);
  std::ofstream f(to_path(dir() + "prefs.json"));
  f << j.dump(2) << "\n";
}

void Prefs::add_recent_project(const std::string& path) { push_recent(recent_projects, path, 10); }

}  // namespace ogle
