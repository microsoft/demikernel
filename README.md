Demikernel
==========

_Demikernel_ is a library operating system (libOS) architecture designed for
use with kernel-bypass I/O devices.  The _Demikernel_ architecture
offers a uniform system call API across kernel-bypass technologies
(e.g., RDMA, DPDK) and OS functionality (e.g., a user-level networking
stack for DPDK).

To read more about the motivation behind the _Demikernel_, check out
this [blog
post](http://irenezhang.net/blog/2019/05/21/demikernel.html).

To get details about the system, read our [HotOS
paper](http://irenezhang.net//papers/demikernel-hotos19.pdf).

> To read more about Demikernel check out https://aka.ms/demikernel.

Building
--------

> **Follow these instructions to build Demikernel on a fresh Ubuntu 20.04 system.**

**1. Clone This Repository**
```
export WORKDIR=$HOME                                                  # Change this to whatever you want.
cd $WORKDIR                                                           # Switch to working directory.
git clone --recursive https://github.com/demikernel/demikernel.git    # Recursive clone.
```

**2. Install Prerequisites**
```
cd $WORKDIR/demikernel                                            # Switch to working directory.
sudo -H scripts/setup/debian.sh                                   # Install third party libraries.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # Get Rust toolchain.
./scripts/setup/dpdk.sh                                           # Build DPDK.
```

**3. Build Demikernel with Default Drivers**
```
cd $WORKDIR/demikernel    # Switch to working directory.
make                      # Build using default drivers.
```

**4. Build Demikernel with Custom Drivers (Optional)**
```
cd $WORKDIR/demikernel    # Switch to working directory.
make DRIVER=[mlx4|mlx5]   # Build using a custom driver.
```

Documentation
--------------

- Checkout UML Documentation in [`etc/README.md`](./etc/README.md)
- Checkout API Documentation (see instructions bellow)

**1. Build API Documentation (Optional)**
```
cargo doc --no-deps    # Build API Documentation
cargo doc --open       # Open API Documentation
```

Code of Conduct
---------------

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

Contributing
------------

See [CONTRIBUTING](./CONTRIBUTING) for details regarding how to contribute
to this project.

Usage Statement
--------------

This project is a prototype. As such, we provide no guarantees that it will
work and you are assuming any risks with using the code. We welcome comments
and feedback. Please send any questions or comments to one of the following
maintainers of the project:

- [Irene Zhang](https://github.com/iyzhang) - [irene.zhang@microsoft.com](mailto:irene.zhang@microsoft.com)
- [Pedro Henrique Penna](https://github.com/ppenna) - [ppenna@microsoft.com](mailto:ppenna@microsoft.com)

> By sending feedback, you are consenting that it may be used  in the further
> development of this project.
