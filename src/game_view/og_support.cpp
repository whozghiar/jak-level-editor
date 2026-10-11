// What the game's renderer vendored from OpenGOAL (extractor/opengoal, built as ogle_game_render)
// takes from its program: its log, its assertions and its global settings. A failed assertion of
// the game's code is an error the game view reports (game_view.cpp), never the end of the editor.

#include <stdexcept>
#include <string>

#include "common/log/log.h"
#include "common/util/Assert.h"
#include "core/log.h"
#include "game/graphics/pipelines/opengl.h"

namespace Gfx {
GlobalSettings g_global_settings;
}

namespace lg::internal {
void log_message(level log_level, LogTime&, const char* message) {
  if (log_level == level::die) throw std::runtime_error(message);
  if (log_level >= level::warn) LOG_WARN("game renderer: %s", message);
}
void log_print(const char* message) {
  LOG_INFO("game renderer: %s", message);
}
}  // namespace lg::internal

void private_assert_failed(const char* expr, const char* file, int line, const char* function, const char* msg) {
  throw std::runtime_error(std::string("assertion failed: ") + expr + (msg && msg[0] ? std::string(" (") + msg + ")" : "") +
                           " at " + file + ":" + std::to_string(line) + " in " + function);
}

void private_assert_failed(const char* expr, const char* file, int line, const char* function,
                           const std::string_view& msg) {
  private_assert_failed(expr, file, line, function, std::string(msg).c_str());
}
