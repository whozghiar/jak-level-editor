cmake_minimum_required(VERSION 3.20)
project(open_goal_level_editor LANGUAGES C CXX)

# A simple level editor for OpenGOAL (Jak 3). It does not need jak-project to build or to
# run: ogle-extract (extractor/) reads the user's own game disc and writes .glb files, the editor
# loads a level from them, moves, rotates, copies and deletes its elements, makes prefabs and nav
# meshes.

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

# ---------------------------------------------------------------------------------------------
# SDL3 (window, input, file dialogs). Order: an installed SDL3 package, a source tree given with
# -DOGLE_SDL3_SOURCE_DIR=<path>, then a download of the release-3.4.10 tag.
# ---------------------------------------------------------------------------------------------
set(OGLE_SDL3_SOURCE_DIR "" CACHE PATH "SDL3 source tree to build instead of downloading it")

find_package(SDL3 CONFIG QUIET)
if(SDL3_FOUND)
  message(STATUS "open-goal-level-editor: using installed SDL3")
  set(OGLE_SDL_TARGET SDL3::SDL3)
else()
  set(SDL_SHARED OFF CACHE BOOL "" FORCE)
  set(SDL_STATIC ON CACHE BOOL "" FORCE)
  set(SDL_TEST_LIBRARY OFF CACHE BOOL "" FORCE)
  set(SDL_TESTS OFF CACHE BOOL "" FORCE)
  set(SDL_EXAMPLES OFF CACHE BOOL "" FORCE)
  set(SDL_INSTALL OFF CACHE BOOL "" FORCE)
  if(OGLE_SDL3_SOURCE_DIR AND EXISTS "${OGLE_SDL3_SOURCE_DIR}/CMakeLists.txt")
    message(STATUS "open-goal-level-editor: building SDL3 from ${OGLE_SDL3_SOURCE_DIR}")
    add_subdirectory("${OGLE_SDL3_SOURCE_DIR}" "${CMAKE_BINARY_DIR}/SDL3" EXCLUDE_FROM_ALL)
  else()
    message(STATUS "open-goal-level-editor: downloading SDL3")
    include(FetchContent)
    FetchContent_Declare(SDL3
      GIT_REPOSITORY https://github.com/libsdl-org/SDL.git
      GIT_TAG release-3.4.10
      GIT_SHALLOW TRUE)
    FetchContent_MakeAvailable(SDL3)
  endif()
  set(OGLE_SDL_TARGET SDL3::SDL3-static)
endif()

# ---------------------------------------------------------------------------------------------
# Vendored libraries
# ---------------------------------------------------------------------------------------------
set(TP "${CMAKE_CURRENT_SOURCE_DIR}/third-party")

add_library(ogle_glad STATIC "${TP}/glad/src/glad.c")
target_include_directories(ogle_glad PUBLIC "${TP}/glad/include")

add_library(ogle_imgui STATIC
  "${TP}/imgui/imgui.cpp"
  "${TP}/imgui/imgui_draw.cpp"
  "${TP}/imgui/imgui_tables.cpp"
  "${TP}/imgui/imgui_widgets.cpp"
  "${TP}/imgui/imgui_stdlib.cpp"
  "${TP}/imgui/backends/imgui_impl_sdl3.cpp"
  "${TP}/imgui/backends/imgui_impl_opengl3.cpp")
target_include_directories(ogle_imgui PUBLIC "${TP}/imgui" "${TP}/imgui/backends")
target_link_libraries(ogle_imgui PUBLIC ${OGLE_SDL_TARGET})

# Recast (recastnavigation): the nav mesh generator
file(GLOB OGLE_RECAST_SOURCES CONFIGURE_DEPENDS "${TP}/recast/Source/*.cpp")
add_library(ogle_recast STATIC ${OGLE_RECAST_SOURCES})
target_include_directories(ogle_recast PUBLIC "${TP}/recast/Include")

# The game's renderer: OpenGOAL's, vendored in extractor/opengoal with jak-project's paths (ISC
# license, extractor/opengoal/LICENSE): the background renderers the game view draws the level with
# (src/game_view), and the common code they need. Its shaders are copied next to the editor.
set(OG "${CMAKE_CURRENT_SOURCE_DIR}/extractor/opengoal")
add_library(ogle_game_render STATIC
  "${OG}/game/graphics/opengl_renderer/background/TFragment.cpp"
  "${OG}/game/graphics/opengl_renderer/background/background_common.cpp"
  "${OG}/game/graphics/opengl_renderer/BucketRenderer.cpp"
  "${OG}/game/graphics/opengl_renderer/Shader.cpp"
  "${OG}/game/graphics/opengl_renderer/dma_helpers.cpp"
  "${OG}/common/custom_data/TFrag3Data.cpp"
  "${OG}/common/dma/dma.cpp"
  "${OG}/common/dma/gs.cpp"
  "${OG}/common/util/Timer.cpp"
  "${OG}/third-party/fmt/src/format.cc")
