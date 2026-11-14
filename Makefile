ASM = nasm
BUILD_DIR = build
SRC_DIR = src
IMG = $(BUILD_DIR)/pizdos.img

all: $(IMG)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/bootloader.bin: $(SRC_DIR)/bootloader.asm | $(BUILD_DIR)
	$(ASM) -f bin $(SRC_DIR)/bootloader.asm -o $(BUILD_DIR)/bootloader.bin

$(BUILD_DIR)/kernel.bin: $(SRC_DIR)/kernel.asm | $(BUILD_DIR)
	$(ASM) -f bin $(SRC_DIR)/kernel.asm -o $(BUILD_DIR)/kernel.bin

$(IMG): $(BUILD_DIR)/bootloader.bin $(BUILD_DIR)/kernel.bin
	cat $(BUILD_DIR)/bootloader.bin $(BUILD_DIR)/kernel.bin > $(IMG)
	truncate -s 1474560 $(IMG)

run: $(IMG)
	qemu-system-x86_64 -drive format=raw,file=$(IMG)

run-curses: $(IMG)
	qemu-system-x86_64 -drive format=raw,file=$(IMG) -display curses

clean:
	rm -rf $(BUILD_DIR)

.PHONY: all run run-curses clean
