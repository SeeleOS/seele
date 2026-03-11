RELIBC_PATH = ../relibc/target/x86_64-unknown-linux-gnu/release
RELIBC_INCLUDE = ../relibc/target/x86_64-unknown-linux-gnu/include
BUILD_DIR = build
TARGET = posix-tests.elf
FULL_TARGET = $(BUILD_DIR)/$(TARGET)
SYSROOT = "../sysroot"
TARGET_DIR = $(SYSROOT)/programs

CC = x86_64-elf-gcc
LD = x86_64-elf-ld
CFLAGS = -ffreestanding -mno-sse -D__linux__ -mno-red-zone \
         -fno-stack-protector -fno-builtin -fno-pie -no-pie \
         -nostdinc -I$(RELIBC_INCLUDE)
LDFLAGS = -static -nostdlib --allow-multiple-definition

C_SRCS = test.c
OBJS = $(addprefix $(BUILD_DIR)/, $(C_SRCS:.c=.o))

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
	    $(RELIBC_PATH)/librelibc.a \
	    $(RELIBC_PATH)/crtn.o \
	    -o $@
	@echo "Build Done. Entry Point:"
	@readelf -h $@ | grep Entry

clean:
	rm -rf $(BUILD_DIR)

install: $(FULL_TARGET)
	@echo "Installing $< to $(TARGET_DIR)/$(TARGET) ..."
	@mkdir -p $(TARGET_DIR)
	@sudo rm -f $(TARGET_DIR)/$(TARGET)
	@sudo cp $< $(TARGET_DIR)/$(TARGET)
	@echo "Syncing filesystem to disk..."
	@sync
	@ORIG_SIZE=$$(stat -c%s "$(FULL_TARGET)"); \
	 INST_SIZE=$$(sudo stat -c%s "$(TARGET_DIR)/$(TARGET)"); \
	 echo "Local: $$ORIG_SIZE bytes | Installed: $$INST_SIZE bytes"; \
	 if [ "$$ORIG_SIZE" -ne "$$INST_SIZE" ]; then \
	     echo "\033[0;31m[ERROR]\033[0m: Size mismatch! File might be corrupted."; \
	     exit 1; \
	 else \
	     echo "\033[0;32m[SUCCESS]\033[0m: Installation verified."; \
	 fi

.PHONY: all clean install
