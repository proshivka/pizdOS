@echo off
echo ========================================================
echo               ZAPUSK pizdOS v1.0.0, NAHUY!
echo ========================================================

:: Ubivaem starye zavisnie processy QEMU
wsl pkill -9 -f qemu-system-x86_64 2>nul

wsl make -C /mnt/c/pizdOS all
if %ERRORLEVEL% NEQ 0 (
    echo Sborocha naebnulas, prover oshibki!
    pause
    exit /b %ERRORLEVEL%
)

echo Zapuskayem v QEMU (GTK s zoom-to-fit dlya lyubogo razresheniya)...
wsl qemu-system-x86_64 -drive format=raw,file=/mnt/c/pizdOS/build/pizdos.img,file.locking=off -display gtk,zoom-to-fit=on
if %ERRORLEVEL% NEQ 0 (
    echo Ne smoglo v GTK, probuyu v curses pryamo v etom okne...
    wsl qemu-system-x86_64 -drive format=raw,file=/mnt/c/pizdOS/build/pizdos.img,file.locking=off -display curses
)
