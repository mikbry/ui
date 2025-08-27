# Makefile for mkui C example
TARGET = mkui_c_example
CC = gcc
CFLAGS = -std=c11 -Wall -Wextra -O2

# Paths
MKUI_ROOT = ../..
MKUI_C_TARGET = $(MKUI_ROOT)/target/release
MKUI_C_INCLUDE = $(MKUI_ROOT)/crates/mkui-c/include

# Source files
SOURCES = main.c

# Library paths and names
LDFLAGS = -L$(MKUI_C_TARGET)
LIBS = -lmkui_c -ldl -lpthread -lm

# Include paths
INCLUDES = -I$(MKUI_C_INCLUDE)

# Default target
all: $(TARGET)

# Build the Rust library first
$(MKUI_C_TARGET)/libmkui_c.a:
	cd $(MKUI_ROOT) && cargo build --release -p mkui-c

# Generate the C header
$(MKUI_C_INCLUDE)/mkui_c.h: $(MKUI_C_TARGET)/libmkui_c.a
	# Header is generated as part of the build process

# Build the C example
$(TARGET): $(SOURCES) $(MKUI_C_TARGET)/libmkui_c.a $(MKUI_C_INCLUDE)/mkui_c.h
	$(CC) $(CFLAGS) $(INCLUDES) $(SOURCES) $(LDFLAGS) $(LIBS) -o $(TARGET)

# Clean build artifacts
clean:
	rm -f $(TARGET)

# Clean everything including Rust artifacts
clean-all: clean
	cd $(MKUI_ROOT) && cargo clean

# Run the example
run: $(TARGET)
	./$(TARGET)

# Install system dependencies (Ubuntu/Debian)
install-deps-ubuntu:
	sudo apt-get update
	sudo apt-get install -y build-essential gcc libc6-dev

# Install system dependencies (macOS)
install-deps-macos:
	# Assumes Xcode command line tools are installed
	@echo "Make sure Xcode command line tools are installed:"
	@echo "xcode-select --install"

.PHONY: all clean clean-all run install-deps-ubuntu install-deps-macos