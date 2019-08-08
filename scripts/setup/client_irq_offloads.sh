#! /bin/bash
ethtool -N eno5 flow-type tcp4 dst-ip 198.19.100.2 src-ip 198.19.100.1 src-port 6789 action 8
ethtool -N eno5 flow-type tcp4 dst-ip 198.19.100.2 src-ip 198.19.100.3 src-port 6789 action 9
