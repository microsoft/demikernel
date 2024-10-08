# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Read IFACE_NAME from standard input.
echo "Enter the name of the interface you want to use (e.g. eth1):"
read IFACE_NAME

# Read CONFIG_PATH from standard input.
echo "Enter the name of the configuration file you want to use (e.g. config.yaml):"
read CONFIG_PATH

# If CONFIG_PATH is empty, set it to the default value.
if [ -z "$CONFIG_PATH" ]; then
    CONFIG_PATH="config.yaml"
fi

export PCI_ADDR=`lspci | grep Ethernet | cut -d ' ' -f 1 | tail -n 1`
export IPV4_ADDR=`ifconfig $IFACE_NAME | grep "inet " | awk '{gsub(/^[ \t]+/,""); print$0, ""}' | cut -d " " -f 2`
export MAC_ADDR=`ifconfig $IFACE_NAME | grep "ether " | awk '{gsub(/^[ \t]+/,""); print$0, ""}' | cut -d " " -f 2`

cp -i ./scripts/config/azure.yaml $CONFIG_PATH

sed -i "s/abcde/$IFACE_NAME/g" $CONFIG_PATH

# Set IPV4_ADDR.
if [ -z "$IPV4_ADDR" ]; then
    echo "IPv4 address not found, skipping."
else
    echo "Writing IPV4_ADDR: $IPV4_ADDR"
    sed -i "s/XX.XX.XX.XX/$IPV4_ADDR/g" $CONFIG_PATH
fi

# Set MAC_ADDR.
if [ -z "$MAC_ADDR" ]; then
    echo "MAC address not found, skipping."
else
    echo "Writing MAC_ADDR: $MAC_ADDR"
    sed -i "s/ff:ff:ff:ff:ff:ff/$MAC_ADDR/g" $CONFIG_PATH
fi

# Set PCI_ADDR.
if [ -z "$PCI_ADDR" ]; then
    echo "PCI address not found, skipping."
else
    echo "Writing PCI_ADDR: $PCI_ADDR"
    sed -i "s/WW:WW.W/$PCI_ADDR/g" $CONFIG_PATH
fi
