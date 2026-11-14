#!/usr/bin/env bash
set -e

echo "=== SBORKA pizdOS 2.0 (Alpine GNU-Free Edition) ==="

BASE_DIR="/mnt/c/pizdOS"
BUILD_DIR="$BASE_DIR/build"
OVERLAY_DIR="$BASE_DIR/pizdos2/overlay"
ISO_WORK="$BUILD_DIR/iso_work"

mkdir -p "$OVERLAY_DIR/etc/profile.d"
mkdir -p "$OVERLAY_DIR/usr/bin"
mkdir -p "$OVERLAY_DIR/etc/local.d"

# 1. /etc/issue
cat > "$OVERLAY_DIR/etc/issue" << 'EOF'
  ______ _____ ______ _____   ____   _____    ___     ___  
 |  __ \_   _|___  /|  __ \ / __ \ / ____|  |__ \   / _ \ 
 | |__) || |    / / | |  | | |  | | (___       ) | | | | |
 |  ___/ | |   / /  | |  | | |  | |\___ \     / /  | | | |
 | |    _| |_ / /__ | |__| | |__| |____) |   / /_  | |_| |
 |_|   |_____/_____|_____/ \____/|_____/   |____|  \___/ 

 === [ pizdOS 2.0 (PDOS 2) - Alpine GNU-Free Edition ] ===
 Pizsoft Corporation (c) 2026. Nahuy Windows, nahuy GNU!

EOF

# 2. /etc/motd
cat > "$OVERLAY_DIR/etc/motd" << 'EOF'
================================================================
           DOBRO POZHALOVAT V pizdOS 2.0, BRATUKHA!
================================================================
 * Paketny menedzher : piz-get (ebash <pkg>, snosi <pkg>, obnovi)
 * Poyasnit za sistemu: pizfetch (ili poyasni)
 * Ochistit ekran    : nahuy
 * Vyklyuchit komp   : vpizdu
================================================================
EOF

# 3. /etc/hostname
echo "pizdos" > "$OVERLAY_DIR/etc/hostname"

# 4. /etc/profile.d/pizdos.sh
cat > "$OVERLAY_DIR/etc/profile.d/pizdos.sh" << 'EOF'
export PATH="/bin:/sbin:/usr/bin:/usr/sbin:/usr/local/bin:$PATH"
export PS1='\[\033[1;31m\]pizdOS-2.0\[\033[0m\]:\[\033[1;32m\]\w\[\033[0m\]# '

alias nahuy='clear'
alias poyasni='pizfetch'
alias ebash='piz-get ebash'
alias snosi='piz-get snosi'
alias obnovi='piz-get obnovi'
alias vpizdu='poweroff'

if [ -t 0 ]; then
    /usr/bin/pizfetch
fi
EOF

# 5. /usr/bin/pizfetch
cat > "$OVERLAY_DIR/usr/bin/pizfetch" << 'EOF'
#!/bin/sh
echo -e "\033[1;31m"
cat << 'ART'
  ______ _____ ______ _____   ____   _____    ___     ___  
 |  __ \_   _|___  /|  __ \ / __ \ / ____|  |__ \   / _ \ 
 | |__) || |    / / | |  | | |  | | (___       ) | | | | |
 |  ___/ | |   / /  | |  | | |  | |\___ \     / /  | | | |
 | |    _| |_ / /__ | |__| | |__| |____) |   / /_  | |_| |
 |_|   |_____/_____|_____/ \____/|_____/   |____|  \___/ 
ART
echo -e "\033[1;33m=== [ pizdOS 2.0 (Alpine GNU-Free Edition) ] ===\033[0m"
echo -e "\033[1;32mOS       :\033[0m pizdOS 2.0 (musl x86_64, GNU-Free)"
echo -e "\033[1;32mKernel   :\033[0m $(uname -s -r -m)"
echo -e "\033[1;32mPackage  :\033[0m piz-get 0.1.0 (Pure Rust Zero-Dep)"
echo -e "\033[1;32mUptime   :\033[0m $(uptime | sed 's/.*up \([^,]*\), .*/\1/')"
echo -e "\033[1;32mMemory   :\033[0m $(free -m | awk '/Mem:/ {print $3 "MB / " $2 "MB"}')"
echo -e "\033[1;32mDev Team :\033[0m proshivka, Lowyk & Antigravity"
echo -e "\033[1;35mBazar    :\033[0m Nahuy Windows, nahuy GNU!"
echo "--------------------------------------------------------"
EOF
chmod +x "$OVERLAY_DIR/usr/bin/pizfetch"

# 6. Вшиваем static musl piz-get от Ловика
cp "$BASE_DIR/piz-get/target/x86_64-unknown-linux-musl/release/piz-get" "$OVERLAY_DIR/usr/bin/piz-get"
chmod +x "$OVERLAY_DIR/usr/bin/piz-get"

# 7. /etc/inittab (автологин в root без пароля на tty1)
cat > "$OVERLAY_DIR/etc/inittab" << 'EOF'
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
::wait:/sbin/openrc default

# Avtologin root na tty1
tty1::respawn:/sbin/getty -n -l /bin/sh 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3

::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
EOF

# 8. Упаковываем apkovl
echo "Upakovyvaem pizdos.apkovl.tar.gz..."
cd "$OVERLAY_DIR"
tar -czf "$BUILD_DIR/pizdos.apkovl.tar.gz" *

# 9. Распаковываем базовый ISO Alpine
echo "Podgotovka ISO obraz..."
rm -rf "$ISO_WORK"
mkdir -p "$ISO_WORK"
7z x -y "$BUILD_DIR/alpine-virt.iso" -o"$ISO_WORK" > /dev/null

# Копируем оверлей в корень ISO
cp "$BUILD_DIR/pizdos.apkovl.tar.gz" "$ISO_WORK/pizdos.apkovl.tar.gz"
cp "$BUILD_DIR/pizdos.apkovl.tar.gz" "$ISO_WORK/localhost.apkovl.tar.gz"

# Кастомизируем bootloader configs
if [ -f "$ISO_WORK/boot/syslinux/syslinux.cfg" ]; then
    sed -i 's/Alpine Linux/pizdOS 2.0 (PDOS 2)/g' "$ISO_WORK/boot/syslinux/syslinux.cfg"
fi

if [ -f "$ISO_WORK/boot/grub/grub.cfg" ]; then
    sed -i 's/Alpine Linux/pizdOS 2.0 (PDOS 2)/g' "$ISO_WORK/boot/grub/grub.cfg"
fi

# 10. Собираем финальный bootable ISO с помощью xorriso
echo "Sborka pizdos2.iso cherez xorriso..."
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
  -o "$BUILD_DIR/pizdos2.iso" \
  "$ISO_WORK"

echo "=== pizdOS 2.0 USPESHNO SOBRAN: $BUILD_DIR/pizdos2.iso ==="
