@echo off
echo ========================================================
echo          ZAPUSK pizdOS 2.0 (Alpine GNU-Free), NAHUY!
echo ========================================================

:: Ubivaem starye zavisnie processy QEMU
wsl pkill -9 -f qemu-system-x86_64 2>nul

if not exist build\pizdos2.iso (
    echo pizdos2.iso ne naiden, sobirayu...
    wsl bash /mnt/c/pizdOS/build_pizdos2.sh
)

echo Zapusk pizdOS 2.0 v QEMU (512MB RAM, GTK)...
wsl qemu-system-x86_64 -cdrom /mnt/c/pizdOS/build/pizdos2.iso -m 512M -net nic -net user -display gtk,zoom-to-fit=on
if %ERRORLEVEL% NEQ 0 (
    echo Ne smoglo v GTK, zapuskayu v curses...
    wsl qemu-system-x86_64 -cdrom /mnt/c/pizdOS/build/pizdos2.iso -m 512M -net nic -net user -display curses
)
