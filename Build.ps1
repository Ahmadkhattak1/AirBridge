param([switch]$Release, [switch]$SkipTests, [switch]$AppOnly)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$toolchain = (& rustc --print sysroot)
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = Join-Path $toolchain 'lib\rustlib\x86_64-pc-windows-msvc\bin\rust-lld.exe'
$sdk = Join-Path $root '.tools\xwin-sdk'
if (-not (Test-Path -LiteralPath $sdk)) { throw 'The local Windows SDK kit is missing from .tools\xwin-sdk.' }
$env:LIB = @(
    (Join-Path $sdk 'crt\lib\x86_64'),
    (Join-Path $sdk 'sdk\lib\ucrt\x86_64'),
    (Join-Path $sdk 'sdk\lib\um\x86_64')
) -join ';'
$zig = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter zig.exe -File -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $zig) { throw 'Zig is required to build AirBridge cryptography. Install package zig.zig.' }
$env:PATH = $zig.DirectoryName + ';' + $env:PATH
$env:CC_x86_64_pc_windows_msvc = $zig.FullName + ' cc -target x86_64-windows-gnu'
$env:AR_x86_64_pc_windows_msvc = Join-Path $root 'scripts\zig-lib-wrapper.cmd'
$env:CRATE_CC_NO_DEFAULTS = '1'
$env:CFLAGS_x86_64_pc_windows_msvc = '-fno-sanitize=all -fno-stack-protector'
$env:CARGO_INCREMENTAL = '0'
& cargo fmt --all --check
if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed.' }
if (-not $SkipTests) {
    & cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
}
$profile = if ($Release) { '--release' } else { $null }
if ($AppOnly) {
    & cargo build -p airbridge-app $profile
} else {
    & cargo build --workspace $profile
}
if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
