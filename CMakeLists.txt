cmake_minimum_required(VERSION 3.25)

#   CROSSFIRE IS ITS OWN PROJECT, built against the light framework beside it. The framework
# supplies the C shell, the portable crates, the ports, the asset compilers and the shared
# script layer; this repository supplies the application, the two tangible boards it runs on,
# and everything that is crossfire's alone -- its version, its signing key, its flash map.
#
#   LIGHT_PATH points at that framework checkout. It is overridable from the environment (the
# script layer sets it) or the cache, and defaults to the sibling directory, which is the layout
# every project here shares.
if(DEFINED ENV{LIGHT_PATH} AND (NOT LIGHT_PATH))
        set(LIGHT_PATH "$ENV{LIGHT_PATH}" CACHE PATH "path to the light framework")
endif()
if(DEFINED ENV{PICO_SDK_PATH} AND (NOT PICO_SDK_PATH))
        set(PICO_SDK_PATH "$ENV{PICO_SDK_PATH}" CACHE PATH "path to the pico-sdk checkout")
endif()
if(NOT LIGHT_PATH)
        set(LIGHT_PATH "${CMAKE_CURRENT_LIST_DIR}/../light_mk5" CACHE PATH "path to the light framework")
endif()
get_filename_component(LIGHT_PATH "${LIGHT_PATH}" REALPATH BASE_DIR "${CMAKE_CURRENT_LIST_DIR}")
if(NOT EXISTS "${LIGHT_PATH}/light_init.cmake")
        message(FATAL_ERROR
                "could not find the light framework at '${LIGHT_PATH}'.\n"
                "It is normally the light_mk5 checkout beside this one; set LIGHT_PATH for another layout.")
endif()
include(${LIGHT_PATH}/external/light_preinit.cmake)

#   crossfire's OWN version, which is what its images are stamped with and what the boot ROM
# compares between the two slots. It belongs to this project, not to the framework beside it:
# the two ship separately, and a device chooses between images by THIS number.
project(crossfire VERSION 0.1.0 LANGUAGES C CXX ASM)
set(CROSSFIRE_VERSION "${PROJECT_VERSION}")

if(LIGHT_PLATFORM STREQUAL "TARGET")
        if(LIGHT_SYSTEM STREQUAL "PICO_SDK")
                pico_sdk_init()
        endif()

        #   rustup installs to ~/.cargo/bin and does not touch PATH, so a plain `cmake --preset`
        # from a shell without the script layer's environment still finds cargo
        list(APPEND CMAKE_PROGRAM_PATH "$ENV{USERPROFILE}/.cargo/bin" "$ENV{HOME}/.cargo/bin")

        include(FetchContent)
        FetchContent_Declare(Corrosion
                GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
                GIT_TAG v0.6.1
                GIT_SHALLOW TRUE
        )
        FetchContent_MakeAvailable(Corrosion)

        #   one staticlib per tangible board, from THIS project's firmware workspace; only the
        # one the executable links is built
        corrosion_import_crate(
                MANIFEST_PATH ${CMAKE_CURRENT_LIST_DIR}/firmware/Cargo.toml
                CRATES light_app_crossfire_pico light_app_crossfire_pico2
                NO_STD
        )
        #   crush belongs to the framework and runs on the host inside this cross build: the
        # asset compilers below are its command line
        corrosion_import_crate(
                MANIFEST_PATH ${LIGHT_PATH}/Cargo.toml
                CRATES crush
        )
        corrosion_set_hostbuild(crush)

        include(${LIGHT_PATH}/cmake/LightFont.cmake)
        include(${LIGHT_PATH}/cmake/LightTheme.cmake)
        include(${LIGHT_PATH}/cmake/LightUi.cmake)
        include(${LIGHT_PATH}/cmake/LightSecure.cmake)
        #   this project signs with ITS key, not the framework's: the two ship separately and a
        # shared key would tie their releases together
        set(LIGHT_SIGNING_KEY "${CMAKE_CURRENT_LIST_DIR}/keys/dev.pem" CACHE FILEPATH
                "the key crossfire's images and partition tables are signed with" FORCE)

        if(LIGHT_SYSTEM STREQUAL "PICO_SDK")
                #   the C shell is the framework's, built here into this tree
                add_subdirectory(${LIGHT_PATH}/module/light_shell light_shell)
                #   module/<target>/ because that is where light-flash.ps1 expects
                # module/<target>/<target>.uf2 -- the layout every sibling project shares
                #   the module IS the tangible board, and names its own chip: a crossfire_pico
                # is an RP2040, a crossfire_pico2 is an RP2350
                if(PICO_PLATFORM MATCHES "^rp2040")
                        add_subdirectory(module/crossfire_pico)
                else()
                        add_subdirectory(module/crossfire_pico2)
                endif()
        endif()
endif()

#   the host tree exists so scripts/test.ps1 runs the application's tests through the same door
# as every other project: one ctest that is `cargo test` over the PORTABLE workspace. The
# hardware-bound crates live in the firmware workspace and are never seen here, so there is no
# port to exclude -- which is the reason for the split.
if(LIGHT_PLATFORM STREQUAL "HOST")
        include(CTest)
        find_program(LIGHT_CARGO cargo
                PATHS "$ENV{USERPROFILE}/.cargo/bin" "$ENV{HOME}/.cargo/bin")
        if(NOT LIGHT_CARGO)
                message(FATAL_ERROR "cargo not found -- install rustup")
        endif()
        add_test(NAME cargo_test
                COMMAND ${LIGHT_CARGO} test --workspace
                WORKING_DIRECTORY ${CMAKE_CURRENT_LIST_DIR}
        )
endif()
