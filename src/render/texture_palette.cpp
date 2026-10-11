#include "render/texture_palette.h"

#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <set>

#include "glad/glad.h"
#include "stb_image.h"

namespace ogle {

namespace {

namespace fs = std::filesystem;

fs::path to_path(const std::string& s) { return fs::path(std::u8string((const char8_t*)s.c_str())); }

std::vector<uint8_t> base64_decode(const std::string& in, size_t from) {
  static int8_t table[256];
  static bool ready = false;
  if (!ready) {
    std::memset(table, -1, sizeof(table));
    const char* chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (int i = 0; chars[i]; i++) table[(uint8_t)chars[i]] = (int8_t)i;
    ready = true;
  }
  std::vector<uint8_t> out;
  out.reserve((in.size() - from) * 3 / 4);
  uint32_t acc = 0;
  int bits = 0;
  for (size_t i = from; i < in.size(); i++) {
    const int8_t v = table[(uint8_t)in[i]];
    if (v < 0) continue;  // padding, line breaks
    acc = (acc << 6) | (uint32_t)v;
    bits += 6;
    if (bits >= 8) {
      bits -= 8;
      out.push_back((uint8_t)(acc >> bits));
    }
  }
  return out;
}

}  // namespace

std::vector<PaletteTexture> recurring_textures(const LibraryIndex& index, const AssetLibrary& library, size_t max) {
  std::map<std::string, PaletteTexture> all;
  std::map<std::string, int> most;  // the most pieces one level draws it with
  for (const auto& li : index.levels()) {
    const LevelEntry* level = library.level(li.level);
    if (!level || !level->listed) continue;
    for (const auto& [name, n] : li.textures) {
      // the animated textures' slots and the far scenery's are no material to build with
      if (name.rfind("anim-slot", 0) == 0 || name.find("lowres") != std::string::npos) continue;
      PaletteTexture& t = all[name];
      t.name = name;
      t.levels++;
      t.uses += n;
      if (n > most[name]) {
        most[name] = n;
        t.background = li.background;
      }
    }
  }
  // by theme (the sewers' many small levels would fill it alone): the most shared of each theme, the
  // themes most drawn first
  std::map<std::string, std::vector<PaletteTexture>> themes;
  std::map<std::string, long> drawn;
  for (auto& [name, t] : all) {
    if (t.levels < 2 && t.uses < 30) continue;
    const size_t dash = name.find('-');
    t.theme = dash == std::string::npos ? name : name.substr(0, dash);
    drawn[t.theme] += t.uses;
    themes[t.theme].push_back(std::move(t));
  }
  std::vector<std::pair<long, std::string>> order;
  for (const auto& [theme, n] : drawn) order.push_back({-n, theme});
  std::sort(order.begin(), order.end());
  std::vector<PaletteTexture> out;
  for (const auto& [n, theme] : order) {
    auto& list = themes[theme];
    std::sort(list.begin(), list.end(), [](const PaletteTexture& a, const PaletteTexture& b) {
      return a.levels != b.levels ? a.levels > b.levels : a.uses != b.uses ? a.uses > b.uses : a.name < b.name;
    });
    for (size_t i = 0; i < list.size() && i < 30 && out.size() < max; i++) out.push_back(std::move(list[i]));
  }
  return out;
}

std::vector<PaletteTexture> all_textures(const LibraryIndex& index, const AssetLibrary& library, const std::string& game) {
  std::map<std::string, PaletteTexture> all;
  std::map<std::string, int> most;  // the most pieces one level draws it with
  for (const auto& li : index.levels()) {
    const LevelEntry* level = library.level(li.level);
    if (!level || !level->listed) continue;
    for (const auto& [name, n] : li.textures) {
      if (name.rfind("anim-slot", 0) == 0 || name.find("lowres") != std::string::npos) continue;
      PaletteTexture& t = all[name];
      t.name = name;
      t.game = game;
      t.levels++;
      t.uses += n;
      if (n > most[name]) {
        most[name] = n;
        t.background = li.background;
      }
    }
  }
  std::vector<PaletteTexture> out;
  out.reserve(all.size());
  for (auto& [name, t] : all) {
    const size_t dash = name.find('-');
    t.theme = dash == std::string::npos ? name : name.substr(0, dash);
    out.push_back(std::move(t));
  }
  std::sort(out.begin(), out.end(), [](const PaletteTexture& a, const PaletteTexture& b) {
    return a.levels != b.levels ? a.levels > b.levels : a.uses != b.uses ? a.uses > b.uses : a.name < b.name;
  });
  return out;
}

bool read_level_textures(const std::string& background, const std::vector<std::string>& names, std::vector<Texture>* out,
                         std::string* error) {
  std::ifstream f(to_path(background), std::ios::binary);
  uint32_t head[5] = {};
  if (!f || !f.read((char*)head, sizeof(head)) || head[0] != 0x46546C67u || head[4] != 0x4E4F534Au) {
    if (error) *error = "not a glTF binary file: " + background;
    return false;
  }
  std::string text(head[3], '\0');
  if (!f.read(text.data(), text.size())) {
    if (error) *error = "truncated file: " + background;
    return false;
  }
  const json j = json::parse(text, nullptr, false);
  if (j.is_discarded() || !j.is_object() || !j.contains("images")) {
    if (error) *error = "no images in " + background;
    return false;
  }
  // the binary chunk, for images kept there (the extractor writes data URIs)
  std::vector<uint8_t> bin;
  auto binary = [&]() -> const std::vector<uint8_t>& {
    if (bin.empty()) {
      uint32_t chunk[2] = {};
      f.clear();
      f.seekg(20 + head[3]);
      if (f.read((char*)chunk, sizeof(chunk)) && chunk[1] == 0x004E4942u) {
        bin.resize(chunk[0]);
        f.read((char*)bin.data(), bin.size());
      }
    }
    return bin;
  };
  const std::set<std::string> wanted(names.begin(), names.end());
  for (const auto& im : j["images"]) {
    const std::string name = im.value("name", std::string());
    if (!wanted.count(name)) continue;
    std::vector<uint8_t> bytes;
    const std::string uri = im.value("uri", std::string());
    if (uri.rfind("data:", 0) == 0) {
      const size_t comma = uri.find(',');
      if (comma != std::string::npos) bytes = base64_decode(uri, comma + 1);
    } else if (im.contains("bufferView") && j.contains("bufferViews")) {
      const json& bv = j["bufferViews"][im["bufferView"].get<int>()];
      const size_t off = bv.value("byteOffset", (size_t)0), len = bv.value("byteLength", (size_t)0);
      const auto& b = binary();
      if (off + len <= b.size()) bytes.assign(b.begin() + off, b.begin() + off + len);
    }
    int w = 0, h = 0, comp = 0;
    unsigned char* data = bytes.empty() ? nullptr : stbi_load_from_memory(bytes.data(), (int)bytes.size(), &w, &h, &comp, 4);
    if (!data) continue;
    Texture t;
    t.name = name;
    t.width = w;
    t.height = h;
    t.rgba.assign(data, data + (size_t)w * h * 4);
    stbi_image_free(data);
    out->push_back(std::move(t));
  }
  return true;
}

TexturePalette::~TexturePalette() {
  if (m_work.valid()) m_work.wait();
}

const Texture* TexturePalette::image(const PaletteTexture& t) {
  const std::string k = key(t);
  auto it = m_images.find(k);
  if (it != m_images.end()) return it->second.width > 0 ? &it->second : nullptr;  // read (or not there)
  if (std::find(m_working.begin(), m_working.end(), k) == m_working.end()) m_wanted[k] = {t.background, t.name, t.game};
  return nullptr;
}

unsigned TexturePalette::gl(const PaletteTexture& t) {
  auto it = m_gl.find(key(t));
  if (it != m_gl.end()) return it->second;
  const Texture* img = image(t);
  if (!img || img->width <= 0 || img->height <= 0) return 0;
  GLuint id = 0;
  glGenTextures(1, &id);
  glBindTexture(GL_TEXTURE_2D, id);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
  // shown opaque: the game's alpha (128 is opaque on the PS2) would let the button's color through
  glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_SWIZZLE_A, GL_ONE);
  glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA8, img->width, img->height, 0, GL_RGBA, GL_UNSIGNED_BYTE, img->rgba.data());
  glBindTexture(GL_TEXTURE_2D, 0);
  m_gl[key(t)] = id;
  return id;
}

