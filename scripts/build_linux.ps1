$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent $PSScriptRoot
$Sysroot = (& rustc --print sysroot).Trim()
$Lld = Join-Path $Sysroot "lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe"

if (-not (Test-Path $Lld)) {
    throw "rust-lld.exe not found at $Lld. Ensure stable-x86_64-pc-windows-msvc toolchain is installed."
}

$Targets = (& rustup target list --installed)
if (-not ($Targets -contains "x86_64-unknown-linux-musl")) {
    Write-Host "Installing x86_64-unknown-linux-musl target..."
    & rustup target add x86_64-unknown-linux-musl
}

$PrevRustFlags = $env:RUSTFLAGS
$env:RUSTFLAGS = "-C linker=$Lld -C linker-flavor=ld.lld -C link-self-contained=yes"

try {
    Write-Host "=========================================================="
    Write-Host "  Building static Linux musl binaries (pizdOS 2.1(00))    "
    Write-Host "=========================================================="

    Push-Location (Join-Path $RootDir "piz-get")
    & cargo build --release --target x86_64-unknown-linux-musl
    if ($LASTEXITCODE -ne 0) { throw "piz-get musl build failed" }
    Pop-Location

    Push-Location (Join-Path $RootDir "pizsoft-ui")
    & cargo build --release --target x86_64-unknown-linux-musl
    if ($LASTEXITCODE -ne 0) { throw "pizsoft-ui musl build failed" }
    Pop-Location

    $OverlayBin = Join-Path $RootDir "os2\overlay\usr\bin"
    New-Item -ItemType Directory -Force -Path $OverlayBin | Out-Null

    $PizGetSrc = Join-Path $RootDir "piz-get\target\x86_64-unknown-linux-musl\release\piz-get"
    $PizUiSrc = Join-Path $RootDir "pizsoft-ui\target\x86_64-unknown-linux-musl\release\pizsoft"
    $PizGetDst = Join-Path $OverlayBin "piz-get"
    $PizUiDst = Join-Path $OverlayBin "pizsoft"

    Copy-Item -Force $PizGetSrc $PizGetDst
    Copy-Item -Force $PizUiSrc $PizUiDst

    foreach ($Bin in @($PizGetDst, $PizUiDst)) {
        $Magic = [System.IO.File]::ReadAllBytes($Bin)[0..3]
        if ($Magic[0] -ne 0x7F -or $Magic[1] -ne 0x45 -or $Magic[2] -ne 0x4C -or $Magic[3] -ne 0x46) {
            throw "$Bin is not a valid ELF binary!"
        }
        $SizeKb = [Math]::Round((Get-Item $Bin).Length / 1024.0, 1)
        Write-Host "OK: $Bin (ELF x86_64 static musl, ${SizeKb} KB)"
    }
} finally {
    if ($null -eq $PrevRustFlags) {
        Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    } else {
        $env:RUSTFLAGS = $PrevRustFlags
    }
}
