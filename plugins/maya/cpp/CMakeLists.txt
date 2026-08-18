# =============================================================================
# NEXORA Bridge — compiled Maya plug-in (.mll) build
# =============================================================================
# Builds nexora_bridge.mll against the Maya devkit.
#
# Requirements:
#   * Maya devkit (headers + libs). Either a full Maya install or the
#     "devkitBase" download for your Maya version.
#   * A C++ compiler: Visual Studio (Windows), clang (macOS), gcc (Linux).
#   * Internet at configure time (fetches the header-only nlohmann/json), OR
#     drop json.hpp beside the source and set NEXORA_VENDORED_JSON=ON.
#
# Configure + build (Windows example):
#   set MAYA_LOCATION=C:\Program Files\Autodesk\Maya2024
#   cmake -B build -S . -G "Visual Studio 17 2022" -A x64
#   cmake --build build --config Release
#   -> build\Release\nexora_bridge.mll
# =============================================================================
cmake_minimum_required(VERSION 3.16)
project(nexora_bridge CXX)

# Maya 2025+ (incl. 2026/2027) devkit is built with C++17.
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# --- Locate Maya -------------------------------------------------------------
if(NOT MAYA_LOCATION)
    if(DEFINED ENV{MAYA_LOCATION})
        set(MAYA_LOCATION $ENV{MAYA_LOCATION})
    else()
        message(FATAL_ERROR "Set MAYA_LOCATION to your Maya install or devkit "
                            "(e.g. -DMAYA_LOCATION=\"C:/Program Files/Autodesk/Maya2024\").")
    endif()
endif()
message(STATUS "Maya: ${MAYA_LOCATION}")

set(MAYA_INCLUDE_DIR "${MAYA_LOCATION}/include")
set(MAYA_LIBRARY_DIR "${MAYA_LOCATION}/lib")

# --- nlohmann/json (header-only) --------------------------------------------
option(NEXORA_VENDORED_JSON "Use a local json.hpp instead of FetchContent" OFF)
if(NEXORA_VENDORED_JSON)
    # Place json.hpp in this folder as ./nlohmann/json.hpp
    add_library(nlohmann_json INTERFACE)
    target_include_directories(nlohmann_json INTERFACE "${CMAKE_CURRENT_SOURCE_DIR}")
    add_library(nlohmann_json::nlohmann_json ALIAS nlohmann_json)
else()
    include(FetchContent)
    FetchContent_Declare(
        nlohmann_json
        URL https://github.com/nlohmann/json/releases/download/v3.11.3/json.hpp
        DOWNLOAD_NO_EXTRACT TRUE
        DOWNLOAD_DIR "${CMAKE_BINARY_DIR}/_deps/json/nlohmann"
    )
    FetchContent_MakeAvailable(nlohmann_json)
    add_library(nlohmann_json INTERFACE)
    target_include_directories(nlohmann_json INTERFACE "${CMAKE_BINARY_DIR}/_deps/json")
    add_library(nlohmann_json::nlohmann_json ALIAS nlohmann_json)
endif()

# --- The plug-in -------------------------------------------------------------
add_library(nexora_bridge SHARED nexora_bridge.cpp)
target_include_directories(nexora_bridge PRIVATE "${MAYA_INCLUDE_DIR}")
target_link_directories(nexora_bridge PRIVATE "${MAYA_LIBRARY_DIR}")
target_link_libraries(nexora_bridge PRIVATE OpenMaya Foundation nlohmann_json::nlohmann_json)

# Maya requires these defines; platform bits differ.
target_compile_definitions(nexora_bridge PRIVATE REQUIRE_IOSTREAM _BOOL)
if(WIN32)
    target_compile_definitions(nexora_bridge PRIVATE NT_PLUGIN _WIN32_WINNT=0x0601)
    target_link_libraries(nexora_bridge PRIVATE ws2_32)
    set_target_properties(nexora_bridge PROPERTIES SUFFIX ".mll" PREFIX "")
elseif(APPLE)
    target_compile_definitions(nexora_bridge PRIVATE OSMac_ MAC_PLUGIN)
    set_target_properties(nexora_bridge PROPERTIES SUFFIX ".bundle" PREFIX "")
else()
    target_compile_definitions(nexora_bridge PRIVATE LINUX LINUX_64)
    set_target_properties(nexora_bridge PROPERTIES SUFFIX ".so" PREFIX "")
endif()
