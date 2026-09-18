#Requires -Version 5.1
<#
.SYNOPSIS
Build and stage a self-contained DBX Web deployment for IIS on Windows.

.DESCRIPTION
Builds the frontend (dist\) and the dbx-web server (target\release\dbx-web.exe), stages
everything under -DeployRoot, generates site\web.config from the template for the chosen
mode, and optionally creates the IIS application pool, unlocks the configuration sections
DBX needs, sets directory ACLs, and runs a smoke test against the staged binary.

The data directory is never deleted or overwritten: it holds connections, history, and the
password hash.

.EXAMPLE
.\deploy.bat
Build with default features and stage to D:\dbx (HttpPlatformHandler mode).

.EXAMPLE
.\deploy.bat -NoSqlCipher -SkipInstall
Skip the vendored OpenSSL build (no SQLCipher support) and reuse existing node_modules.

.EXAMPLE
.\deploy.bat -DeployRoot E:\apps\dbx -Mode arr -Port 4224 -SubPath /dbx
Stage for the ARR reverse-proxy mode under a /dbx subpath.

.EXAMPLE
.\deploy.bat -SkipBuild -SkipIis
Restage an existing build and print the IIS commands to run as an administrator.
#>
[CmdletBinding()]
param(
  # Deployment root that receives bin\, static\, data\, logs\ and site\.
  [string]$DeployRoot,

  # httpPlatform: IIS launches dbx-web.exe. arr: IIS only reverse-proxies to a service.
  [ValidateSet("httpPlatform", "arr")]
  [string]$Mode = "httpPlatform",

  # Listen port recorded in the generated config. httpPlatform mode overrides it per request.
  [int]$Port = 4224,

  # IIS site name, used when -Binding is given.
  [string]$SiteName = "dbx-web",

  # IIS application pool name, also the identity that receives data directory ACLs.
  [string]$AppPool = "dbx-web",

  # Optional IIS binding such as "http/*:8080:dbx.example.com". Creating a site needs admin.
  [string]$Binding,

  # Optional reverse-proxy subpath such as /dbx. Must match DBX_PUBLIC_BASE_PATH.
  [string]$SubPath,

  # Skip "pnpm install --frozen-lockfile" and reuse the existing node_modules.
  [switch]$SkipInstall,

  # Reuse existing build output instead of compiling.
  [switch]$SkipBuild,

  # Do not touch IIS or directory ACLs; print the commands instead.
  [switch]$SkipIis,

  # Do not start the staged binary for the smoke test.
  [switch]$SkipVerify,

  # Build with --no-default-features, dropping SQLCipher for encrypted SQLite files.
  # This avoids the vendored OpenSSL build entirely.
  [switch]$NoSqlCipher,

  # Overwrite an existing site\web.config.
  [switch]$Force,

  # Print the resolved plan and the commands that would run, without changing anything.
  [switch]$DryRun
)

$ErrorActionPreference = "Stop"

