@echo off
echo ========================================================
echo               ZAPUSK pizdOS v1.0.0, NAHUY!
echo ========================================================

wsl make -C /mnt/c/pizdOS all
if %ERRORLEVEL% NEQ 0 (
    echo Sborocha naebnulas, prover oshibki!
    pause
    exit /b %ERRORLEVEL%
)

echo Zapuskayem v QEMU s graficheskim oknom...
wsl qemu-system-x86_64 -drive format=raw,file=/mnt/c/pizdOS/build/pizdos.img -display gtk
if %ERRORLEVEL% NEQ 0 (
    echo Ne smoglo v GTK, probuyu v curses pryamo v etom okne...
    wsl qemu-system-x86_64 -drive format=raw,file=/mnt/c/pizdOS/build/pizdos.img -display curses
)
