# Demikernel in Cloudlab

# Requesting resources on Cloudlab
- Look at the node IDs which have which have Mellanox ConnectX-4 or above in the hardware allocation in the hardware list (https://docs.cloudlab.us/hardware.html). 
    - Some of the ones that work: Utah (c6525-100g, c6525-25g, xl170, d6515), Clemson (r7525)
- Starting a new experiment
    - Choose Profile: small-lan
    - Parameterize
        - No of nodes: 2+
        - OS Image: Ubuntu 20.04
        - Optional Physical Node Type [REALLY IMPORTANT]: Put one of the IDSs mentioned above. Check availability here: https://www.cloudlab.us/resinfo.php?embedded=true
- Finalize
- Schedule
# Pre Requisites

Run the following scripts on all the nodes. 

## Installing Mellanox OFED (mlx5)
    wget https://content.mellanox.com/ofed/MLNX_OFED-5.5-1.0.3.2/MLNX_OFED_LINUX-5.5-1.0.3.2-ubuntu20.04-x86_64.tgz --no-check-certificate
    tar -xzvf MLNX_OFED_LINUX-5.5-1.0.3.2-ubuntu20.04-x86_64.tgz 
    cd MLNX_OFED_LINUX-5.5-1.0.3.2-ubuntu20.04-x86_64
    sudo ./mlnxofedinstall --upstream-libs --dpdk
## Installing Rust Cargo
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh


# Setup for Demikernel 
## Pull the code from repository

Fetch the code from the original repository and use `dev` branch for latest updates


    git clone --recursive https://github.com/demikernel/demikernel.git
    cd demikernel
    git checkout dev 


## Running setup scripts

Execute these scripts within the demikernel repository to setup pre-requisite for Ubuntu, Rust DPDK, and Hugepages


    ./scripts/setup/debian.sh
    ./scripts/setup/dpdk.sh
    ./scripts/setup/hugepages.sh


# Running Demikernel

Run `make all` to make sure everything compiles. 


## Configuration files

Edit the config files in `scripts/config/default.yaml` using the following steps.  

**Node 0 → Server**

 * Get information about the Mellanox NICs using `lshw`

   ``` 
   user@node0:~/demikernel/scripts/config$ sudo lshw | grep -i Mellanox -A 10
    vendor: Mellanox Technologies
                    physical id: 0.1
                    bus info: pci@0000:03:00.1
                    logical name: ens1f1
                    version: 00
                    serial: 9c:dc:71:5e:2f:71
                    capacity: 25Gbit/s
                    width: 64 bits
                    clock: 33MHz
                    capabilities: pciexpress vpd msix pm bus_master cap_list ethernet physical 1000bt-fd 10000bt-fd 25000bt-fd autonegotiation
                    configuration: autonegotiation=off broadcast=yes driver=mlx5_core driverversion=5.5-1.0.3 duplex=full firmware=14.18.2030 (HP_2420110034) ip=10.10.1.1 latency=0 link=yes multicast=yes
    ```

From the above output extract the following entities required for config file:

1. `my_interface_name` → `logical name` (`ens1f1`)
2. `my_link_address` → serial (`9c:dc:71:5e:2f:71`)

If there are multiple interfaces pick one of the interface. For this example, our interface name is `ens1f1`. 


* Finding out the IP of the system using `ifconfig`
```
    user@node0:~/demikernel/scripts/config$ ifconfig ens1f1
    ens1f1: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>  mtu 1500
            inet 10.10.1.1  netmask 255.255.255.0  broadcast 10.10.1.255
            inet6 fe80::9edc:71ff:fe5e:2f71  prefixlen 64  scopeid 0x20<link>
            ether 9c:dc:71:5e:2f:71  txqueuelen 1000  (Ethernet)
            RX packets 8  bytes 670 (670.0 B)
            RX errors 0  dropped 1  overruns 0  frame 0
            TX packets 19  bytes 1895 (1.8 KB)
            TX errors 1  dropped 0 overruns 0  carrier 1  collisions 0
```

IP: `10.10.1.1`


* Finding the PCI address for the `eal_init` section using `lshw`

```
    user@node0:~/demikernel/scripts/config$ sudo lshw -c network -businfo
    Bus info          Device     Class          Description
    =======================================================
    pci@0000:03:00.1  ens1f1     network        MT27710 Family [ConnectX-4 Lx]
```

From the above example, the PCI address will be `03:00.1`

**Node 1 → Client**

Apply the same commands to get the values for Node 1 which will be our client. 


* Get information about the NIC using `lshw`

```
    user@node1:~/demikernel/scripts/config$ sudo lshw | grep -i Mellanox -A 10
                    vendor: Mellanox Technologies
                    physical id: 0.1
                    bus info: pci@0000:03:00.1
                    logical name: ens1f1
                    version: 00
                    serial: 9c:dc:71:5d:41:31
                    capacity: 25Gbit/s
                    width: 64 bits
                    clock: 33MHz
                    capabilities: pciexpress vpd msix pm bus_master cap_list ethernet physical 1000bt-fd 10000bt-fd 25000bt-fd autonegotiation
                    configuration: autonegotiation=off broadcast=yes driver=mlx5_core driverversion=5.5-1.0.3 duplex=full firmware=14.18.2030 (HP_2420110034) ip=10.10.1.2 latency=0 link=yes multicast=yes
```

From the above output extract the following entities required for config file

1. `my_interface_name` → `logical name`  (`ens1f1`)
2. `my_link_address` → serial (`9c:dc:71:5d:41:31`)

If there are multiple interfaces pick one of the interface. For this example, our interface name is `ens1f1`. 


* Get information about IP using `ifconfig`
  ```
    user@node1:~/demikernel$ ifconfig ens1f1
    ens1f1: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>  mtu 1500
            inet 10.10.1.2  netmask 255.255.255.0  broadcast 10.10.1.255
            inet6 fe80::9edc:71ff:fe5d:4131  prefixlen 64  scopeid 0x20<link>
            ether 9c:dc:71:5d:41:31  txqueuelen 1000  (Ethernet)
            RX packets 14  bytes 1293 (1.2 KB)
            RX errors 0  dropped 1  overruns 0  frame 0
            TX packets 19  bytes 1895 (1.8 KB)
            TX errors 1  dropped 0 overruns 0  carrier 1  collisions 0
    ```
IP → `10.10.1.2`


* Finding the PCI address for the `eal_init` section using `lshw`
  ```
  
    user@node1:~/demikernel$ sudo lshw -c network -businfo
    Bus info          Device     Class          Description
    =======================================================
    pci@0000:03:00.1  ens1f1     network        MT27710 Family [ConnectX-4 Lx]
  
  ```

From the above example, the PCI address will be `03:00.1`

**Putting together config file for Node 0**

```
    # Copyright (c) Microsoft Corporation.
    # Licensed under the MIT license.
    
    server:
      bind:
        host: 10.10.1.1 (IP of Node 0)
        port: 12345
      client:
        host: 10.10.1.2 (IP of Node 1)
        port: 12345
    catnip:
      my_ipv4_addr: 10.10.1.1 (IP of Node 0)
      my_link_addr: "9c:dc:71:5e:2f:71" (Serial of ens1f1 in Node 0)
      my_interface_name: "ens1f1" 
      arp_table:
        <Link_addr_Node_x> : <IP_Node_x>
        "9c:dc:71:5e:2f:71": 10.10.1.1 
        "9c:dc:71:5d:41:31": 10.10.1.2
    dpdk:
            eal_init: ["-c", "0xff", "-n", "4", "-w", "03:00.1","--proc-type=auto"]
                                                          ||
                                                          \/
                                                      PCI address of ens1f1 in Node 0
    # vim: set tabstop=2 shiftwidth=2
```

**Putting together config file for Node 1**
```
    user@node1:~/demikernel$ cat scripts/config/cl_node_1.yaml 
    # Copyright (c) Microsoft Corporation.
    # Licensed under the MIT license.
    
    client:
      connect_to:
        host: 10.10.1.1 (IP of Node 0)
        port: 12345
      client:
        host: 10.10.1.2 (IP of Node 1)
        port: 12345
    catnip:
      my_ipv4_addr: 10.10.1.2 (IP of Node 1)
      my_link_addr: "9c:dc:71:5d:41:31" (Serial of ens1f1 in Node 1)
      my_interface_name: "ens1f1"
      arp_disable: true
    dpdk:
            eal_init: ["-c", "0xff", "-n", "4", "-w", "03:00.1","--proc-type=auto"]
                                                          ||
                                                          \/
                                                      PCI address of ens1f1 in Node 1
    # vim: set tabstop=2 shiftwidth=2
```

# Running the code

Running test files:

Edit the `CONFIG_PATH`  in `Makefile` with the absolute path of your config file:

Node 0 →
```
    export CONFIG_PATH ?= /users/user/demikernel/scripts/config/cl_node_0.yaml
```

Node 1 → 
```
    export CONFIG_PATH ?= /users/user/demikernel/scripts/config/cl_node_1.yaml
```

Run the following test scripts to make sure everything works

Node 0 (Running Server) → 
```
    user@node0:~/demikernel$ PEER=server TIMEOUT=300 TEST=udp_ping_pong sudo -E make test-catnip
```

Node 1 (Running Client) →
```
    user@node1:~/demikernel$ PEER=client TIMEOUT=300 TEST=udp_ping_pong sudo -E make test-catnip
```
