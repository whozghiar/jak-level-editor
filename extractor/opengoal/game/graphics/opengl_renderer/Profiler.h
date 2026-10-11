#pragma once

// ogle: the editor's stand-in for the game's profiler (frame statistics of its debug window).

#include <string>

class ScopedProfilerNode {
 public:
  ScopedProfilerNode make_scoped_child(const std::string&) { return {}; }
  void add_tri(int) {}
  void add_draw_call(int = 1) {}
};
