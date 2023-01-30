# POSIX SHIM for Demikernel

This project provides a POSIX SHIM for [Demikernel](https://github.com/demikernel/demikernel).

> Demikernel is a libOS architecture for kernel-bypass devices. Read more about it at https://aka.ms/demikernel.

## Building

```bash
# Location where Demikernel artifacts should be searched for.
# You may want to change this to match your setup.
export DEMIKERNEL_HOME=$HOME

# Location where the POSIX SHIM library should be placed.
# It is recommended that you set this to $DEMIKERNEL_HOME.
export INSTALL_PREFIX=$DEMIKERNEL_HOME

# Build the POSIX SHIM library.
make all

# Install the POSIX SHIM library (optional).
make install
```

## Testing

```bash
# Build the hello example.
make all -C test/hello

# Run the hello example with Linux.
make run -C test/hello

# Run the hello example with Demikernel.
export LIBOS=catnap
export CONFIG_PATH=$DEMIKERNEL_HOME/config.yaml
export RUST_LOG=trace
export C_TRACE=trace
LD_PRELOAD=$DEMIKERNEL_HOME/lib/libshim.so make run -C test/hello
```

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Usage Statement

This project is a prototype. As such, we provide no guarantees that it will
work and you are assuming any risks with using the code. We welcome comments
and feedback. Please send any questions or comments to one of the following
maintainers of the project:

- [Pedro Henrique Penna](https://github.com/ppenna) - [ppenna@microsoft.com](mailto:ppenna@microsoft.com)

> By sending feedback, you are consenting that it may be used  in the further
> development of this project.
