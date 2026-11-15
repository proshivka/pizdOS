$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent $PSScriptRoot
$BuildDir = Join-Path $RootDir "build"
$IsoWork = Join-Path $BuildDir "iso_std_work"
$AlpineIso = Join-Path $BuildDir "alpine-standard.iso"
$OverlayDir = Join-Path $RootDir "os2\overlay"
$Qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"

if (-not (Test-Path $Qemu)) {
    throw "QEMU not found at $Qemu"
}

New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null

# 1. Download Alpine Standard ISO if missing (includes full fbdev/bochs/evdev drivers)
if (-not (Test-Path $AlpineIso)) {
    Write-Host "Downloading Alpine Standard ISO..."
    & curl.exe -L -o $AlpineIso "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/alpine-standard-3.21.3-x86_64.iso"
    if ($LASTEXITCODE -ne 0) { throw "Failed to download Alpine ISO" }
}

# 2. Extract ISO using built-in Windows bsdtar (MIT/BSD, no 7z/GPL, no WSL)
if (-not (Test-Path (Join-Path $IsoWork "boot\vmlinuz-lts"))) {
    Write-Host "Extracting Alpine Standard ISO with bsdtar..."
    New-Item -ItemType Directory -Force -Path $IsoWork | Out-Null
    & tar.exe -xf $AlpineIso -C $IsoWork
    if ($LASTEXITCODE -ne 0) { throw "Failed to extract Alpine ISO" }
}

# 3. Ensure static musl binaries exist in overlay
$PizSoftBin = Join-Path $OverlayDir "usr\bin\pizsoft"
$PizGetBin = Join-Path $OverlayDir "usr\bin\piz-get"
if (-not (Test-Path $PizSoftBin) -or -not (Test-Path $PizGetBin)) {
    & (Join-Path $PSScriptRoot "build_linux.ps1")
}

# 4. Normalize LF line endings on shell/config scripts & pack apkovl overlay using bsdtar
$TextDirs = @("bin", "etc")
foreach ($Sub in $TextDirs) {
    $Dir = Join-Path $OverlayDir $Sub
    if (Test-Path $Dir) {
        Get-ChildItem -Path $Dir -Recurse -File | ForEach-Object {
            $raw = [System.IO.File]::ReadAllText($_.FullName)
            if ($raw.Contains("`r`n")) {
                [System.IO.File]::WriteAllText($_.FullName, $raw.Replace("`r`n", "`n"), (New-Object System.Text.UTF8Encoding $false))
            }
        }
    }
}
$PizFetch = Join-Path $OverlayDir "usr\bin\pizfetch"
if (Test-Path $PizFetch) {
    $raw = [System.IO.File]::ReadAllText($PizFetch)
    if ($raw.Contains("`r`n")) {
        [System.IO.File]::WriteAllText($PizFetch, $raw.Replace("`r`n", "`n"), (New-Object System.Text.UTF8Encoding $false))
    }
}
Write-Host "Packing pizdos.apkovl.tar.gz..."
$Apkovl = Join-Path $IsoWork "pizdos.apkovl.tar.gz"
Push-Location $OverlayDir
& tar.exe -czf $Apkovl *
$TarExit = $LASTEXITCODE
Pop-Location
if ($TarExit -ne 0) { throw "Failed to pack apkovl" }

$Kernel = Join-Path $IsoWork "boot\vmlinuz-lts"
$Initrd = Join-Path $IsoWork "boot\initramfs-lts"
$Append = "alpine_dev=sda1:vfat modloop=boot/modloop-lts modules=loop,squashfs,sd-mod,usb-storage,vfat,fbcon,bochs,vesafb,evdev console=tty0 console=ttyS0"

Write-Host "Booting pizdOS 2.1(00) in QEMU 11.1.0 (WHPX)..."
& $Qemu `
    -accel whpx `
    -smp 4 `
    -m 512M `
    -kernel $Kernel `
    -initrd $Initrd `
    -append $Append `
    -drive "file=fat:rw:$IsoWork,format=raw" `
    -vga std `
    -usb -device usb-tablet `
    -net nic -net user `
    -serial stdio `
    -display sdl
