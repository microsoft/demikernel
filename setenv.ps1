$RustPath = "$Env:UserProfile\.cargo\bin"
$GitPath = "$Env:ProgramFiles\Git\cmd"
$Env:Path += ";$Env:ProgramFiles\Git\cmd;$Env:HOME\.cargo\bin"
$Env:Path += ";$GitPath;$RustPath"

$Env:DEBUG = "yes"
$Env:RUST_LOG = "trace"
$Env:RUST_BACKTRACE = "full"
$Env:LIBOS = "catpowder"
$Env:CONFIG_PATH = "config.yaml"


# Launch MS VC PowerShell.
$VsInstallPath = & "${Env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" "-latest" "-property" "installationPath"
Import-Module (Join-Path $VsInstallPath "Common7\Tools\Microsoft.VisualStudio.DevShell.dll")
Enter-VsDevShell -VsInstallPath $VsInstallPath -SkipAutomaticLocation -DevCmdArguments "-arch=x64 -host_arch=x64"
