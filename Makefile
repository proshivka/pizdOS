ASM = nasm
BUILD_DIR = build
SRC_DIR = src
IMG = $(BUILD_DIR)/pizdos.img
ISO2 = $(BUILD_DIR)/pizdos2.iso

all: $(IMG)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

# --- pizdOS 1.0 (ASM 16-bit) ---
$(BUILD_DIR)/bootloader.bin: $(SRC_DIR)/bootloader.asm | $(BUILD_DIR)
	$(ASM) -f bin $(SRC_DIR)/bootloader.asm -o $(BUILD_DIR)/bootloader.bin

$(BUILD_DIR)/kernel.bin: $(SRC_DIR)/kernel.asm | $(BUILD_DIR)
	$(ASM) -f bin $(SRC_DIR)/kernel.asm -o $(BUILD_DIR)/kernel.bin

$(IMG): $(BUILD_DIR)/bootloader.bin $(BUILD_DIR)/kernel.bin
	cat $(BUILD_DIR)/bootloader.bin $(BUILD_DIR)/kernel.bin > $(IMG)
	truncate -s 1474560 $(IMG)

run: $(IMG)
	qemu-system-x86_64 -drive format=raw,file=$(IMG),file.locking=off -display gtk,zoom-to-fit=on

run-curses: $(IMG)
	qemu-system-x86_64 -drive format=raw,file=$(IMG),file.locking=off -display curses

# --- pizdOS 2.0 (Alpine GNU-Free Live ISO) ---
os2-iso:
	bash scripts/build_iso.sh

run-os2: $(ISO2)
	qemu-system-x86_64 -cdrom $(ISO2) -m 512M -vga std -usb -device usb-tablet -net nic -net user -display gtk,zoom-to-fit=on

run-os2-curses: $(ISO2)
	qemu-system-x86_64 -cdrom $(ISO2) -m 512M -vga std -net nic -net user -display curses

clean:
	rm -rf $(BUILD_DIR)

.PHONY: all run run-curses os2-iso run-os2 run-os2-curses clean
