cmake_minimum_required(VERSION 3.24)
project(warscape_community_editor LANGUAGES C CXX)

# Migration scaffolding (PLAN.md, Phase 0): the two imported projects build unchanged from legacy/ until their code
# has moved into libs/ and tools/. legacy/tile_editor still compiles some legacy/weather_editor sources in place.
add_subdirectory(legacy/weather_editor)
add_subdirectory(legacy/tile_editor)
