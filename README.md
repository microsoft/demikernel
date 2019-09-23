Demikernel
==========

The Demikernel is a library operating system architecture designed for
use with kernel-bypass I/O devices.  The Demikernel architecture
offers a uniform system call API across kernel-bypass technologies
(e.g., RDMA, DPDK) and OS functionality (e.g., a user-level networking
stack for DPDK).  Demeter OS is our first Demikernel OS, currently in
the prototype stage.

To read more about the motivation behind the Demikernel, check out
this [blog
post](http://irenezhang.net/blog/2019/05/21/demikernel.html).

To get details about the system, read our [HotOS
paper](http://irenezhang.net//papers/demikernel-hotos19.pdf). 

Source-code Organization
----------
Application wanting to use the Demikernel/Demeter OS should include
header files from `include/dmtr`.  Most of the Demeter libOS code is
contained in `src/c++/libos`.  Example applications that work with the
Demikernel API are in `src/c++/apps/` and `submodules/redis-*/`.

* `include/dmtr/` - Demikernel/Demeter header files
* `scripts/` - scripts for using the Demikernel/Demeter OS
  * `build/` - build scripts
  * `cmake/` - cmake build scripts
  * `run/` - scripts for running experiments
  * `setup/` - scripts for setting up environment to use the
    Demikernel
* `src/c++/` - most of the Demikernel/Demeter libOS code
  * `apps/` - C++ Demikernel example apps
    * `echo/` - A simple multi-client echo server
  * `latency/` - performance measurement code for measuring latencies
    of various components
  * `libos/` - All Demikernel/Demeter libOS components
    * `common/` - code common to all libOSes regardless of
      kernel-bypass device. Implements functionality across queues.
    * `lwip/` - simple DPDK libOS based on the [lwIP
      stack](https://savannah.nongnu.org/projects/lwip/). 
    * `posix/` - libOS without kernel-bypass using the POSIX API and
      the Linux kernel for I/O
    * `rdma/` - RDMA libOS using two-sided operations to implements Demikernel queues
    * `rdmacm-common/`
    * `spdk` - simple libOS that implements a single log file on a
      SPDK drive. (untested)
* `submodules` - Demikernel/Demeter external dependencies and example applications
  * `Heap-Layers/` - needed by Hoard
  * `Hoard/` - unmodified Hoard memory allocator
  * `HoardRdma` - modified version of Hoard that registers all of the
  heap for RDMA access
  * `dpdk/` - version of DPDK used for Demikernel/Demeter
  * `redis-lwip` - modified Redis to work with the Demikernel
  interfaces, set up to compile with lwIP libOS
  * `redis-posix` - modified Redis to work with the Demikernel
  interfaces, set up to compile with POSIX libOS
  * `redis-rdma` - modified Redis to work with the Demikernel
  interfaces, set up to compile with RDMA libOS
  * `redis-vanilla` - unmodified version of Redis
  * `spdk` - version of SPDK used for spdk libOS
  * `tapir-lwip` - version of TAPIR modified to run with Demikernel
  syscall API, set up to compile with lwIP libOS
  * `tapir-posix` - version of TAPIR modified to run with Demikernel
  syscall API, set up to compile with POSIX libOS
  * `tapir-rdma` - version of TAPIR modified to run with Demikernel
  syscall API, set up to compile with RDMA libOS
  
Demikernel Syscall API
---------
Refer to [syscall documentation API.md](API.md)

Building
--------

See [BUILDING.md](./BUILDING.md) for build instructions.

Code of Conduct
---------------

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

Contributing
------------

See [CONTRIBUTING.md](./CONTRIBUTING.md) for details regarding how to contribute to _Demikernel_.


Usage Statement
--------------

The Demikernel is prototype code. As such, we provide no guarantees
that it will work and you are assuming any risks with using the code.
We welcome comments and feedback. Please send any questions or
comments to irene.zhang@microsoft.com.  By sending feedback, you are
consenting to your feedback being used in the further development of
this project.
