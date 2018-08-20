Datacenter Operating system
===

### Installation
+ cd ./libos/libmtcp/ (NOTE, must go into that directory)
+ bash setup_mtcp.sh
+ cd ./libos/libspdk/
+ bash setup_spdk.sh
+ cmake .
    + if cmake version not higher enough, install from source code
+ make

NOTE: if using Mellanox ConnectX-4 NICS, DPDK needs to be compiled
with the CONFIG_RTE_LIBRTE_MLX5_PMD=y in config/common_base