function Write-Step {
  param([string]$Message)
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Info {
  param([string]$Message)
  Write-Host "    $Message"
}

function Write-Warn {
  param([string]$Message)
  Write-Host "warn: $Message" -ForegroundColor Yellow
}

function Stop-Deploy {
  param([string]$Message)
  Write-Host ""
  Write-Host "error: $Message" -ForegroundColor Red
  exit 1
}

function Test-CommandAvailable {
  param([string]$Name)
  return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Invoke-Native {
  param([string]$Exe, [string[]]$Arguments)
  Write-Info "$Exe $($Arguments -join ' ')"
  & $Exe @Arguments
  if ($LASTEXITCODE -ne 0) {
    Stop-Deploy "$Exe exited with code $LASTEXITCODE"
  }
}

# Locates vcvars64.bat. The vendored OpenSSL build inside the default feature set runs
# nmake directly, so it needs INCLUDE/LIB from a Visual Studio developer environment;
# a plain PowerShell session leaves those unset and the C build fails on limits.h.
function Find-VcVars {
  $candidates = @()
  $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $vswhere) {
    $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($install) {
      $candidates += (Join-Path $install.Trim() "VC\Auxiliary\Build\vcvars64.bat")
    }
  }
  $candidates += Get-ChildItem "${env:ProgramFiles}\Microsoft Visual Studio\*\*\VC\Auxiliary\Build\vcvars64.bat" -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty FullName

  foreach ($candidate in $candidates) {
    if (Test-Path $candidate) {
      return $candidate
    }
  }
  return $null
}

function Find-MsvcInclude {
  param([string]$VcVars)
  if (-not $VcVars) {
    return (Get-DeveloperPromptInclude)
  }
  $vcRoot = Split-Path (Split-Path (Split-Path $VcVars -Parent) -Parent) -Parent
  $header = Get-ChildItem (Join-Path $vcRoot "Tools\MSVC\*\include\vcruntime.h") -ErrorAction SilentlyContinue |
    Select-Object -First 1
  if ($header) { return $header.FullName }
  return $null
}

# Returns the INCLUDE directory holding vcruntime.h when a Visual Studio developer
# prompt is already active, otherwise $null.
function Get-DeveloperPromptInclude {
  if (-not $env:INCLUDE) { return $null }
  foreach ($dir in ($env:INCLUDE -split ';')) {
    if ($dir -and (Test-Path (Join-Path $dir "vcruntime.h"))) { return $dir }
  }
  return $null
}

function Find-WindowsSdkUcrt {
  $roots = @()
  $sdk = (Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Microsoft SDKs\Windows\v10.0" -ErrorAction SilentlyContinue).InstallationFolder
  if ($sdk) { $roots += (Join-Path $sdk "Include") }
  $roots += "${env:ProgramFiles(x86)}\Windows Kits\10\Include"
  foreach ($root in $roots) {
    $header = Get-ChildItem (Join-Path $root "*\ucrt\corecrt.h") -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($header) { return $header.FullName }
  }
  return $null
}

function Get-IisAppCmd {
  $appcmd = Join-Path $env:windir "System32\inetsrv\appcmd.exe"
  if (Test-Path $appcmd) { return $appcmd }
  return $null
}

function Test-IsAdministrator {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-AppCmd {
  param([string]$AppCmd, [string[]]$Arguments)
  $output = & $AppCmd @Arguments 2>&1
  $code = $LASTEXITCODE
  $text = ($output | Out-String).Trim()
  return [pscustomobject]@{ Code = $code; Output = $text }
}

function Invoke-AppCmdChecked {
  param([string]$AppCmd, [string[]]$Arguments, [string]$Description, [switch]$Required)
  $result = Invoke-AppCmd -AppCmd $AppCmd -Arguments $Arguments
  if ($result.Code -ne 0 -or $result.Output -match "ERROR \(") {
    if ($Required) {
      Stop-Deploy "$Description failed: $($result.Output)"
    }
    Write-Warn "$Description failed: $($result.Output)"
  }
  return $result
}

# ---------------------------------------------------------------------------

if (-not $DeployRoot) {
  $DeployRoot = if ($env:DBX_DEPLOY_ROOT) { $env:DBX_DEPLOY_ROOT } else { "D:\dbx" }
}
$DeployRoot = [IO.Path]::GetFullPath($DeployRoot).TrimEnd('\')

$normalizedSubPath = ""
if ($SubPath) {
  $normalizedSubPath = "/" + ($SubPath.Trim().Trim('/'))
  if ($normalizedSubPath -eq "/") {
    $normalizedSubPath = ""
  }
}

Write-Host "DBX Web -> IIS deployment" -ForegroundColor White
Write-Info "repository: $PSScriptRoot\..\.."
Write-Info "deploy root: $DeployRoot"
Write-Info "mode: $Mode"
Write-Info "app pool: $AppPool"
if ($normalizedSubPath) { Write-Info "base path: $normalizedSubPath" }
if ($DryRun) { Write-Info "dry run: nothing will be changed" }

if ($Mode -eq "arr" -and $normalizedSubPath) {
  Write-Warn "in arr mode DBX_PUBLIC_BASE_PATH comes from the Windows service environment, not from web.config."
  Write-Warn "Add DBX_PUBLIC_BASE_PATH=$normalizedSubPath to the service definition."
}

# --- preflight -------------------------------------------------------------

Write-Step "Checking the toolchain"

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..\..")).Path
if (-not (Test-Path (Join-Path $repoRoot "crates\dbx-web\Cargo.toml"))) {
  Stop-Deploy "cannot find crates\dbx-web in $repoRoot; run this script from the repository."
}

$hasCargo = Test-CommandAvailable "cargo"
$developerPromptInclude = Get-DeveloperPromptInclude
# A developer prompt already provides INCLUDE/LIB, so there is nothing to launch.
$vcVars = if ($developerPromptInclude) { $null } else { Find-VcVars }
$msvcInclude = Find-MsvcInclude -VcVars $vcVars
$sdkUcrt = Find-WindowsSdkUcrt
$needsVcVars = -not $NoSqlCipher
$pnpmExe = if (Test-CommandAvailable "pnpm.cmd") { "pnpm.cmd" } else { "pnpm" }

if (-not $SkipBuild) {
  if (-not $hasCargo) {
    Stop-Deploy "cargo was not found on PATH. Install Rust with the MSVC toolchain from https://rustup.rs"
  }
  if (-not $SkipInstall -and -not (Test-CommandAvailable "pnpm")) {
    Stop-Deploy "pnpm was not found on PATH. Install Node.js 22 and run 'corepack enable' or 'npm i -g pnpm'."
  }
  if (-not $msvcInclude) {
    Stop-Deploy @"
the MSVC C++ headers are missing (VC\Tools\MSVC\<version>\include\vcruntime.h).
Rust cannot compile the C dependencies (zstd, SQLite, OpenSSL) without them.

Fix: Visual Studio Installer -> Modify -> Individual components, enable
  - MSVC v143/v145 C++ x64/x86 build tools
  - Windows 10 SDK or Windows 11 SDK
and make sure 'Desktop development with C++' is installed.
"@
  }
  Write-Info "MSVC headers: $msvcInclude"
  if ($needsVcVars) {
    if (-not $sdkUcrt) {
      Stop-Deploy "no Windows 10/11 SDK ucrt headers found; install one from the Visual Studio installer."
    }
    if (-not $vcVars) {
      Stop-Deploy @"
no Visual Studio developer environment is available, and the vendored OpenSSL build
needs INCLUDE/LIB from one.

Fix one of these:
  - Install 'Desktop development with C++' (which provides VC\Auxiliary\Build\vcvars64.bat), or
  - Run this script from an "x64 Native Tools Command Prompt for VS", or
  - Re-run with -NoSqlCipher to skip the OpenSSL build entirely.
"@
    }
    Write-Info "vcvars: $vcVars"
  } elseif ($developerPromptInclude) {
    Write-Info "SQLCipher disabled: skipping the vendored OpenSSL build"
    Write-Info "using the active Visual Studio developer prompt ($developerPromptInclude)"
  } else {
    Write-Info "SQLCipher disabled: skipping the vendored OpenSSL build"
  }
}

$frontendDist = Join-Path $repoRoot "dist"
$builtExe = Join-Path $repoRoot "target\release\dbx-web.exe"

$binDir = Join-Path $DeployRoot "bin"
$staticDir = Join-Path $DeployRoot "static"
$dataDir = Join-Path $DeployRoot "data"
$logsDir = Join-Path $DeployRoot "logs"
$siteDir = Join-Path $DeployRoot "site"

if ($DryRun) {
  Write-Step "Plan"
  Write-Info "build frontend  : $pnpmExe install --frozen-lockfile ; $pnpmExe build -> $frontendDist"
  $planArgs = if ($NoSqlCipher) {
    "build --release -p dbx-web --no-default-features --features duckdb-sidecar,mq-admin,system-fonts"
  } else {
    "build --release -p dbx-web"
  }
  if ($vcVars) {
    Write-Info "build backend   : cmd /c (call `"$vcVars`" && cargo $planArgs)"
  } else {
    Write-Info "build backend   : cargo $planArgs (using the active developer prompt)"
  }
  Write-Info "stage binary    : $builtExe -> $binDir\dbx-web.exe"
  Write-Info "stage frontend  : $frontendDist\* -> $staticDir"
  Write-Info "create dirs     : $binDir $staticDir $dataDir $logsDir $siteDir (data\ is never cleared)"
  Write-Info "write config    : $siteDir\web.config (mode $Mode)"
  Write-Info "create app pool : $AppPool"
  Write-Info "grant ACLs      : IIS AppPool\$AppPool -> $dataDir, $logsDir"
  Write-Info "smoke test      : bin\dbx-web.exe on 127.0.0.1:4295"
  exit 0
}

# --- build -----------------------------------------------------------------

if (-not $SkipBuild) {
  if (-not $SkipInstall) {
    Write-Step "Installing frontend dependencies"
    Invoke-Native $pnpmExe @("install", "--frozen-lockfile")
  }

  Write-Step "Building the frontend"
  Invoke-Native $pnpmExe @("build")
  if (-not (Test-Path (Join-Path $frontendDist "index.html"))) {
    Stop-Deploy "the frontend build did not produce $frontendDist\index.html"
  }

  Write-Step "Building dbx-web"
  $cargoArgs = @("build", "--release", "-p", "dbx-web")
  if ($NoSqlCipher) {
    $cargoArgs += @("--no-default-features", "--features", "duckdb-sidecar,mq-admin,system-fonts")
  }

  if ($vcVars) {
    # Build inside vcvars64.bat whenever one is available: cc-rs then compiles the C
    # dependencies (SQLite, zstd, and optionally OpenSSL) with the Visual Studio
    # install those INCLUDE/LIB variables belong to, instead of picking the newest
    # registered install, which may be missing its C++ component.
    $bootScript = Join-Path $env:TEMP ("dbx-iis-build-" + [Guid]::NewGuid().ToString("N") + ".cmd")
    $lines = @(
      "@echo off"
      "call `"$vcVars`" >nul || exit /b 1"
      "cd /d `"$repoRoot`" || exit /b 1"
      "cargo $($cargoArgs -join ' ')"
    )
    Set-Content -LiteralPath $bootScript -Value $lines -Encoding OEM
    try {
      Invoke-Native "cmd.exe" @("/c", $bootScript)
    } finally {
      Remove-Item -LiteralPath $bootScript -Force -ErrorAction SilentlyContinue
    }
  } else {
    Invoke-Native "cargo" $cargoArgs
  }

  if (-not (Test-Path $builtExe)) {
    Stop-Deploy "the build did not produce $builtExe"
  }
}

# --- stage -----------------------------------------------------------------

Write-Step "Staging into $DeployRoot"

foreach ($dir in @($binDir, $staticDir, $dataDir, $logsDir, $siteDir)) {
  if (-not (Test-Path $dir)) {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Write-Info "created $dir"
  }
}

if (Test-Path (Join-Path $binDir "dbx-web.exe")) {
  Write-Info "replacing $binDir\dbx-web.exe"
}

if (Test-Path $builtExe) {
  Copy-Item -LiteralPath $builtExe -Destination (Join-Path $binDir "dbx-web.exe") -Force
  Write-Info "$builtExe -> $binDir\dbx-web.exe"
} elseif (-not (Test-Path (Join-Path $binDir "dbx-web.exe"))) {
  Stop-Deploy "no binary to stage: $builtExe is missing and $binDir\dbx-web.exe does not exist either."
} else {
  Write-Warn "$builtExe is missing; keeping the existing $binDir\dbx-web.exe"
}

if (-not (Test-Path (Join-Path $frontendDist "index.html"))) {
  Stop-Deploy "no frontend build to stage: $frontendDist\index.html is missing."
}

# static\ holds build output only, so it is safe to mirror. data\ is never touched.
if ((Split-Path $staticDir -Leaf) -ne "static") {
  Stop-Deploy "refusing to clean an unexpected static directory: $staticDir"
}
Get-ChildItem -LiteralPath $staticDir -Force | Remove-Item -Recurse -Force
Copy-Item -Path (Join-Path $frontendDist "*") -Destination $staticDir -Recurse -Force
Write-Info "$frontendDist\* -> $staticDir"
Write-Info "existing data directory left untouched: $dataDir"

# --- site config -----------------------------------------------------------

Write-Step "Generating the IIS site configuration"

$templateName = if ($Mode -eq "arr") { "web.config.arr-proxy" } else { "web.config" }
$templatePath = Join-Path $PSScriptRoot $templateName
$configTarget = Join-Path $siteDir "web.config"

if (-not (Test-Path $templatePath)) {
  Stop-Deploy "missing template $templatePath"
}

if ((Test-Path $configTarget) -and -not $Force) {
  Write-Warn "$configTarget already exists; keeping it. Use -Force to regenerate from $templateName."
} else {
  $basePathBlock = if ($normalizedSubPath) {
    "        <environmentVariable name=`"DBX_PUBLIC_BASE_PATH`" value=`"$normalizedSubPath`" />"
  } else {
    @(
      "        <!-- Uncomment to publish under a subpath, for example https://host/dbx/ -->"
      "        <!-- <environmentVariable name=`"DBX_PUBLIC_BASE_PATH`" value=`"/dbx`" /> -->"
    ) -join "`r`n"
  }

  $configText = Get-Content -LiteralPath $templatePath -Raw
  $configText = $configText.Replace("{{DBX_ROOT}}", $DeployRoot)
  $configText = $configText.Replace("{{DBX_PORT}}", $Port.ToString())
  $configText = $configText.Replace("{{DBX_BASE_PATH}}", $basePathBlock)

  if ($configText -match "\{\{") {
    Stop-Deploy "the generated configuration still contains an unsubstituted placeholder."
  }
  try {
    [xml]$configText | Out-Null
  } catch {
    Stop-Deploy "the generated configuration is not valid XML: $($_.Exception.Message)"
  }

  [IO.File]::WriteAllText($configTarget, $configText, (New-Object Text.UTF8Encoding($false)))
  Write-Info "$templateName -> $configTarget"
}

# --- IIS -------------------------------------------------------------------

Write-Step "Configuring IIS"

$appcmd = Get-IisAppCmd
$isAdmin = Test-IsAdministrator
$manual = @(
  "Start an elevated prompt and run:"
  "  appcmd.exe add apppool /name:$AppPool /managedRuntimeVersion:`"`" /managedPipelineMode:Integrated /startMode:AlwaysRunning /idleTimeout:0 /periodicRestart.time:00:00:00 /disallowOverlappingRotation:true"
  "  appcmd.exe set apppool /apppool.name:$AppPool /idleTimeout:0 /periodicRestart.time:00:00:00 /disallowOverlappingRotation:true"
  "  appcmd.exe unlock config /section:system.webServer/handlers"
)
if ($Mode -eq "httpPlatform") {
  $manual += "  appcmd.exe unlock config /section:system.webServer/httpPlatform"
} else {
  $manual += "  appcmd.exe unlock config /section:system.webServer/proxy"
}
$manual += @(
  "  appcmd.exe add site /name:$SiteName /physicalPath:""$siteDir"" /bindings:""http/*:8080:"""
  "  appcmd.exe set app ""$SiteName/"" /applicationPool:$AppPool"
  "  icacls ""$dataDir"" /grant ""IIS AppPool\${AppPool}:(OI)(CI)M"" /T /C"
  "  icacls ""$logsDir"" /grant ""IIS AppPool\${AppPool}:(OI)(CI)M"" /T /C"
)

if (-not $appcmd) {
  Write-Warn "IIS does not look installed ($env:windir\System32\inetsrv\appcmd.exe is missing); skipping."
} elseif ($SkipIis) {
  Write-Info "-SkipIis given; IIS was not touched."
  $manual | ForEach-Object { Write-Info $_ }
} elseif (-not $isAdmin) {
  Write-Warn "not running as administrator; IIS and ACL steps were skipped."
  $manual | ForEach-Object { Write-Info $_ }
} else {
  $websocket = $null
  try {
    $websocket = Get-WindowsOptionalFeature -Online -FeatureName IIS-WebSockets -ErrorAction Stop
  } catch {
    Write-Warn "could not query the WebSocket feature: $($_.Exception.Message)"
  }
  if ($websocket -and $websocket.State -ne "Enabled") {
    Write-Warn "the WebSocket Protocol feature is disabled; Redis Pub/Sub will not work."
    Write-Warn "enable it with: dism /online /enable-feature /featurename:IIS-WebSockets"
  }

  if ($Mode -eq "httpPlatform") {
    $platformHandler = Get-ChildItem (Join-Path $env:windir "System32\inetsrv\httpPlatformHandler*.dll") -ErrorAction SilentlyContinue
    if (-not $platformHandler) {
      Write-Warn "HttpPlatformHandler was not found in $env:windir\System32\inetsrv."
      Write-Warn "install it from https://www.iis.net/downloads/microsoft/httpplatformhandler"
    }
  }

  $pool = Invoke-AppCmd -AppCmd $appcmd -Arguments @("list", "apppool", $AppPool)
  $poolExists = ($pool.Code -eq 0) -and ($pool.Output -notmatch "ERROR \(") -and $pool.Output
  if (-not $poolExists) {
    Invoke-AppCmdChecked -AppCmd $appcmd -Description "add app pool $AppPool" -Required -Arguments @(
      "add", "apppool", "/name:$AppPool", "/managedRuntimeVersion:", "/managedPipelineMode:Integrated",
      "/startMode:AlwaysRunning", "/idleTimeout:0", "/periodicRestart.time:00:00:00",
      "/disallowOverlappingRotation:true", "/enable32BitAppOnWin64:false"
    )
    Write-Info "created app pool $AppPool"
  } else {
    Write-Info "app pool $AppPool already exists"
  }
  Invoke-AppCmdChecked -AppCmd $appcmd -Description "tune app pool $AppPool" -Required -Arguments @(
    "set", "apppool", "/apppool.name:$AppPool", "/idleTimeout:0", "/periodicRestart.time:00:00:00",
    "/disallowOverlappingRotation:true", "/startMode:AlwaysRunning"
  )

  # handlers and modules are unlocked for all applications on a default install, but
  # site-level sections for vendor modules such as httpPlatform and proxy are locked.
  Invoke-AppCmdChecked -AppCmd $appcmd -Description "unlock system.webServer/handlers" -Arguments @(
    "unlock", "config", "/section:system.webServer/handlers"
  )
  if ($Mode -eq "httpPlatform") {
    Invoke-AppCmdChecked -AppCmd $appcmd -Description "unlock system.webServer/httpPlatform" -Arguments @(
      "unlock", "config", "/section:system.webServer/httpPlatform"
    )
  } else {
    Invoke-AppCmdChecked -AppCmd $appcmd -Description "unlock system.webServer/proxy" -Arguments @(
      "unlock", "config", "/section:system.webServer/proxy"
    )
  }

  if ($Binding) {
    $site = Invoke-AppCmd -AppCmd $appcmd -Arguments @("list", "site", $SiteName)
    if ($site.Output -and ($site.Output -notmatch "ERROR \(")) {
      Write-Info "site $SiteName already exists; assigning app pool $AppPool"
    } else {
      Invoke-AppCmdChecked -AppCmd $appcmd -Description "add site $SiteName" -Required -Arguments @(
        "add", "site", "/name:$SiteName", "/physicalPath:$siteDir", "/bindings:$Binding"
      )
      Write-Info "created site $SiteName with binding $Binding"
    }
    Invoke-AppCmdChecked -AppCmd $appcmd -Description "assign app pool to $SiteName" -Required -Arguments @(
      "set", "app", "$SiteName/", "/applicationPool:$AppPool"
    )
  } else {
    Write-Info "no -Binding given: create a site in IIS Manager pointing at $siteDir and set its app pool to $AppPool."
  }

  $identity = "IIS AppPool\$AppPool"
  foreach ($target in @($dataDir, $logsDir)) {
    Write-Info "icacls $target /grant ${identity}:(OI)(CI)M /T /C"
    & icacls $target /grant "${identity}:(OI)(CI)M" /T /C | Out-Null
    if ($LASTEXITCODE -ne 0) {
      Write-Warn "icacls failed for $target (exit $LASTEXITCODE); grant modify rights to $identity manually."
    } else {
      Write-Info "granted modify on $target to $identity"
    }
  }
}

# --- verify ----------------------------------------------------------------

$verificationFailed = $false

if ($SkipVerify) {
  Write-Step "Smoke test skipped (-SkipVerify)"
} else {
  Write-Step "Verifying the staged deployment"

  $probe = Join-Path $dataDir ".dbx-write-probe"
  try {
    Set-Content -LiteralPath $probe -Value "ok"
    Remove-Item -LiteralPath $probe -Force
    Write-Info "data directory is writable by the current account"
  } catch {
    Write-Warn "the current account cannot write to $dataDir : $($_.Exception.Message)"
  }

  $stagedExe = Join-Path $binDir "dbx-web.exe"
  if (-not (Test-Path $stagedExe)) {
    Write-Warn "no staged binary at $stagedExe; skipping the HTTP smoke test"
  } else {
    $smokeDir = Join-Path $logsDir "smoke-data"
    $smokeLog = Join-Path $logsDir "smoke.stdout.log"
    $smokeErr = Join-Path $logsDir "smoke.stderr.log"
    $smokePort = 4295
    Remove-Item -LiteralPath $smokeDir -Recurse -Force -ErrorAction SilentlyContinue

    $savedEnv = @{}
    foreach ($name in @("DBX_DATA_DIR", "DBX_STATIC_DIR", "DBX_PORT", "DBX_DISABLE_PASSWORD")) {
      $savedEnv[$name] = [Environment]::GetEnvironmentVariable($name)
    }

    $env:DBX_DATA_DIR = $smokeDir
    $env:DBX_STATIC_DIR = $staticDir
    $env:DBX_PORT = "$smokePort"
    $env:DBX_DISABLE_PASSWORD = "1"

    $process = $null
    try {
      $process = Start-Process -FilePath $stagedExe -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput $smokeLog -RedirectStandardError $smokeErr
    } catch {
      Write-Warn "could not start $stagedExe : $($_.Exception.Message)"
    } finally {
      foreach ($name in $savedEnv.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnv[$name])
      }
    }

    if ($process) {
      $ready = $false
      for ($attempt = 0; $attempt -lt 30; $attempt++) {
        if ($process.HasExited) { break }
        try {
          $index = Invoke-WebRequest -Uri "http://127.0.0.1:$smokePort/" -UseBasicParsing -TimeoutSec 3
          $auth = Invoke-WebRequest -Uri "http://127.0.0.1:$smokePort/api/auth/check" -UseBasicParsing -TimeoutSec 3
          if ($index.StatusCode -eq 200 -and $auth.Content -match '"authenticated"\s*:\s*true') {
            $ready = $true
            break
          }
        } catch {
          Start-Sleep -Seconds 1
        }
      }

      if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
      }
      Start-Sleep -Milliseconds 500

      if ($ready) {
        Write-Info "dbx-web served $staticDir and /api/auth/check on 127.0.0.1:$smokePort"
      } else {
        $verificationFailed = $true
        Write-Warn "the staged binary did not answer on 127.0.0.1:$smokePort within 30s."
        foreach ($log in @($smokeLog, $smokeErr)) {
          if (Test-Path $log) {
            Write-Warn "--- $log ---"
            Get-Content -LiteralPath $log -Tail 20 | ForEach-Object { Write-Host "    $_" }
          }
        }
      }
      Remove-Item -LiteralPath $smokeDir -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
}

# --- summary ---------------------------------------------------------------

Write-Step "Done"
Write-Info "binary : $binDir\dbx-web.exe"
Write-Info "static : $staticDir"
Write-Info "data   : $dataDir"
Write-Info "config : $configTarget ($Mode)"

if ($Mode -eq "httpPlatform") {
  Write-Info "next   : point an IIS site at $siteDir; IIS starts dbx-web.exe on demand."
} else {
  Write-Info "next   : install dbx-web.exe as a Windows service (WinSW), then point IIS at the site."
  Write-Info "         block inbound TCP $Port with Windows Firewall: dbx-web listens on 0.0.0.0, not loopback."
}

if ($Binding) {
  Write-Info "url    : $Binding"
} else {
  Write-Info "url    : add an IIS binding, then open it and sign in on the first-run setup page."
}

Write-Warn "keep the app pool idle timeout at 0 and recycling disabled, or sessions will be lost on recycle."

if ($verificationFailed) {
  Stop-Deploy "the smoke test failed; the deployment was staged but not verified."
}
