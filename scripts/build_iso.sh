#!/usr/bin/env bash
set -e

echo "=========================================================="
echo "  SBORKA pizdOS 2.1(00) Live ISO (Alpine GNU-Free + DE)   "
echo "=========================================================="

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASE_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$BASE_DIR/build"
OVERLAY_DIR="$BASE_DIR/os2/overlay"
ISO_WORK="$BUILD_DIR/iso_work"
ISO_OUTPUT="$BUILD_DIR/pizdos2.iso"
ALPINE_ISO="$BUILD_DIR/alpine-virt.iso"

mkdir -p "$BUILD_DIR"

# 1. Скачиваем базовый образ Alpine virt если его нет
if [ ! -f "$ALPINE_ISO" ]; then
    echo "Skachivayu Alpine virt ISO..."
    curl -L -o "$ALPINE_ISO" "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-virt-3.21.3-x86_64.iso"
fi

# 2. Всегда пересобираем static musl piz-get (или берем предсобранный при SKIP_RUST_BUILD=1)
PIZGET_SRC="$BASE_DIR/piz-get"
PIZGET_BIN="$PIZGET_SRC/target/x86_64-unknown-linux-musl/release/piz-get"

if [ "${SKIP_RUST_BUILD:-0}" != "1" ] || [ ! -f "$PIZGET_BIN" ]; then
    echo "Sobirayu static musl piz-get..."
    cd "$PIZGET_SRC"
    source "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release --target x86_64-unknown-linux-musl
fi

# 3. Всегда пересобираем static musl pizsoft-ui
PIZUI_SRC="$BASE_DIR/pizsoft-ui"
PIZUI_BIN="$PIZUI_SRC/target/x86_64-unknown-linux-musl/release/pizsoft"

if [ "${SKIP_RUST_BUILD:-0}" != "1" ] || [ ! -f "$PIZUI_BIN" ]; then
    echo "Sobirayu static musl pizsoft-ui..."
    cd "$PIZUI_SRC"
    source "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release --target x86_64-unknown-linux-musl
fi

# Копируем бинарники в оверлей
mkdir -p "$OVERLAY_DIR/usr/bin"
mkdir -p "$OVERLAY_DIR/bin"
cp -f "$PIZGET_BIN" "$OVERLAY_DIR/usr/bin/piz-get"
cp -f "$PIZUI_BIN" "$OVERLAY_DIR/usr/bin/pizsoft"

# Убираем все CRLF из текстовых файлов намертво
find "$OVERLAY_DIR" -type f -exec dos2unix -q {} + 2>/dev/null || true

chmod +x "$OVERLAY_DIR/usr/bin/piz-get" "$OVERLAY_DIR/usr/bin/pizsoft"
chmod +x "$OVERLAY_DIR/bin/"* 2>/dev/null || true
chmod +x "$OVERLAY_DIR/usr/bin/"* 2>/dev/null || true
chmod +x "$OVERLAY_DIR/etc/local.d/"* 2>/dev/null || true

# 4. Упаковываем apkovl оверлей
echo "Upakovka apkovl..."
cd "$OVERLAY_DIR"
tar -czf "$BUILD_DIR/pizdos.apkovl.tar.gz" *

# 5. Распаковываем базовый ISO
echo "Podgotovka ISO struktury..."
rm -rf "$ISO_WORK"
mkdir -p "$ISO_WORK"
7z x -y "$ALPINE_ISO" -o"$ISO_WORK" > /dev/null

# Внедряем оверлеи
cp -f "$BUILD_DIR/pizdos.apkovl.tar.gz" "$ISO_WORK/pizdos.apkovl.tar.gz"
cp -f "$BUILD_DIR/pizdos.apkovl.tar.gz" "$ISO_WORK/localhost.apkovl.tar.gz"

# 6. Настройка мгновенной автозагрузки (без зависаний)
mkdir -p "$ISO_WORK/boot/syslinux"
mkdir -p "$ISO_WORK/isolinux"

cat > "$ISO_WORK/boot/syslinux/syslinux.cfg" << 'EOF'
TIMEOUT 1
PROMPT 0
DEFAULT virt

LABEL virt
MENU LABEL pizdOS 2.1(00) (Alpine GNU-Free + Pizsoft DE)
KERNEL /boot/vmlinuz-virt
INITRD /boot/initramfs-virt
APPEND modules=loop,squashfs,sd-mod,usb-storage,fbcon,bochs_drm,vesafb,efifb,evdev console=tty0 vga=792 quiet
EOF

dos2unix -q "$ISO_WORK/boot/syslinux/syslinux.cfg" 2>/dev/null || true
cp -f "$ISO_WORK/boot/syslinux/syslinux.cfg" "$ISO_WORK/boot/syslinux/isolinux.cfg"
cp -f "$ISO_WORK/boot/syslinux/syslinux.cfg" "$ISO_WORK/isolinux/isolinux.cfg"
cp -f "$ISO_WORK/boot/syslinux/syslinux.cfg" "$ISO_WORK/isolinux.cfg"

if [ -f "$ISO_WORK/boot/grub/grub.cfg" ]; then
    sed -i 's/Alpine Linux/pizdOS 2.1(00) (PDOS 2.1)/g' "$ISO_WORK/boot/grub/grub.cfg"
    dos2unix -q "$ISO_WORK/boot/grub/grub.cfg" 2>/dev/null || true
fi

# 7. Сборка bootable ISO через xorriso
echo "Generatsiya $ISO_OUTPUT..."
xorriso -as mkisofs \
  -r -V "PIZDOS2" \
  -J -joliet-long \
  -b boot/syslinux/isolinux.bin \
  -c boot/syslinux/boot.cat \
  -no-emul-boot -boot-load-size 4 -boot-info-table \
  -eltorito-alt-boot \
  -e boot/grub/efi.img \
  -no-emul-boot \
  -isohybrid-gpt-basdat \
  -o "$ISO_OUTPUT" \
  "$ISO_WORK"

echo "=========================================================="
echo "   pizdOS 2.1(00) Live ISO USPESHNO GOTOV: $ISO_OUTPUT   "
echo "=========================================================="
