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
  std::string theme;       // the first word of its name, what it comes from ("city", "wascity", "des")
  std::string background;  // the level that draws it most: where its image is read
  int levels = 0;          // the levels whose decor draws it
  int uses = 0;            // the pieces drawing it in all the levels
};

// The textures drawn by the decor of at least two levels, or by many pieces of one, the most
// shared first (at most `max`).
std::vector<PaletteTexture> recurring_textures(const LibraryIndex& index, const AssetLibrary& library,
                                               size_t max = 600);

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
  std::map<std::string, std::string> m_wanted;  // name -> background
  std::map<std::string, Texture> m_images;
  std::map<std::string, unsigned> m_gl;
  std::future<std::vector<Texture>> m_work;
  std::vector<std::string> m_working;  // the names being read
};

}  // namespace ogle
