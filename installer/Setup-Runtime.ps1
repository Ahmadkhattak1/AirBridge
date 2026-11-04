param(
    [string]$InstallRoot = $PSScriptRoot,
    [string]$DistroName = 'AirBridgeRuntime',
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$script:restartNeeded = $false
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$setupHome = Join-Path $env:LOCALAPPDATA 'AirBridge'
New-Item -ItemType Directory -Path $setupHome -Force | Out-Null
$resultPath = Join-Path $setupHome 'setup-result.txt'
$logPath = Join-Path $setupHome 'setup.log'

function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $algorithm.Dispose(); $stream.Dispose() }
}

function Assert-Hash([string]$Path, [string]$Expected) {
    if ($Expected -notmatch '^[a-fA-F0-9]{64}$' -or
        (Get-Sha256 $Path) -ne $Expected) {
        throw "The downloaded file failed verification: $([IO.Path]::GetFileName($Path)). Run Setup again."
    }
}

function Get-Dependency($Entry) {
    if ($Entry.file -notmatch '^[a-zA-Z0-9.-]+$' -or $Entry.url -notlike 'https://*') {
        throw 'Invalid dependency manifest.'
    }
    $cache = Join-Path $setupHome 'setup-cache'
    New-Item -ItemType Directory -Path $cache -Force | Out-Null
    $path = Join-Path $cache $Entry.file
    if (-not (Test-Path -LiteralPath $path) -or
        (Get-Sha256 $path) -ne $Entry.sha256) {
        $client = [Net.WebClient]::new()
        try { $client.DownloadFile($Entry.url, $path) }
        finally { $client.Dispose() }
    }
    Assert-Hash $path $Entry.sha256
    return $path
}

function Invoke-Elevated([string]$Program, [string[]]$Arguments) {
    $process = Start-Process -FilePath $Program -ArgumentList $Arguments -Verb RunAs -Wait -PassThru
    if ($process.ExitCode -in 1641,3010) { $script:restartNeeded = $true; return }
    if ($process.ExitCode -ne 0) { throw "Dependency setup failed with code $($process.ExitCode). See setup.log." }
}

function Invoke-Wsl([string[]]$Arguments) {
    $savedPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & wsl.exe @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $savedPreference }
    $text = ($output | Out-String).Replace([string][char]0, '').Trim()
    Add-Content -LiteralPath $logPath -Value $text
    if ($code -ne 0) { throw "Runtime setup failed ($code). $text" }
    return $text
}

