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

Building
--------

> **Follow these instructions to build Demikernel on a fresh Ubuntu 18.04 system.**

**1. Clone This Repository**
```
export WORKDIR=$HOME                                                  # change this to whatever you want
cd $WORKDIR                                                           # switch to working directory
git clone --recursive https://github.com/demikernel/demikernel.git    # recursive clone
```


**2. Install Prerequisites**
```
cd $WORKDIR/demikernel                                            # switch to working directory
sudo -H scripts/setup/debian.sh                                   # install third party libraries
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh    # get Rust toolchain
make dpdk                                                         # build DPDK
```

**3. Build LibOSes**
```
cd $WORKDIR/demikernel    # switch to working directory
make demikernel           # build everything
```

Code of Conduct
---------------

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

Contributing
------------

See [CONTRIBUTING.md](./CONTRIBUTING.md) for details regarding how to contribute
to this project.

Usage Statement
--------------

The _Demikernel_ is prototype code. As such, we provide no guarantees that it will
work and you are assuming any risks with using the code.  We welcome comments
and feedback. Please send any questions or comments to
[irene.zhang@microsoft.com](mailto:irene.zhang@microsoft.com) or
[ppenna@microsoft.com](ppenna@microsoft.com).  By sending feedback, you are
consenting to your feedback being used in the further development of this
project.
