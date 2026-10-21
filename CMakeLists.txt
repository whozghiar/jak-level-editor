cmake_minimum_required(VERSION 3.20)
project(jak_level_editor_extractor LANGUAGES C CXX)

# ogle-extract, the level extractor of Jak Level Editor (extractor/): it reads the user's own game
# disc and writes the .glb files the editor loads. The editor itself is Rust (Cargo.toml); this
# builds nothing else. Into <build>/bin/: ogle-extract and its tables (ogle-extract-data/), where
# the editor looks for them when <build> is build/.

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
  set(CMAKE_BUILD_TYPE Release CACHE STRING "" FORCE)
endif()

add_subdirectory(extractor)