# the editor's root: the vendored code includes third-party/glad and third-party/imgui from there
target_include_directories(ogle_game_render PUBLIC "${OG}" "${OG}/third-party/fmt/include"
                           "${CMAKE_CURRENT_SOURCE_DIR}")
target_link_libraries(ogle_game_render PUBLIC ogle_glad ogle_imgui)
if(WIN32)
  target_compile_definitions(ogle_game_render PUBLIC NOMINMAX _CRT_SECURE_NO_WARNINGS WIN32_LEAN_AND_MEAN)
endif()
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
  # jak-project's own warnings stay its own; the time of day is mixed with SSE4.1 (like the game)
  target_compile_options(ogle_game_render PRIVATE -w -msse4.1)
endif()

# ---------------------------------------------------------------------------------------------
# The editor
# ---------------------------------------------------------------------------------------------
file(GLOB_RECURSE OGLE_SOURCES CONFIGURE_DEPENDS
  "${CMAKE_CURRENT_SOURCE_DIR}/src/*.cpp")

add_executable(open-goal-level-editor ${OGLE_SOURCES})
target_include_directories(open-goal-level-editor PRIVATE
  "${CMAKE_CURRENT_SOURCE_DIR}/src"
  "${TP}/tinygltf"
  "${TP}/json"
  "${TP}/stb")
target_link_libraries(open-goal-level-editor PRIVATE ogle_imgui ogle_glad ogle_recast ogle_game_render
                      ${OGLE_SDL_TARGET})
# the default folder of the extracted games: library/ in the editor's source tree (ignored by git)
target_compile_definitions(open-goal-level-editor PRIVATE OGLE_PROJECT_DIR="${CMAKE_CURRENT_SOURCE_DIR}")

if(WIN32)
  target_compile_definitions(open-goal-level-editor PRIVATE NOMINMAX _CRT_SECURE_NO_WARNINGS
                             WIN32_LEAN_AND_MEAN)
  # shell32: the command line in UTF-16; ws2_32: the remote control of the MCP server
  target_link_libraries(open-goal-level-editor PRIVATE shell32 ws2_32)
endif()
if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
  target_compile_options(open-goal-level-editor PRIVATE -Wall -Wno-unused-parameter
                         -Wno-missing-field-initializers)
endif()

set_target_properties(open-goal-level-editor PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin")

# the translations of the interface (lang/<code>.json) and what the editor knows of the game
# (data/jak3/game-data.json, made by tools/gen_game_data.py), next to the executable: copied again
# at every build (not only when the editor is linked again), without the files removed since
add_custom_target(ogle_resources ALL
  COMMAND ${CMAKE_COMMAND} -E rm -rf "${CMAKE_BINARY_DIR}/bin/lang" "${CMAKE_BINARY_DIR}/bin/data"
          "${CMAKE_BINARY_DIR}/bin/game-shaders"
  COMMAND ${CMAKE_COMMAND} -E copy_directory "${CMAKE_CURRENT_SOURCE_DIR}/lang"
          "${CMAKE_BINARY_DIR}/bin/lang"
  COMMAND ${CMAKE_COMMAND} -E copy_directory "${CMAKE_CURRENT_SOURCE_DIR}/data"
          "${CMAKE_BINARY_DIR}/bin/data"
  COMMAND ${CMAKE_COMMAND} -E copy_directory "${OG}/game/graphics/opengl_renderer/shaders"
          "${CMAKE_BINARY_DIR}/bin/game-shaders")
add_dependencies(open-goal-level-editor ogle_resources)

# ---------------------------------------------------------------------------------------------
# The level extractor (ogle-extract): reads the user's game disc, writes the .glb files
# ---------------------------------------------------------------------------------------------
option(OGLE_BUILD_EXTRACTOR "Build ogle-extract, the level extractor" ON)
if(OGLE_BUILD_EXTRACTOR)
  add_subdirectory(extractor)
endif()
