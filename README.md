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

## Notes on Azure

### VM Creation

- Select the Ubuntu 18.04 VM template.
- Use a VM size that supports hyperthreading (e.g. D4s_v3).
- Do not select _Accelerated Networking_ option when creating the VM.
- Use the `az` CLI tool to create an additional network adaptor with Accelerated Networking enabled. e.g.:

```
az network nic create --resource-group centigo --name cassance596 --vnet-name centigo-vnet --subnet default --accelerated-networking true --network-security-group cassance-nsg
```

- Use the portal or the CLI tool to attach the NIC to the VM.

### Single Sender & Receiver Test

Use the following on `cassance` and `hightent` to test DPDK with `testpmd`:

Sender (`cassance.southcentralus.cloudapp.azure.com`):

```
testpmd -l 0-3 -n 1 -w ac2a:00:02.0 --vdev="net_vdev_netvsc0,iface=eth1" -- --port-topology=chained --nb-cores 1 --forward-mode=txonly --eth-peer=1,00:0d:3a:70:25:75 --stats-period 1
```

Receiver (`hightent.southcentralus.cloudapp.azure.com`):

```
testpmd -l 0-3 -n 1 -w aa89:00:02.0 --vdev="net_vdev_netvsc0,iface=eth1" -- --port-topology=chained --nb-cores 1 --forward-mode=rxonly --eth-peer=1,00:0d:3a:70:25:75 --stats-period 1
```

### Resources

- [Set up DPDK in a Linux virtual machine](https://docs.microsoft.com/en-us/azure/virtual-network/setup-dpdk).
- [Create a Linux virtual machine with Accelerated Networking](https://docs.microsoft.com/en-us/azure/virtual-network/create-vm-accelerated-networking-cli).

