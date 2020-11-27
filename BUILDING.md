Datacenter Operating system
===

These instructions are for setting up a clean build of Demeter on a clean install of Ubuntu 18.04

## Installing Mellanox drivers

- Get drivers from Mellanox website: http://www.mellanox.com/page/products_dyn?product_family=26&mtag=linux
- Run 'mlxnofed_install --upstream-libs --dpdk' for dpdk support
- on a clean Ubuntu install, you may have to run 'apt-get install libnl-3-dev' or 'libnl-route-3-dev'to get the Mellanox script to complete because it seems to not be updated with all dependencies
- The mlxnofed script should install the RDMA library headers that are needed.  DO NOT install the RDMA ibverbs libraries from Ubuntu.

## Checking out the code
- Remember to use 'git clone --recursive' to check out the necessary submodules to build the Demikernel, including DPDK and Hoard.

## Building

- On Debian systems, run `scripts/setup/debian.sh` to install prerequisites.
- Install Rust nightly. Run 'curl https://sh.rustup.rs -sSf | sh'
- You need to use a nightly build of Rust. Currently, the build is only tested to work with `nightly-2020-05-14`. You can install this by running `rustup override set nightly-2020-05-14` from the `src/rust` directory.
- Make a directory for the build. We suggest `$DATACENTEROS/build/debug` or `$DATACENTEROS/build/release`.
- Run CMake from the build directory, passing the source directory in as an argument.
- Set the `CMAKE_BUILD_TYPE` variable to `Release` if you want an optimized build. You can do this with the CLI (`ccmake`) or the GUI (`cmake-gui`).
- Set the `DPDK_MLX4_SUPPORT` and/or `DPDK_MLX5_SUPPORT` option to `ON` if you need DPDK compiled with support for Mellanox NICs.
- Run `make` from the build directory.

### Cleaning

- You can clean the build by deleting the build directory and starting over.
- Run `scripts/build/clean.sh` to thouroughly clean the repository. Be warned that this will delete any untracked files that you have not yet staged.

## Configuring

The Demikernel libOSes and apps are configured through a config.yaml file. You can find an example in `src/c++/config.yaml` 

DPDK requires a white list to ensure that it does not co-opt all of your network connections. Use 'ifconfig' to find the name of your DPDK network device, then 'ethtool -i' to find the PCI bus ID. Update the PCI bus ID in the eal args in your config.yaml file.

In addition, some system-wide configuration needs to be performed before DPDK will function.

### Hugepage Support

- Enter the DPDK `usertools` directory (`cd $DATACENTEROS/submodules/mtcp/dpdk-17.08/usertools`).
- Run the `dpdk-setup.sh` script with administrative privileges (`sudo `./dpdk-setup.sh`).
- At the menu, select a *hugepage* mapping option, depending upon the system your using (option `19` or `20`).
- Specify the number of pages for each node (e.g. `1024`).
- Once at the menu, select *Exit Script* (option `33`).

## Building on Azure

### VM Creation

- Select the Ubuntu 18.04 VM template.
- Use a VM size that supports hyperthreading (e.g. D4s_v3).
- Do not select _Accelerated Networking_ option when creating the VM.
- Use the `az` CLI tool to create an additional network adaptor with Accelerated Networking enabled. e.g.:

```
az network nic create --resource-group centigo --name cassance596 --vnet-name centigo-vnet --subnet default --accelerated-networking true --network-security-group cassance-nsg
```

- Use the portal or the CLI tool to attach the NIC to the VM.
- Be sure to allow traffic on the ports that you are using!!

### Building Demikernel for Azure
1. Clone git repo with recurse-submodules flag.
2. Install Azure packages by running `script/setup/ubuntu-azure.sh`.
3. Create a build directory, I use 'demikernel/build'.
4. Turn on Azure support and CX5 support by running `cmake -DAZURE_SUPPORT=ON -DDPDK_MLX5_SUPPORT=ON -DCMAKE_BUILD_TYPE=Release ..` in your build directory or using `ccmake ..` and setting the cmake flags manually. 
5. Compile DPDK by running 'make dpdk' in the build directory. After this you should be able to run the testpmd single sender & receiver test below.
6. Use config options from testpmd in the config.yaml file. The DPDK interface is usually the slave interface listed first by `ifconfig`.  Using that interface id, run `ethtool -i` to find your PCI ID.
7. Don't forget to turn on hugepage support and run as root. Running `scripts/setup/dpdk.sh` will turn on huge page support and install needed kernel modules.
8. If you're using Catnip, set the `disable_arp` flag in `config.yaml` since the Azure VLAN will set the destination address in the Ethernet header automatically based on the destination IP address.

### Single Sender & Receiver Test

The testpmd executable can be found in your build directory: `build/ExternalProject/dpdk/bin/testpmd`

Use the following on `cassance` and `hightent` to test DPDK with `testpmd`:

Sender (`cassance.southcentralus.cloudapp.azure.com`):

```
testpmd -l 0-3 -n 1 -w ac2a:00:02.0 --vdev="net_vdev_netvsc0,iface=eth1" -- --port-topology=chained --nb-cores 1 --forward-mode=txonly --eth-peer=1,00:0d:3a:5e:4f:6e --stats-period 1
```

Receiver (`hightent.southcentralus.cloudapp.azure.com`):

```
testpmd -l 0-3 -n 1 -w aa89:00:02.0 --vdev="net_vdev_netvsc0,iface=eth1" -- --port-topology=chained --nb-cores 1 --forward-mode=rxonly --eth-peer=1,00:0d:3a:70:25:75 --stats-period 1
```

### Resources

- [Set up DPDK in a Linux virtual machine](https://docs.microsoft.com/en-us/azure/virtual-network/setup-dpdk).
- [Create a Linux virtual machine with Accelerated Networking](https://docs.microsoft.com/en-us/azure/virtual-network/create-vm-accelerated-networking-cli).

