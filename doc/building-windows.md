# Building Demikernel on Windows

> **These instructions were tested on a fresh Windows Server 2019 Datacenter Version 10.0.17763.**

This document contains instructions on how to build Demikernel on Windows. The following content is covered:

- [1. Download and Install Development Tools (Run Once)](#1-download-and-install-development-tools-run-once)
- [2. Setup Your Environment Path in PowerShell (Run Always)](#2-setup-your-environment-path-in-powershell-run-always)
- [3. Launch Microsoft Visual Studio PowerShell (Run Always)](#3-launch-microsoft-visual-studio-powershell-run-always)
- [4. Clone This Repository (Run Once)](#4-clone-this-repository-run-once)
- [5. Build Demikernel (Run Always)](#5-build-demikernel-run-always)

## 1. Download and Install Development Tools (Run Once)

Download the following tools from your web browser:

- [Rustup Latest (64-Bit)](https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe)
- [LLVM 15.0.3 (64-Bit)](https://github.com/llvm/llvm-project/releases/download/llvmorg-15.0.3/LLVM-15.0.3-win64.exe)
- [Git 2.39.0 (64-Bit)](https://github.com/git-for-windows/git/releases/download/v2.39.0.windows.2/Git-2.39.0.2-64-bit.exe)

Alternatively, download these tools from a PowerShell terminal:

```powershell
Invoke-WebRequest -Uri https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe -OutFile rustup-init.exe
Invoke-WebRequest -Uri https://github.com/llvm/llvm-project/releases/download/llvmorg-15.0.3/LLVM-15.0.3-win64.exe -OutFile LLVM-15.0.3-win64.exe
Invoke-WebRequest -Uri https://github.com/git-for-windows/git/releases/download/v2.39.0.windows.2/Git-2.39.0.2-64-bit.exe
```

Execute the installer setup of each tool. Follow their default instructions. No special configuration is required.

## 2. Setup Your Environment Path in PowerShell (Run Always)

Open up a PowerShell terminal and run the following commands:

```powershell
# Add git to your path.
$GitPath = Join-Path $Env:ProgramFiles "\Git\cmd"
$Env:Path += $GitPath + ';'

# Add rust toolchain to your path.
$RustPath = Join-Path $Env:HOME "\.cargo\bin"
$Env:Path += $RustPath + ';'
```

## 3. Launch Microsoft Visual Studio PowerShell (Run Always)

In the same PowerShell terminal from the previous step, run the following commands:

```powershell
# Retrieve the location of Microsoft Visual Studio.
$VsInstallPath = &(Join-Path ${Env:ProgramFiles(x86)} '\Microsoft Visual Studio\Installer\vswhere.exe') -latest -property installationPath

# Launch a 64-bit development environment.
Import-Module (Join-Path $VsInstallPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $VsInstallPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
```

## 4. Clone This Repository (Run Once)

In the same PowerShell terminal from the previous step, run the following commands:

```powershell
# Switch to a directory where you want to store your local clone of Demikernel's source tree.
cd c:\

# Clone Demikernel's source tree and enter in it.
git clone --recursive https://github.com/demikernel/demikernel.git
cd demikernel
```

## 5. Build Demikernel (Run Always)

In the same PowerShell terminal from the previous step, run the following commands:

```powershell
# Build Demikernel in release mode with the default configuration.
nmake all

# Build Demikernel in debug mode with the default configuration.
nmake DEBUG=yes all
```
