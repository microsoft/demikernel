# Rust implementations of our latency and throughput experiments
This crate includes simple echo servers that use both Catnip's TCP and UDP stacks as well as
directly implementing UDP on top of DPDK.

- `{tcp,udp,raw_udp}_echo.rs`: Closed loop echo application with a single client
- `{tcp,udp,raw_udp}_throughput.rs`: Server that returns whatever's sent to it (and for TCP accepts
  any number of connections), continuously logging throughput, with multiple clients that
  periodically log latency histograms.

As with the `catnip_libos` crate, be sure to set `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH` when
building and running these experiments.

## Table 2 (latency for 64 byte messages)
### Catnip/UDP
Client:
```
root@prometheus3:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=64 USE_JUMBO=1 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/udp_echo ~/config3.yaml
```
Server:
```
root@prometheus5:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=64 USE_JUMBO=1 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/udp_echo ~/config5.yaml
```

### Catnip/TCP
Client:
```
root@prometheus3:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=64 USE_JUMBO=1 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/tcp_echo ~/config3.yaml
```
Server:
```
root@prometheus5:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=64 USE_JUMBO=1 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/tcp_echo ~/config5.yaml
```

## Table 3 (latency for 64 bytes messages with logging)
See the `spdk-build` branch.

## Figure 4 (throughput with varying message sizes)
For each of these experiments, vary the `BUFFER_SIZE` environment variable to change the message size.

### Catnip/UDP
Client:
```
root@prometheus3:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=$((256 * 1024)) USE_JUMBO=1 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/udp_echo ~/config3.yaml
```
Server:
```
root@prometheus5:~/src/demikernel/src/rust/catnip_libos# BUFFER_SIZE=$((256 * 1024)) USE_JUMBO=1 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 MTU=9216 ../target/release/udp_echo ~/config5.yaml
```

### Raw/UDP
Client:
```
root@prometheus3:~/src/demikernel/src/rust/catnip_libos# UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 SRC_MAC=b8:83:03:70:98:34 DST_MAC=b8:83:03:70:98:44 BUFFER_SIZE=$((256 * 1024)) ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 ../target/release/raw_udp_echo ~/config3.yaml
```
Server:
```
root@prometheus5:~/src/demikernel/src/rust/catnip_libos# UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 DST_MAC=b8:83:03:70:98:34 SRC_MAC=b8:83:03:70:98:44 BUFFER_SIZE=$((256 * 1024)) ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 ../target/release/raw_udp_echo ~/config5.yaml
```

### Catnip/TCP
Client:
```
root@prometheus3:~/src/demikernel/src/rust/catnip_libos# TCP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 BUFFER_SIZE=$((256 * 1024)) ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 ../target/release/tcp_echo ~/config3.yaml
```
Server:
```
root@prometheus5:~/src/demikernel/src/rust/catnip_libos# TCP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 BUFFER_SIZE=$((256 * 1024)) ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=1000 MSS=9000 ../target/release/tcp_echo ~/config5.yaml
```

## Figure 5 (latency vs. throughput for a single core server)
### Catnip/UDP
Server:
```
root@prometheus5:~/src/demikernel/src/rust/experiments# BUFFER_SIZE=$((1024)) UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 DST_MAC=b8:83:03:70:98:34 SRC_MAC=b8:83:03:70:98:44 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 ../target/release/udp_throughput ~/config5.yaml
```
Clients (I maxed out throughput with two machines running 8 clients each):
```
root@prometheus6:~/src/demikernel/src/rust/experiments# NUM_CLIENTS=8 BUFFER_SIZE=$((1024)) UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 SRC_MAC=b8:83:03:70:98:34 DST_MAC=b8:83:03:70:98:44 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 ../target/release/udp_throughput ~/config6.yaml
```

### Raw/UDP
Server:
```
root@prometheus5:~/src/demikernel/src/rust/experiments# BUFFER_SIZE=1024 UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 DST_MAC=b8:83:03:70:98:34 SRC_MAC=b8:83:03:70:98:44 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 numactl -N0 -m0 ../target/release/raw_udp_throughput ~/config5.yaml
```
Clients:
```
root@prometheus6:~/src/demikernel/src/rust/experiments# NUM_CLIENTS=8 BUFFER_SIZE=1024 UDP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 DST_MAC=b8:83:03:70:98:44 SRC_MAC=b8:83:03:70:88:98 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 numactl -N0 -m0 ../target/release/raw_udp_throughput ~/config3.yaml
```

### Catnip/TCP
Server:
```
root@prometheus5:~/src/demikernel/src/rust/experiments# BUFFER_SIZE=$((1024)) RUST_LOG=error,catnip::protocols::tcp::established::background=error TCP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 DST_MAC=b8:83:03:70:98:34 SRC_MAC=b8:83:03:70:98:44 ECHO_SERVER=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 ../target/release/tcp_throughput ~/config5.yaml
```
Clients:
```
root@prometheus3:~/src/demikernel/src/rust/experiments# RUST_LOG=error NUM_CLIENTS=8 BUFFER_SIZE=$((1024)) TCP_CHECKSUM_OFFLOAD=1 USE_JUMBO=1 MTU=9216 SRC_MAC=b8:83:03:70:98:34 DST_MAC=b8:83:03:70:98:44 ECHO_CLIENT=1 LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/install/lib/x86_64-linux-gnu NUM_ITERS=10000 MSS=9000 ../target/release/tcp_throughput ~/config3.yaml
```

## Figure 7 (Redis throughput)
This doesn't strictly belong here, but here are the commands for reproducing the Redis
experiments. There's a hack to pass in the YAML configuration path as a `CONFIG_PATH` environment
variable (rather than through `dmtr_init`), so be sure to set that.

Also, be sure to change `bind` in the Redis config to the interface's IP.

### Without logging
Build:
```
sujayakar@prometheus3:~/src/demikernel/build$ make redis-dpdk-catnip

```
Server:
```
sujayakar@prometheus3:~/src/demikernel/build$ LD_LIBRARY_PATH=~/src/demikernel/build/ExternalProject/dpdk/lib/ CONFIG_PATH=$HOME/config3.yaml submodules/redis-dpdk-catnip/bin/redis-server ~/redis3.conf
```

Since we only care about throughput for this experiment, we can just use Redis's built-in benchmark
on a few other machines.

Gets:
```
sujayakar@prometheus3:~$ for i in $(seq 5 9); do nohup ssh prometheus$i -- 'echo $(hostname):$(~/src/demikernel/submodules/redis-vanilla/src/redis-benchmark -h 198.19.200.3 -p 6379 -t get -n 500000 -r 1000000 -d 64  -c 24 -q --csv)' < /dev/null &> /tmp/get$i.out & done
sujayakar@prometheus3:~$ cat /tmp/get*.out | cut -d ',' -f2 | sed 's/"//g' | awk '{total += $1} END {print total}'
207271
```

Sets:
```
sujayakar@prometheus3:~$ for i in $(seq 5 9); do nohup ssh prometheus$i -- 'echo $(hostname):$(~/src/demikernel/submodules/redis-vanilla/src/redis-benchmark -h 198.19.200.3 -p 6379 -t set -n 500000 -r 1000000 -d 64  -c 24 -q --csv)' < /dev/null &> /tmp/set$i.out & done
sujayakar@prometheus3:~$ cat /tmp/set*.out | cut -d ',' -f2 | sed 's/"//g' | awk '{total += $1} END {print total}'
157590

```

### With logging
See `spdk-build` branch. Set `appendonly yes` in the Redis config to turn on logging.
