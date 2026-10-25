# # Usage
#
# ```powershell
# $ doj-studio [FLAGS] [OPTIONS] <SUBCOMMAND> [ARG ...]
# ```
#
# See the `Write-Help` function below for complete usage instructions.
#
# # Synopsis
#
# blah
#
# # License and Copyright
#
# ```
# Copyright: Copyright (c) 2016 Chef Software, Inc.
# License: Apache License, Version 2.0
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#      http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
# ```
#
#

param (
    [switch]$h,
    [switch]$n,
    [switch]$q,
    [switch]$v,
    [switch]$R,
    [switch]$D,
    [switch]$w,
    [string]$command,
    [string]$commandVal,
    [string]$k,
    [string]$o,
    [string]$s
)

# # Internals

# ## Help/Usage functions

# **Internal** Prints help and usage information. Straight forward, no?
function Write-Help {
  Write-Host @"
$program $version

$author

Dojo Studios - Plan for success!

USAGE:
        $program [FLAGS] [OPTIONS] <SUBCOMMAND> [ARG ..]

COMMON FLAGS:
    -h  Prints this message
    -n  Do not mount the source path into the Studio (default: mount the path)
    -q  Prints less output for better use in scripts
    -v  Prints more verbose output
    -D  Use a Docker Studio instead of a native Studio
DEPRECATED FLAG:
    -w  Use a local Windows studio instead of a Docker Studio

COMMON OPTIONS:
    -k <DOJ_ORIGIN_KEYS>  Installs secret origin keys (default:\$DOJ_ORIGIN )
    -o <DOJ_STUDIO_ROOT>  Sets a Studio root (default: /doj/studios/<DIR_NAME>)
    -s <SRC_PATH>         Sets the source path (default: \$PWD)

SUBCOMMANDS:
    build     Build using a Studio
    enter     Interactively enter a Studio
    help      Prints this message
    new       Creates a new Studio
    rm        Destroys a Studio
    run       Run a command in a Studio
    version   Prints version information

ENVIRONMENT VARIABLES:
    DOJ_ORIGIN        Propagates this variable into any studios
    DOJ_ORIGIN_KEYS   Installs secret keys (\`-k' option overrides)
    DOJ_STUDIOS_HOME  Sets a home path for all Studios (default: /doj/studios)
    DOJ_STUDIO_ROOT   Sets a Studio root (\`-r' option overrides)
    NO_SRC_PATH       If set, do not mount source path (\`-n' flag overrides)
    QUIET             Prints less output (\`-q' flag overrides)
    SRC_PATH          Sets the source path (\`-s' option overrides)
    VERBOSE           Prints more verbose output (\`-v' flag overrides)

SUBCOMMAND HELP:
    $program <SUBCOMMAND> -h

EXAMPLES:

    # Create a new default Studio
    $program new

    # Enter the default Studio
    $program enter

    # Run a command in the default Studio
    $program run doj --version

    # Destroy the default Studio
    $program rm

    # Create and enter a Studio with a custom root
    $program -o /opt/slim

    # Run a command in the slim Studio, showing only the command output
    $program -q -o /opt/slim run busybox ls -l /

    # Verbosely destroy the slim Studio
    $program -v -o /opt/slim rm

"@
}

function Write-BuildHelp {
  Write-Host @"
${program}-build $version

$author

Dojo Studios - execute a build using a Studio

USAGE:
        $program [COMMON_FLAGS] [COMMON_OPTIONS] build [FLAGS] [PLAN_DIR]

FLAGS:
    -R  Reuse a previous Studio state (default: clean up before building)

EXAMPLES:

    # Build a Redis plan
    $program build plans/redis

    # Reuse previous Studio for a build
    $program build -R plans/glibc

"@
}

function Write-EnterHelp {
  Write-Host @"
${program}-enter $version

$author

Dojo Studios - interactively enter a Studio

USAGE:
        $program [COMMON_FLAGS] [COMMON_OPTIONS] enter

"@
}

function Write-NewHelp {
  Write-Host @"
${program}-new $version

$author

Dojo Studios - create a new Studio

USAGE:
        $program [COMMON_FLAGS] [COMMON_OPTIONS] new

"@
}

function Write-RmHelp {
  Write-Host @"
${program}-rm $version

$author

Dojo Studios - destroy a Studio

USAGE:
        $program [COMMON_FLAGS] [COMMON_OPTIONS] rm

"@
}

function Write-RunHelp {
  Write-Host @"
${program}-run $version

$author

Dojo Studios - run a command in a Studio

USAGE:
        $program [COMMON_FLAGS] [COMMON_OPTIONS] run [CMD] [ARG ..]

CMD:
    Command to run in the Studio

ARG:
    Arguments to the command

EXAMPLES:

    $program run wget --version

"@
}

function Write-DojInfo($Message) {
  if($quiet) { return }
  Write-Host "   ${program}: " -ForegroundColor Cyan -NoNewline
  Write-Host $Message
}

# ## Subcommand functions
#
# These are the implementations for each subcommand in the program.

function New-Studio {
  if($printHelp) {
    Write-NewHelp
    return
  }
  Write-DojInfo "Creating Studio at $DOJ_STUDIO_ROOT"

  if(!(Test-Path $DOJ_STUDIO_ROOT)) {
    mkdir $DOJ_STUDIO_ROOT | Out-Null
  }

  Set-Location $DOJ_STUDIO_ROOT
  if(!(Test-Path src) -and !($doNotMount)) {
    mkdir src | Out-Null
    New-Item -Name src -ItemType Junction -target $SRC_PATH.Path | Out-Null
  }

  $pathArray = @(
    "$PSScriptRoot\powershell",
    "$PSScriptRoot\doj",
    "$PSScriptRoot\7zip",
    "$PSScriptRoot",
    "$env:WINDIR\system32",
    "$env:WINDIR",
    (Join-Path $DOJ_STUDIO_ROOT "doj\bin")
  )

  $env:PATH = [String]::Join(";", $pathArray)

  if($env:DOJ_ORIGIN_KEYS) {
    $secret_keys = @()
    $public_keys = @()
    $env:DOJ_ORIGIN_KEYS.Split(" ") | % {
      $pk = & doj origin key export $_ --type=public | Out-String
      $sk = & doj origin key export $_ --type=secret | Out-String
      # doj key import does not like carriage returns
      $secret_keys += $sk.Replace("`r", "")
      $public_keys += $pk.Replace("`r", "")
    }

    $env:FS_ROOT=$DOJ_STUDIO_ROOT
    $env:DOJ_CACHE_KEY_PATH = Join-Path $env:FS_ROOT "doj\cache\keys"
    $public_keys | % { $_ | & doj origin key import }
    $secret_keys | % { $_ | & doj origin key import }
  }
  else {
    $env:FS_ROOT=$DOJ_STUDIO_ROOT
    $env:DOJ_CACHE_KEY_PATH = Join-Path $env:FS_ROOT "doj\cache\keys"
  }


  Set-Secrets

  New-PSDrive -Name "Dojo" -PSProvider FileSystem -Root $DOJ_STUDIO_ROOT -Scope Script | Out-Null
  Set-Location "Dojo:\src"
}

