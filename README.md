# Demikernel

[![Join us on Slack!](https://img.shields.io/badge/chat-on%20Slack-e01563.svg)](https://join.slack.com/t/demikernel/shared_invite/zt-11i6lgaw5-HFE_IAls7gUX3kp1XSab0g)
[![Catnip LibOS](https://github.com/demikernel/demikernel/actions/workflows/catnip.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catnip.yml)
[![Catnap LibOS](https://github.com/demikernel/demikernel/actions/workflows/catnap.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catnap.yml)
[![Catmem LibOS](https://github.com/demikernel/demikernel/actions/workflows/catmem.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catmem.yml)
[![Catpowder LibOS](https://github.com/demikernel/demikernel/actions/workflows/catpowder.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catpowder.yml)
[![Catcollar LibOS](https://github.com/demikernel/demikernel/actions/workflows/catcollar.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catcollar.yml)
[![Catloop LibOS](https://github.com/demikernel/demikernel/actions/workflows/catloop.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catloop.yml)
[![Catnapw LibOS](https://github.com/demikernel/demikernel/actions/workflows/catnapw.yml/badge.svg)](https://github.com/demikernel/demikernel/actions/workflows/catnapw.yml)

_Demikernel_ is a library operating system (LibOS) architecture designed for use
with kernel-bypass I/O devices. This architecture offers a uniform system call
API across kernel-bypass technologies (e.g., RDMA, DPDK) and OS functionality
(e.g., a user-level networking stack for DPDK).

To read more about the motivation behind the _Demikernel_, check out this
[blog post](http://irenezhang.net/blog/2019/05/21/demikernel.html).

To get details about the system, read our paper in [SOSP '21](https://doi.org/10.1145/3477132.3483569).

> To read more about Demikernel check out <https://aka.ms/demikernel>.

## Codename for LibOSes

- `catcollar` -- I/O Uring LibOS
- `catloop` -- TCP Socket Loopback LibOS
- `catmem` -- Shared Memory LibOS
- `catnap` -- Linux Sockets LibOS
- `catnip` -- DPDK LibOS
- `catpowder` -- Linux Raw Sockets
- `catnapw` -- Windows Sockets LibOS

## Documentation

- For instructions on development environment setup, see [doc/setup.md](./doc/setup.md).
- For instructions on building, see [doc/building.md](./doc/building.md).
- For instructions on testing and running, [doc/testing.md](./doc/testing.md).
- For instructions for running on CloudLab, see [doc/cloudlab.md](./doc/cloudlab.md).
- For documentation on the API, see documents in [man](./man).
- For instructions on how to contribute to this project, see [CONTRIBUTING](./CONTRIBUTING.md).

## Code of Conduct

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/)
or contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Usage Statement

This project is a prototype. As such, we provide no guarantees that it will
work and you are assuming any risks with using the code. We welcome comments
and feedback. Please send any questions or comments to one of the following
maintainers of the project:

- [Irene Zhang](https://github.com/iyzhang) - [irene.zhang@microsoft.com](mailto:irene.zhang@microsoft.com)
- [Pedro Henrique Penna](https://github.com/ppenna) - [ppenna@microsoft.com](mailto:ppenna@microsoft.com)

> By sending feedback, you are consenting that it may be used  in the further
> development of this project.
