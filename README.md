Demikernel
==========

[![Join us on Slack!](https://img.shields.io/badge/chat-on%20Slack-e01563.svg)](https://join.slack.com/t/demikernel/shared_invite/zt-11i6lgaw5-HFE_IAls7gUX3kp1XSab0g)
[![Build Libs](https://github.com/demikernel/demikernel/actions/workflows/build-libs.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/build-libs.yml)
[![Build Tests](https://github.com/demikernel/demikernel/actions/workflows/build-tests.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/build-tests.yml)

_Demikernel_ is a library operating system (LibOS) architecture designed for use
with kernel-bypass I/O devices. This architecture offers a uniform system call
API across kernel-bypass technologies (e.g., RDMA, DPDK) and OS functionality
(e.g., a user-level networking stack for DPDK).

To read more about the motivation behind the _Demikernel_, check out
this [blog
post](http://irenezhang.net/blog/2019/05/21/demikernel.html).

To get details about the system, read our paper in [SOSP '21](https://doi.org/10.1145/3477132.3483569).

> To read more about Demikernel check out https://aka.ms/demikernel.

Building
--------

> **Follow these instructions to build Demikernel on a fresh Ubuntu 20.04 system.**

**1. Clone This Repository**
```
export WORKDIR=$HOME                                                  # Change this to whatever you want.
cd $WORKDIR                                                           # Switch to working directory.
git clone --recursive https://github.com/demikernel/demikernel.git    # Recursive clone.
cd $WORKDIR/demikernel                                                # Switch to repository's source tree.
```

**2. Install Prerequisites (Only Once)**
```
sudo -H scripts/setup/debian.sh                                   # Install third party libraries.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # Get Rust toolchain.
```

**3. Build DPDK Libraries (For Catnip and Only Once)**
```
./scripts/setup/dpdk.sh
```

**4. Build Demikernel with Default Parameters**
```
make
```

**5. Build Demikernel with Custom Parameters (Optional)**
```
make LIBOS=[catnap|catnip|catpowder]   # Build using a specific LibOS.
make DRIVER=[mlx4|mlx5]                # Build using a specific driver.
```

Running
--------
> **Follow these instructions to run examples that are shipped in the source tree**.

**1. Setup Configuration File (Only Once)**

- Copy the template from `scripts/config/default.yaml` to `$HOME/config.yaml`.
- Open the file in `$HOME/config.yaml` for editing and do the following:
    - Change `XX.XX.XX.XX` to match the IPv4 address of your server host.
    - Change `YY.YY.YY.YY` to match the IPv4 address of your client host.
    - Change `PPPP` to the port number that you will expose in the server host.
    - Change `ZZ.ZZ.ZZ.ZZ` to match the IPv4 address that in the local host.
    - Change `ff:ff:ff:ff:ff:ff` to match the MAC address in the local host.
    - Change `abcde` to match the name of the interface in the local host.
    - Change the `arp_table` according to your setup.
    - If using DPDK, change `WW:WW.W` to match the PCIe address of your NIC.
- Save the file.

**2. Run UDP Push-Pop Demo**
```
# Server-Side
PEER=server TEST=udp_push_pop sudo -E make LIBOS=catnap test-system

# Client-Side
PEER=client TEST=udp_push_pop sudo -E make LIBOS=catnap test-system
```

**3. Run UDP Ping-Pong Demo**
```
# Server-Side
PEER=server TEST=udp_ping_pong sudo -E make LIBOS=catnap test-system

# Client-Side
PEER=client TEST=udp_ping_pong sudo -E make LIBOS=catnap test-system
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
