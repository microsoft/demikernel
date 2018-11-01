Datacenter Operating system
===

### Installation

- On Debian systems, run `scripts/setup/debian.sh` to install prerequisites.
- Run `scripts/build/mtcp.sh` to build `mtcp`.

+ cd ./libos/libspdk/
+ bash setup_spdk.sh
+ cmake .
    + if cmake version not higher enough, install from source code
+ make

NOTE: if using Mellanox ConnectX-4 NICS, DPDK needs to be compiled
with the CONFIG_RTE_LIBRTE_MLX5_PMD=y in config/common_base

