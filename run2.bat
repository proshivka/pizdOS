@echo off
setlocal
set "ROOT_DIR=%~dp0"
set "ROOT_DIR=%ROOT_DIR:\=/%"

echo ========================================================
echo       ZAPUSK pizdOS 2.1(00) (Alpine GNU-Free + DE)!
echo ========================================================

:: Ubivaem starye zavisnie processy QEMU
taskkill /F /IM qemu-system-x86_64.exe >nul 2>nul

if not exist "%~dp0build\pizdos2.iso" (
    echo pizdos2.iso ne naiden, zapuskayu sborku cherez scripts/build_iso.sh...
    wsl bash -c "cd \"$(wslpath -a '%ROOT_DIR%')\" && bash scripts/build_iso.sh"
)

set "QEMU_WIN=C:\Program Files\qemu\qemu-system-x86_64.exe"
if exist "%QEMU_WIN%" (
    echo Zapusk pizdOS 2.1^(00^) v nativnom Windows QEMU ^(512MB RAM, std VGA /dev/fb0, USB Tablet^)...
    "%QEMU_WIN%" -accel whpx -smp 4 -cdrom "%~dp0build\pizdos2.iso" -m 512M -vga std -usb -device usb-tablet -net nic -net user -display gtk,zoom-to-fit=on
    exit /b %ERRORLEVEL%
)

echo Zapusk pizdOS 2.1(00) v WSL QEMU (512MB RAM, std VGA /dev/fb0, USB Tablet)...
wsl bash -c "qemu-system-x86_64 -cdrom \"$(wslpath -a '%ROOT_DIR%build/pizdos2.iso')\" -m 512M -vga std -device usb-tablet -net nic -net user -display gtk,zoom-to-fit=on"
if %ERRORLEVEL% NEQ 0 (
    echo Ne smoglo v GTK, zapuskayu v curses pryamo v etom okne...
    wsl bash -c "qemu-system-x86_64 -cdrom \"$(wslpath -a '%ROOT_DIR%build/pizdos2.iso')\" -m 512M -vga std -net nic -net user -display curses"
)