function Enter-Studio {
  if($printHelp) {
    Write-EnterHelp
    return
  }
  if(!(Test-Path $DOJ_STUDIO_ROOT)) {
    mkdir $DOJ_STUDIO_ROOT | Out-Null
  }
  $env:DOJ_STUDIO_ENTER_ROOT = Resolve-Path $DOJ_STUDIO_ROOT
  if (Test-InContainer) {
    # The Windows Docker TTY does not render non standard
    # characters. Each is rendered as a '?'. So we are going
    # to just render standard ascii symbols. No pretty clouds
    # or check marks.
    $env:DOJ_GLYPH_STYLE="ascii"
  }
  New-Studio
  $env:STUDIO_SCRIPT_ROOT = $PSScriptRoot
  $shouldStartStudio = $false

  if(!(Test-InContainer) -and (Get-Process -Name doj-sup -ErrorAction SilentlyContinue)) {
    Write-Warning "A Dojo Supervisor is already running on this machine."
    Write-Warning "Only one Supervisor can run at a time."
    Write-Warning "A Supervisor will not be started in this Studio."
  } else {
    $dojSvc = Get-Service Dojo -ErrorAction SilentlyContinue
    if(!$dojSvc -or ($dojSvc.Status -eq "Stopped")) {
      $shouldStartStudio = $true

      # Set console encoding to UTF-8 so that any redirected glyphs
      # from the supervisor log are propperly encoded
      [System.Console]::OutputEncoding = [System.Text.Encoding]::UTF8

      # We start the Supervisor and handle its output logging in C# which will handle
      # the process's OutputDataReceived and ErrorDataReceived events in a separate thread.
      # While we could use PowerShell's Register-ObjectEvent instead. That uses a PSJob
      # which will be blocked while the interactive studio is open
      Add-Type -TypeDefinition (Get-Content "$PSScriptRoot\SupervisorBootstrapper.cs" | Out-String)

      # If the termcolor crate cannot find a console, which it will not
      # since we launch the supervisor in the background, it will fall back
      # to ANSI codes on Windows unless we explicitly turn off color. Lets
      # do that if on a windows version that does not support ANSI codes in
      # its console
      $ansi_min_supported_version = [Version]::new(10, 0, 10586)
      $osVersion = [Version]::new((Get-CimInstance -ClassName Win32_OperatingSystem).Version)
      $isAnsiSupported = $false
      if ($osVersion -ge $ansi_min_supported_version) {
        $isAnsiSupported = $true
      }

      mkdir $env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default -Force | Out-Null
      [SupervisorBootstrapper]::Run($isAnsiSupported)
    }
    
    Write-Host  "** The Dojo Supervisor has been started in the background." -ForegroundColor Cyan
    Write-Host  "** Use 'doj svc start' and 'doj svc stop' to start and stop services." -ForegroundColor Cyan
    Write-Host  "** Use the 'Get-SupervisorLog' command to stream the Supervisor log." -ForegroundColor Cyan
    Write-Host  "** Use the 'Stop-Supervisor' to terminate the Supervisor." -ForegroundColor Cyan
    Write-Host  ""
  }
  Write-DojInfo "Entering Studio at $DOJ_STUDIO_ROOT"
  & "$PSScriptRoot\powershell\pwsh.exe" -NoProfile -ExecutionPolicy bypass -NoLogo -NoExit -Command {
    function prompt {
      Write-Host "[DOJ-STUDIO]" -NoNewLine -ForegroundColor Green
      " $($executionContext.SessionState.Path.CurrentLocation)$('>' * ($nestedPromptLevel +1)) "
    }
    function build {
      & "$env:STUDIO_SCRIPT_ROOT\doj-plan-build.ps1" @args
    }
    function Test-InContainer {
      (Get-Service -Name cexecsvc -ErrorAction SilentlyContinue) -ne $null
    }

    function Get-SupervisorLog {
      # If we are not running in a container then the powershell studio was
      # spawned by doj.exe and ctrl+c behaves poorly in this scenario. So
      # we work around the issue by tailing the log in a new window that can
      # simply be closed. If we are inside a container then we cannot launch 
      # new windows but thats ok because docker run launched the studio directly
      # from powershell so the ctrl+c issue is not a problem so we can do
      # a simple tail
      if (!(Test-InContainer)) {
        Start-Process "$env:STUDIO_SCRIPT_ROOT\powershell\pwsh.exe" -ArgumentList "-Command `"& {Get-Content $env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\out.log -Tail 100 -Wait}`""
      }
      else {
        # Container studios run dojo as a service which logs to a 
        # configured file location
        $svcPath = Join-Path $env:SystemDrive "doj\svc\windows-service"
        [xml]$configXml = Get-Content (Join-Path $svcPath log4net.xml)
        $logPath = (Resolve-Path $configXml.log4net.appender.file.value).Path

        Get-Content $logPath -Tail 100 -Wait
      }
    }

    function Stop-Supervisor {
      $dojSvc = Get-Service Dojo -ErrorAction SilentlyContinue
      if($dojSvc -and ($dojSvc.Status -eq "Running")) {
        Stop-Service Dojo
      }
      elseif(Test-Path "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK") {
        Stop-Process -Id (Get-Content "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK")
        Remove-Item "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK" -Force -ErrorAction SilentlyContinue
      }
    }

    New-PSDrive -Name "Dojo" -PSProvider FileSystem -Root $env:DOJ_STUDIO_ENTER_ROOT | Out-Null
    Set-Location "Dojo:\src"
  }

  if($shouldStartStudio -and (Test-Path "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK")) {
    Stop-Process -Id (Get-Content "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK")
    $retry = 0
    while(($retry -lt 5) -and (Test-Path "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK")) {
      $retry += 1
      Write-Host "Waiting for Supervisor to finish..."
      Start-Sleep -Seconds 5
    }
    Remove-Item "$env:DOJ_STUDIO_ENTER_ROOT\doj\sup\default\LOCK" -Force -ErrorAction SilentlyContinue
  }
}

function Invoke-StudioRun($cmd) {
  if($printHelp -or ([String]::IsNullOrEmpty($cmd))) {
    Write-RunHelp
    return
  }
  New-Studio
  Write-DojInfo "Running '$cmd' in Studio at $DOJ_STUDIO_ROOT"
  Invoke-Expression $cmd
}

function Invoke-StudioBuild($location, $reuse) {
  # This trap will cause powershell to return an exit code of 1
  trap { "An error occured in the build!" }
  
  if($printHelp -or ([String]::IsNullOrEmpty($location))) {
    Write-BuildHelp
    return
  }
  if(!$reuse) { Remove-Studio}

  New-Studio
  Write-DojInfo "Building '$location' in Studio at $DOJ_STUDIO_ROOT"

  & "$PSScriptRoot\doj-plan-build.ps1" $location
}

function Remove-Studio {
  if($printHelp) {
    Write-RmHelp
    return
  }
  if ($DOJ_STUDIO_ROOT -eq "$env:SystemDrive\") {
    Write-DojInfo "Studio is rooted in system drive. Skipping Studio removal."
  } else {
    if(Test-Path $DOJ_STUDIO_ROOT) {
      Write-DojInfo "Destroying Studio at $DOJ_STUDIO_ROOT"
      Remove-Item $DOJ_STUDIO_ROOT -Recurse -Force
    }
  }
}

function Test-InContainer {
  (Get-Service -Name cexecsvc -ErrorAction SilentlyContinue) -ne $null
}

function Remove-UnsafeSecrets {
  @('DOJ_ORIGIN', 'PATH') | ForEach-Object {
    if(Test-Path "env:\DOJ_STUDIO_SECRET_$_") {
      Remove-Item "env:\DOJ_STUDIO_SECRET_$_"
    }
  }
}

function Set-Secrets {
  Remove-UnsafeSecrets
  Get-ChildItem env: | ? { $_.Name.StartsWith('DOJ_STUDIO_SECRET_') } | % {
    New-Item -Name $_.Name.Replace('DOJ_STUDIO_SECRET_', '') -Value $_.Value -Path Env: -Force | Out-Null
    Remove-Item -Path "Env:\$($_.Name)" -Force
  }
}

$ErrorActionPreference="stop"

# The current version of Dojo Studio
$script:version='@version@'
# The author of this program
$script:author='@author@'
# The short version of the program name which is used in logging output
$script:program="doj-studio"

if($env:SRC_PATH) {
  $script:SRC_PATH = Resolve-Path $env:SRC_PATH
}
else {
  $script:SRC_PATH = Get-Location
}
if($s) { $script:SRC_PATH = Resolve-Path $s }
$script:dir_name = $SRC_PATH.Path.Replace("$($SRC_PATH.Drive):\","").Replace("\","--")

if(!$env:DOJ_STUDIOS_HOME) {
  $script:DOJ_STUDIOS_HOME = "/doj/studios"
}
else {
  $script:DOJ_STUDIOS_HOME = $env:DOJ_STUDIOS_HOME
}

if(!$env:DOJ_STUDIO_ROOT) {
  $script:DOJ_STUDIO_ROOT = "$DOJ_STUDIOS_HOME/$dir_name"
}
else {
  $script:DOJ_STUDIO_ROOT = $env:DOJ_STUDIO_ROOT
}
if($o) { $script:DOJ_STUDIO_ROOT = $o }
$DOJ_STUDIO_ROOT = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($DOJ_STUDIO_ROOT)

if($k) {
  $env:DOJ_ORIGIN_KEYS = $k
}
else {
  $env:DOJ_ORIGIN_KEYS = $env:DOJ_ORIGIN
}

if($h) { $script:printHelp = $true }
if($n) { $script:doNotMount = $true }
if($q) { $script:quiet = $true }

$currentVerbose = $VerbosePreference
if($v) { $VerbosePreference = "Continue" }

if($w) {
  Write-Warning "The -w argument has been deprecated."
  Write-Warning "A local Studio is now the default on Windows."
  Write-Warning "Alternatively you may use the -D argument to use a Docker Studio."
}

if(!$w -and !(Test-InContainer)) {
  Write-Warning "Using a local Studio. To use a Docker studio, use the -D argument."
}

try {
  switch ($command) {
    "new" { New-Studio }
    "run" { Invoke-StudioRun $commandVal }
    "rm" { Remove-Studio }
    "enter" { Enter-Studio }
    "build" { Invoke-StudioBuild $commandVal $R }
    "version" { Write-Host "$program $version" }
    "help" { Write-Help }
    default {
      Write-Help
      Write-Error "Invalid Argument $command"
    }
  }
}
finally { $VerbosePreference = $currentVerbose }
