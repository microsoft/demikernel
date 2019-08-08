ethtool -N eno5 flow-type tcp4 src-ip 198.19.100.2 dst-ip 198.19.100.1 dst-port 6789 action 8
ethtool -N eno5 flow-type tcp4 src-ip 198.19.100.2 dst-ip 198.19.100.3 dst-port 6789 action 9