function Wsl-Path([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full -notmatch '^([a-zA-Z]):\\(.*)$') { throw 'Setup needs files on a local Windows drive.' }
    return '/mnt/' + $Matches[1].ToLowerInvariant() + '/' + $Matches[2].Replace('\','/')
}

try {
    Set-Content -LiteralPath $logPath -Value ('AirBridge setup ' + (Get-Date -Format o))
    $deps = Get-Content -LiteralPath (Join-Path $InstallRoot 'dependencies.json') -Raw | ConvertFrom-Json
    $payload = Get-Content -LiteralPath (Join-Path $InstallRoot 'runtime-manifest.json') -Raw | ConvertFrom-Json
    $overlay = Join-Path $InstallRoot 'airbridge-runtime.tar.gz'
    Assert-Hash $overlay $payload.sha256
    if ($payload.kernel -ne $deps.kernel -or $payload.version -ne '0.2.0') { throw 'Runtime manifest does not match this installer.' }
    if ($ValidateOnly) { Write-Output 'PACKAGE_VALIDATED'; exit 0 }
    if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
        throw 'This release supports x64 Windows only.'
    }
    if ($DistroName -notmatch '^AirBridge[A-Za-z0-9]*$') { throw 'Invalid runtime name.' }
    # Elevate only machine-wide dependencies. WSL registration must run as the
    # original Windows user, even when UAC uses another administrator account.
    $wslReady = $false
    if (Get-Command wsl.exe -ErrorAction SilentlyContinue) {
        & wsl.exe --status *> $null
        $wslReady = $LASTEXITCODE -eq 0
    }
    if (-not $wslReady) {
        $msi = Get-Dependency $deps.wsl
        Invoke-Elevated 'msiexec.exe' @('/i', ('"' + $msi + '"'), '/passive', '/norestart')
        Invoke-Elevated 'dism.exe' @('/online','/enable-feature','/featurename:VirtualMachinePlatform','/all','/norestart')
        Invoke-Elevated 'dism.exe' @('/online','/enable-feature','/featurename:Microsoft-Windows-Subsystem-Linux','/all','/norestart')
        if ($script:restartNeeded) { throw 'RESTART_REQUIRED' }
        # Enabling virtualization can require a reboot even when DISM returns 0.
        & wsl.exe --status *> $null
        if ($LASTEXITCODE -ne 0) { throw 'RESTART_REQUIRED' }
    }
    $usbExe = Join-Path $env:ProgramFiles 'usbipd-win\usbipd.exe'
    if (-not (Test-Path -LiteralPath $usbExe)) {
        $msi = Get-Dependency $deps.usbipd
        Invoke-Elevated 'msiexec.exe' @('/i', ('"' + $msi + '"'), '/passive', '/norestart')
    }
    if (-not (Test-Path -LiteralPath $usbExe)) { throw 'USB support did not install successfully.' }
    if ($script:restartNeeded) { throw 'RESTART_REQUIRED' }
    $names = Invoke-Wsl @('--list','--quiet')
    $exists = @($names -split "`r?`n" | ForEach-Object { $_.Trim() }) -contains $DistroName
    if (-not $exists) {
        $base = Get-Dependency $deps.ubuntu
        $runtimeDirectory = Join-Path $setupHome $DistroName
        Invoke-Wsl @('--import', $DistroName, $runtimeDirectory, $base, '--version', '2') | Out-Null
    }
    $kernel = Invoke-Wsl @('-d',$DistroName,'-u','root','--exec','uname','-r')
    if ($kernel -ne $deps.kernel) {
        if ($kernel -match '^(\d+\.\d+\.\d+\.\d+)-' -and [version]$Matches[1] -lt [version]'6.18.33.2') {
            $msi = Get-Dependency $deps.wsl
            Invoke-Elevated 'msiexec.exe' @('/i', ('"' + $msi + '"'), '/passive', '/norestart')
            throw 'RESTART_REQUIRED'
        }
        throw "This release needs WSL kernel $($deps.kernel). Your kernel is $kernel. Update WSL to 2.7.13, restart WSL, then run Setup again. Setup has preserved your existing WSL settings."
    }
    if ($exists) {
        $marker = Invoke-Wsl @('-d',$DistroName,'-u','root','--exec','sh','-c','test -f /opt/airbridge/runtime-version && cat /opt/airbridge/runtime-version || true')
        if ($marker -notlike 'AirBridge runtime *') { throw 'An unmanaged AirBridgeRuntime already exists. Setup preserved it. See the upgrade instructions before migrating a development installation.' }
    }
    Invoke-Wsl @('-d',$DistroName,'-u','root','--exec','tar','-xzf',(Wsl-Path $overlay),'-C','/') | Out-Null
    Invoke-Wsl @('-d',$DistroName,'-u','root','--exec','bash','/opt/airbridge/bin/bootstrap') | Out-Null
    $rows = & $usbExe list
    $row = $rows | Where-Object { $_ -match '^\S+\s+0bda:f179\s' } | Select-Object -First 1
    if (-not $row) { throw 'Plug in the supported Realtek RTL8188FTV adapter, then run Setup again to finish USB configuration.' }
    if ($row -and $row -match 'Not shared') {
        $busId = ($row.Trim() -split '\s+')[0]
        Invoke-Elevated $usbExe @('bind','--busid',$busId)
    }
    Set-Content -LiteralPath $resultPath -Value 'Setup complete. Plug in the supported adapter and open AirBridge.'
    exit 0
} catch {
    if ($_.Exception.Message -eq 'RESTART_REQUIRED') {
        Set-Content -LiteralPath $resultPath -Value 'Restart Windows, then run AirBridge Setup again to finish installing the runtime.'
        exit 3010
    }
    $_ | Out-String | Add-Content -LiteralPath $logPath
    Set-Content -LiteralPath $resultPath -Value $_.Exception.Message
    exit 1
}
