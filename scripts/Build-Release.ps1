param(
    [Parameter(Mandatory=$true)][string]$RuntimeArchive,
    [Parameter(Mandatory=$true)][string]$Compiler,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\output\release-0.2.0')
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$payloadDir = Join-Path ([IO.Path]::GetFullPath($OutputDirectory)) 'payload'
New-Item -ItemType Directory -Path $payloadDir -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $repoRoot 'target\release\airbridge-ui.exe') -Destination (Join-Path $payloadDir 'AirBridge.exe')
Copy-Item -LiteralPath $RuntimeArchive -Destination (Join-Path $payloadDir 'airbridge-runtime.tar.gz')
Copy-Item -LiteralPath (Join-Path $repoRoot 'installer\dependencies.json') -Destination $payloadDir
Copy-Item -LiteralPath (Join-Path $repoRoot 'installer\Setup-Runtime.ps1') -Destination $payloadDir
Copy-Item -LiteralPath (Join-Path $repoRoot 'runtime\adapters.tsv') -Destination $payloadDir
$hash = (Get-FileHash -LiteralPath $RuntimeArchive -Algorithm SHA256).Hash.ToLowerInvariant()
@{ version='0.2.0'; kernel='6.18.33.2-microsoft-standard-WSL2'; sha256=$hash } |
    ConvertTo-Json | Set-Content -LiteralPath (Join-Path $payloadDir 'runtime-manifest.json') -Encoding UTF8
# Include license texts from the resolved dependency sources, including git deps.
$metadata = (& cargo metadata --locked --format-version 1 | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { throw 'Could not collect dependency licenses.' }
$notices = [Text.StringBuilder]::new()
$rustSysroot = (& rustc --print sysroot).Trim()
$rustLibraryNotices = Join-Path $rustSysroot 'share\doc\rust\COPYRIGHT-library.html'
if (-not (Test-Path -LiteralPath $rustLibraryNotices)) { throw 'The Rust toolchain library copyright notices are missing.' }
[void]$notices.AppendLine('Rust standard library copyright and licenses')
[void]$notices.AppendLine((Get-Content -LiteralPath $rustLibraryNotices -Raw))
foreach ($package in $metadata.packages | Sort-Object name,version) {
    [void]$notices.AppendLine("$($package.name) $($package.version) - $($package.license)")
    $directory = Split-Path $package.manifest_path -Parent
    $licenseFiles = @(Get-ChildItem -LiteralPath $directory -File -Recurse | Where-Object {
        $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)|license\.txt$|^(OFL|UFL)\.txt$' -or
        $_.Directory.Name -match '^LICEN[CS]ES?$'
    })
    if ($package.license_file) {
        $licenseFiles += Get-Item -LiteralPath (Join-Path $directory $package.license_file)
    }
    foreach ($file in $licenseFiles | Sort-Object FullName -Unique) {
        [void]$notices.AppendLine((Get-Content -LiteralPath $file.FullName -Raw))
    }
    [void]$notices.AppendLine()
}
[IO.File]::WriteAllText((Join-Path $payloadDir 'THIRD-PARTY-LICENSES.txt'), $notices.ToString())
& $Compiler ("/DPayloadDir=" + $payloadDir) (Join-Path $repoRoot 'installer\AirBridge.iss')
if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
