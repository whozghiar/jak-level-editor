#pragma once

// The palette of materials: the textures of Jak 3's decor that its levels use again and again (city
// walls, metal floors, Spargus' sandstone, wood planks...), to give any element of a level on the
// fly. Found by the library's index (how many pieces of each level draw each texture); their images
// read from the levels' files alone (the extractor writes them as data URIs in the glTF's JSON: no
// geometry is loaded), on a worker thread, then made GL textures for the interface.

#include <chrono>
#include <future>
#include <map>
#include <string>
#include <vector>

#include "io/asset_library.h"
#include "io/library_index.h"
#include "scene/scene.h"

namespace ogle {

struct PaletteTexture {
  std::string name;        // the game's name of the texture ("city-ind-wall-05")
  std::string game;        // "" Jak 3's, "jak2" Jak II's (named "jak2-<name>" in a document)
  std::string theme;       // the first word of its name, what it comes from ("city", "wascity", "des")
  std::string background;  // the level that draws it most: where its image is read
  int levels = 0;          // the levels whose decor draws it
  int uses = 0;            // the pieces drawing it in all the levels
};

// The textures drawn by the decor of at least two levels, or by many pieces of one, the most
// shared first (at most `max`).
std::vector<PaletteTexture> recurring_textures(const LibraryIndex& index, const AssetLibrary& library,
                                               size_t max = 600);

// Every texture the decor of the levels draws (but the animated textures' slots and the far
// scenery's), the most shared first; `game`: "" for Jak 3's library, "jak2" for Jak II's.
std::vector<PaletteTexture> all_textures(const LibraryIndex& index, const AssetLibrary& library,
                                         const std::string& game = "");

// The images named `names` of a level's background file, read from its JSON (RGBA). Those it does
// not have are left out.
bool read_level_textures(const std::string& background, const std::vector<std::string>& names, std::vector<Texture>* out,
                         std::string* error);

// The images of the palette, read when asked for (a level's at once), and their GL textures for the
// interface. Used from the main thread.
class TexturePalette {
 public:
  ~TexturePalette();
  // the image of a texture of the palette: null while it is read (it is asked for)
  const Texture* image(const PaletteTexture& t);
  // its GL texture, for ImGui (0 while it is read)
  unsigned gl(const PaletteTexture& t);
  void update();  // each frame: the images read
  bool busy() const { return m_work.valid() || !m_wanted.empty(); }  // images being read
  void clear();   // another library

 private:
  // by key: the name, "jak2:" before Jak II's (both games have textures of the same names)
  static std::string key(const PaletteTexture& t) { return t.game.empty() ? t.name : t.game + ":" + t.name; }
  struct Wanted {
    std::string background, name, game;
  };
  std::map<std::string, Wanted> m_wanted;
  std::map<std::string, Texture> m_images;
  std::map<std::string, unsigned> m_gl;
  std::future<std::vector<Texture>> m_work;
  std::vector<std::string> m_working;  // the keys being read
  std::string m_working_game;          // their game
};

}  // namespace ogle
