# Windows script to build and run Demikernel

## Build Demikernel

Run `build.ps1` from the repository root directory:

```powershell
.\build.ps1
```

### Arguments

- `-libos <catnap|catpowder>`: Select LibOS to build with (default: `catnap`).
- `-config <debug|release>`: Build configuration (default: `release`).
- `-profiler`: Include profiling information in the build.

See [build.ps1](build.ps1) for details.

## Generate config

Run `generate-config.ps1` from the repository root directory

```powershell
.\tools\scripts\windows\generate-config.ps1 .\tools\scripts\config_template\config.yaml.template
```

Note: (only applicable to Catpowder LibOS) if your machine has VF network adapter, please adjust `config.yaml`'s `xdp_vf_interface_index` entry to the VF interface's index

## Adjust environment variables

After generating config, edit `env.ps1` to adjust LibOS, Rust Log level and other environment variables. It will be used for running Demikernel sample server and client.

## Run Client Program

Run `run-client.ps1` from the repository root directory

## Run Server Program

Run `run-server.ps1` from the repository root directory
