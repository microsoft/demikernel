# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# Read IFACE_NAME from standard input.
$IFACE_NAME = Read-Host "Enter the name of the interface you want to use (e.g. 'Ethernet 1')"

# Read CONFIG_PATH from standard input.
$CONFIG_PATH = Read-Host "Enter the name of the configuration file you want to use (e.g. config.yaml)"

# If CONFIG_PATH is empty, set it to the default value.
if (-not $CONFIG_PATH) {
    $CONFIG_PATH = "config.yaml"
}

# Get IPV4_ADDR.
$IPV4_ADDR = (Get-NetIPAddress -InterfaceAlias $IFACE_NAME -AddressFamily IPv4).IPAddress

# Get MAC_ADDR.
$MAC_ADDR = (Get-NetAdapter -Name $IFACE_NAME).MacAddress

# Copy the azure.yaml file.
Copy-Item -Path .\scripts\config\azure.yaml -Destination $CONFIG_PATH -Confirm:$false

# Replace 'abcde' with IFACE_NAME in the configuration file.
(Get-Content -Path $CONFIG_PATH) | ForEach-Object {
    $_ -replace 'abcde', $IFACE_NAME
} | Set-Content -Path $CONFIG_PATH

# Set IPV4_ADDR.
if (-not [string]::IsNullOrEmpty($IPV4_ADDR)) {
    Write-Host "Writing IPV4_ADDR: $IPV4_ADDR"
    (Get-Content -Path $CONFIG_PATH) | ForEach-Object {
        $_ -replace 'XX.XX.XX.XX', $IPV4_ADDR
    } | Set-Content -Path $CONFIG_PATH
} else {
    Write-Host "IPv4 address not found, skipping."
}

# Set MAC_ADDR.
if (-not [string]::IsNullOrEmpty($MAC_ADDR)) {
    Write-Host "Writing MAC_ADDR: $MAC_ADDR"
    (Get-Content -Path $CONFIG_PATH) | ForEach-Object {
        $_ -replace 'ff:ff:ff:ff:ff:ff', $MAC_ADDR
    } | Set-Content -Path $CONFIG_PATH
} else {
    Write-Host "MAC address not found, skipping."
}
