cmake_minimum_required(VERSION 3.20)
project(sen66_core_tests LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

include(FetchContent)
FetchContent_Declare(
  doctest
  URL https://github.com/doctest/doctest/archive/refs/tags/v2.5.3.tar.gz
  URL_HASH SHA256=174ebc4e769928959614789c5b4e9c3d0a0f81a62bb608756b127bfebfb21331
  DOWNLOAD_EXTRACT_TIMESTAMP TRUE
)
FetchContent_MakeAvailable(doctest)

set(CORE_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../src/core)

add_executable(core_tests
  test_aggregator.cpp
  test_json.cpp
  test_device_status.cpp
  ${CORE_DIR}/aggregator.cpp
  ${CORE_DIR}/json.cpp
  ${CORE_DIR}/device_status.cpp
)
target_include_directories(core_tests PRIVATE ${CORE_DIR})
target_link_libraries(core_tests PRIVATE doctest::doctest_with_main)
target_compile_options(core_tests PRIVATE -Wall -Wextra -Werror)

enable_testing()
add_test(NAME core_tests COMMAND core_tests)
