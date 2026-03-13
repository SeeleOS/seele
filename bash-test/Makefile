# --- Paths ---
RELIBC_PATH = ../relibc/target/x86_64-unknown-linux-gnu/release
RELIBC_INCLUDE = ../relibc/target/x86_64-unknown-linux-gnu/include
BUILD_DIR = build
TARGET = bash-test.elf
FULL_TARGET = $(BUILD_DIR)/$(TARGET)
SYSROOT = "../sysroot"
TARGET_DIR = $(SYSROOT)/programs

# --- Toolchain ---
CC = x86_64-elf-gcc
LD = x86_64-elf-ld
CFLAGS = -ffreestanding -mno-sse -D__linux__ -mno-red-zone \
         -fno-stack-protector -fno-builtin -fno-pie -no-pie \
         -nostdinc -I$(RELIBC_INCLUDE) -I.
LDFLAGS = -static -nostdlib --allow-multiple-definition

ALL_C_SRCS = $(shell find . -name "*.c")
OBJS = $(addprefix $(BUILD_DIR)/, $(ALL_C_SRCS:.c=.o))

all: clean $(FULL_TARGET) install

$(BUILD_DIR)/%.o: %.c
	@mkdir -p $(dir $@)
	@echo "Compiling $<..."
	@$(CC) $(CFLAGS) -c $< -o $@

$(FULL_TARGET): $(OBJS)
	@echo "Linking $@..."
	@$(LD) $(LDFLAGS) \
	    $(RELIBC_PATH)/crt0.o \
	    $(RELIBC_PATH)/crti.o \
	    $(OBJS) \
	    $(RELIBC_PATH)/libc.a \
	    $(RELIBC_PATH)/crtn.o \
	    -o $@

clean:
	rm -rf $(BUILD_DIR)

install: $(FULL_TARGET)
	@echo "Installing $< to $(TARGET_DIR)/$(TARGET) ..."
	@mkdir -p $(TARGET_DIR)
	@sudo rm -f $(TARGET_DIR)/$(TARGET)
	@sudo cp $< $(TARGET_DIR)/$(TARGET)
	@sync

.PHONY: all clean install