void TexturePalette::update() {
  using namespace std::chrono_literals;
  if (m_work.valid() && m_work.wait_for(0s) == std::future_status::ready) {
    for (auto& t : m_work.get()) {
      PaletteTexture p;
      p.name = t.name;
      p.game = m_working_game;
      m_images[key(p)] = std::move(t);
    }
    // what the level did not have: not asked again
    for (const auto& n : m_working)
      if (!m_images.count(n)) m_images[n] = Texture{};
    m_working.clear();
  }
  if (m_work.valid() || m_wanted.empty()) return;
  // the images of one level at once: its file is read once
  const std::string background = m_wanted.begin()->second.background;
  m_working_game = m_wanted.begin()->second.game;
  std::vector<std::string> names;
  for (auto it = m_wanted.begin(); it != m_wanted.end();)
    if (it->second.background == background) {
      m_working.push_back(it->first);
      names.push_back(it->second.name);
      it = m_wanted.erase(it);
    } else {
      ++it;
    }
  m_work = std::async(std::launch::async, [background, names]() {
    std::vector<Texture> out;
    std::string err;
    read_level_textures(background, names, &out, &err);
    return out;
  });
}

void TexturePalette::clear() {
  if (m_work.valid()) m_work.wait();
  m_work = {};
  m_working.clear();
  m_wanted.clear();
  m_images.clear();
  for (auto& [name, id] : m_gl) glDeleteTextures(1, &id);
  m_gl.clear();
}

}  // namespace ogle
