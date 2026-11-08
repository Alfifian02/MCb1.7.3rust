# Host build without CMake: make test | make shot
CXX ?= g++
CXXFLAGS ?= -std=c++17 -O2 -Wall -Wextra -pthread
CORE = src/core/noise.cpp src/core/terrain.cpp src/core/caves.cpp src/core/mesher.cpp src/core/streamer.cpp src/core/player.cpp
BIN = build
$(BIN)/%: src/tools/%.cpp $(CORE) $(wildcard src/core/*.hpp)
	@mkdir -p $(BIN)
	$(CXX) $(CXXFLAGS) -o $@ $< $(CORE)
test: $(BIN)/golden_test $(BIN)/sim_test
	$(BIN)/golden_test && $(BIN)/sim_test
shot: $(BIN)/softshot
	$(BIN)/softshot 12345 30 -12 shot.ppm
clean:
	rm -rf $(BIN) shot.ppm
.PHONY: test shot clean
