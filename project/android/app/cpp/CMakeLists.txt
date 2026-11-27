cmake_minimum_required(VERSION 3.7)
project(krkr2yuri_android)

set(KRKR2YURI_DIR ${CMAKE_SOURCE_DIR}/../../../..)
if(${ANDROID_ABI} MATCHES "arm64-v8a")
    set(PORTBUILD_PATH ${KRKR2YURI_DIR}/vendor/build/arch_androida64)
else()
    set(PORTBUILD_PATH ${KRKR2YURI_DIR}/vendor/build/arch_androida64)
endif()

# Diagnostic knobs, set by .github/workflows/release-apk.yml (environment variables):
#   KRKR2_EXTRA_CXX_FLAGS  extra flags for our own C/C++ code (not the prebuilt vendor/ libs)
#   KRKR2_OPT_LEVEL        optimisation level of the Release build: O0, O1, O2, O3, Os
if (NOT "$ENV{KRKR2_EXTRA_CXX_FLAGS}" STREQUAL "")
    set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS} $ENV{KRKR2_EXTRA_CXX_FLAGS}")
    set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} $ENV{KRKR2_EXTRA_CXX_FLAGS}")
endif()
if (NOT "$ENV{KRKR2_OPT_LEVEL}" STREQUAL "")
    set(CMAKE_C_FLAGS_RELEASE "-$ENV{KRKR2_OPT_LEVEL} -DNDEBUG")
    set(CMAKE_CXX_FLAGS_RELEASE "-$ENV{KRKR2_OPT_LEVEL} -DNDEBUG")
endif()

if (CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CMAKE_CXX_FLAGS_DEBUG "${CMAKE_CXX_FLAGS_DEBUG} -O0 -g")
endif()

add_subdirectory(${KRKR2YURI_DIR} ${KRKR2YURI_DIR}/build_android)