<#
.SYNOPSIS
    This script provides helpers for building Demikernel.

.PARAMETER config
    The debug or release configuration to build for.

.PARAMETER libos
    Using Catnap or Catpowder LibOS to build Demikernel.

.PARAMETER profiler
    If profiler parameter is passed in to this script, build Demikernel with extra information for Profiler

#>

#Requires -Version 7.2

param (
    [Parameter(Mandatory = $false)]
    [ValidateSet("debug", "release")]
    [string]$config = "release",

    [Parameter(Mandatory = $false)]
    [ValidateSet("catnap", "catpowder")]
    [string]$libos = "catnap",

    [Parameter(Mandatory = $false)]
    [switch]$profiler = $false
)

# Initialize environment variables
$env:RUSTC_BOOTSTRAP = 1
$env:RUST_LOG = "trace"
$env:RUST_BACKTRACE = "full"
$env:LIBOS = "$libos"

# Start Visual Studio Developer Powershell
$VsInstallPath = &(Join-Path ${Env:ProgramFiles(x86)} '\Microsoft Visual Studio\Installer\vswhere.exe') -latest -property installationPath
Import-Module (Join-Path $VsInstallPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $VsInstallPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'

$Arguments = ""
if ($config -eq "debug") {
    $Arguments += "DEBUG=yes "
}
if ($profiler) {
    $Arguments += "PROFILER=yes "
} else {
    $Arguments += "PROFILER=no "
}
$Command = "${Arguments}all"

##############################################################
#                     Main Execution                         #
##############################################################
Write-Host "Using LibOS: $libos; running 'nmake $Command' to build Demikernel"

nmake $Command