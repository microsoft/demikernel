# Linux Scripts to Build and run Demikernel Programs

## Linux

Run `build.bash` from the `scripts/linux` directory:

```bash
bash ./scripts/linux/build.bash
```

### Arguments

- `--libos <catnap|catnip|catpowder>`: Select LibOS to build with (default: `catnap`).
- `--config <debug|release>`: Build configuration (default: `release`).
- `--profiler`: Include profiling information in the build.

See [build.bash](build.bash) for details.

## Generate config

Run `generate-config.bash` from the repository root directory

```bash
bash ./scripts/linux/generate-config.bash
```

## Adjust environment variables

After generating config, edit `env.bash` to adjust LibOS, Rust Log level and other environment variables. It will be used for running Demikernel sample server and client.

## Run Client Program

Run `run-client.bash` from the repository root directory

## Run Server Program

Run `run-server.bash` from the repository root directory
