#!/bin/bash

if [ $# -ne 1 ]; then
    echo "Usage: generate-config.bash {path to config.yaml.template}"
    exit 1
fi

script_dir="$(dirname "$(readlink -f "$0")")"
echo "Using folder: $script_dir"
echo ""
# Check if config template exists and read it
if [ -f "$1" ]; then
    cp -f "$1" "$script_dir/config.yaml"
else
    echo "File does not exist: $1"
    exit 1
fi

# Check if server template exists
run_template="$script_dir/run-server.bash.template"
if [ -f "$run_template" ]; then
    cp -f "$run_template" "$script_dir/run-server.bash"
    chmod +x "$script_dir/run-server.bash"
else
    echo "File does not exist: $run_template"
    exit 1
fi

chmod +x "$script_dir/run-client.bash"
chmod +x "$script_dir/run-server.bash"

# Find all network adapters on this computer
interfaces=$(ip -o link show | awk -F': ' '{print $2}')

# List adapters
echo "Adapter list:"
for iface in $interfaces; do
    ip_addr=$(ip -4 addr show "$iface" | awk '/inet / {print $2}' | cut -d'/' -f1 | tr '\n' ' ')
    mac_addr=$(cat /sys/class/net/"$iface"/address)
    echo "Adapter $iface: IP: ${ip_addr:-} ; MAC: $mac_addr"
done

echo ""
echo "Please type your selected network interface name (e.g. eth0):"
read -r selected_iface

target_iface=""
for iface in $interfaces; do
    if [ "$iface" == "$selected_iface" ]; then
        target_iface="$selected_iface"
        echo "Selected: $target_iface"
        break
    fi
done

if [ -z "$target_iface" ]; then
    echo "Invalid option, please retry."
    exit 1;
fi

ipv4=$(ip -o -4 addr show "$target_iface" | awk '{print $4}' | awk -F '/' '{print $1}')
target_mac=$(cat /sys/class/net/"$target_iface"/address)

# Update the config.yaml with the selected interface information
sed -i "s|{{abcde}}|$target_iface|g" "$script_dir/config.yaml"
sed -i "s|{{IPADDR}}|$ipv4|g" "$script_dir/config.yaml"
sed -i "s|{{MACADDR}}|$target_mac|g" "$script_dir/config.yaml"
# Update the run-server.bash with the selected interface's IP address
sed -i "s|{{IPADDR}}|$ipv4|g" "$script_dir/run-server.bash"

# Update the CLIENT_HOST in env.bash if template exists
env_path="$script_dir/env.bash.template"
if [ -f "$env_path" ]; then
    cp -f "$script_dir/env.bash.template" "$script_dir/env.bash"
    sed -i "s|{{CLIENT_HOST}}|$ipv4|g" "$script_dir/env.bash"
else
    echo "File does not exist: $env_path"
    exit 1
fi

echo "This machine uses the following IP: $ipv4"
echo "Done generating config.yaml and run-server.bash files"
