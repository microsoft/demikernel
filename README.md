Datacenter Operating system
===

These instructions are for setting up a clean build of Demeter on a clean install of Ubuntu 18.04

## Installing Mellanox drivers

- Get drivers from Mellanox website: http://www.mellanox.com/page/products_dyn?product_family=26&mtag=linux
- Run 'mlxnofed_install --upstream-libs --dpdk' for dpdk support
- on a clean Ubuntu install, you may have to run 'apt-get install libnl-3-dev' to get the Mellanox script to complete
- The mlxnofed script should install the RDMA library headers that are needed.  DO NOT install the RDMA ibverbs libraries from Ubuntu.
## Building

- On Debian systems, run `scripts/setup/debian.sh` to install prerequisites.
- Make a directory for the build. We suggest `$DATACENTEROS/build/debug` or `$DATACENTEROS/build/release`.
- Run CMake from the build directory, passing the source directory in as an argument.
- Set the `CMAKE_BUILD_TYPE` variable to `Release` if you want an optimized build. You can do this with the CLI (`ccmake`) or the GUI (`cmake-gui`).
- Set the `DPDK_USE_MELLANOX_PMD` option to `ON` if you need DPDK compiled with support for Mellanox ConnectX-4 NICs.
- Run `make` from the build directory.

### Cleaning

- You can clean the build by deleting the build directory and starting over.
- Run `scripts/build/clean.sh` to thouroughly clean the repository. Be warned that this will delete any untracked files that you have not yet staged.

## Configuring

Some system-wide configuration needs to be performed before DPDK will function.

### Hugepage Support

- Enter the DPDK `usertools` directory (`cd $DATACENTEROS/submodules/mtcp/dpdk-17.08/usertools`).
- Run the `dpdk-setup.sh` script with administrative privileges (`sudo `./dpdk-setup.sh`).
- At the menu, select a *hugepage* mapping option, depending upon the system your using (option `19` or `20`).
- Specify the number of pages for each node (e.g. `1024`).
- Once at the menu, select *Exit Script* (option `33`).
