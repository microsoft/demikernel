if ($args.Count -ne 1) {
    Write-Host "Usage: generate-config.ps1 {path to config.yaml.template}"
    exit 1
}

if (Test-Path -Path $args[0]) {
    $configFileContent = Get-Content -Path $args[0]
} else {
    Write-Output "File does not exist: $args[0]"
    exit 1
}

$srvTemplateFile="$PSScriptRoot\run-server.ps1.template"
if (Test-Path -Path $srvTemplateFile) {
    $srvTemplateContent = Get-Content -Path $srvTemplateFile
} else {
    Write-Output "File does not exist: $srvTemplateFile"
    exit 1
}

function Get-NetworkAdapterName {
    # Get all NetAdapters
    Get-NetAdapter | Select-Object InterfaceIndex, Name, InterfaceDescription, MacAddress, @{
         Name="IP Addr";
         Expression={(Get-NetIPAddress -InterfaceIndex $_.InterfaceIndex | Where-Object AddressFamily -eq 'IPv4').IPAddress -join ", "}
    } | Sort-Object InterfaceIndex | Format-Table -Wrap -AutoSize | Out-Host

    # Hint for input
    $selection = Read-Host "Please input the InterfaceIndex number of ethernet adapter"

    # Get the selected netadapter
    $selectedAdapter = Get-NetAdapter | Where-Object { $_.InterfaceIndex -eq [int]$selection }

    if ($selectedAdapter) {
        # Return the adapter's name
        return $selectedAdapter.Name
    } else {
        return $null
    }
}

$adapterName = Get-NetworkAdapterName

while ($adapterName -eq $null) {
    Write-Host "Invalid option, please retry."
    $adapterName = Get-NetworkAdapterName
}

# Get ipv4 of the netadapter and fill the ipv4 and index
$ipv4=(Get-NetIPAddress -AddressFamily IPv4 -InterfaceAlias $adapterName | Select-Object -ExpandProperty IpAddress)

$adapter = Get-NetIPAddress | Where-Object { $_.IPAddress -eq $ipv4 }
if ($adapter) {
  $configFileContent = $configFileContent -replace "{{IPADDR}}", $ipv4
  $configFileContent = $configFileContent -replace "{{XDPIFIDX}}", $adapter.InterfaceIndex
  $srvTemplateContent = $srvTemplateContent -replace "{{IPADDR}}", $ipv4
  $netAdapter = Get-NetAdapter | Where-Object { $_.InterfaceIndex -eq $adapter.InterfaceIndex }
  $mac = $($netAdapter.MacAddress)
} else {
    Write-Host "Warning: Did not found the card"
    exit 1
}

# Replace the macaddress placeholder
$mac = $mac -replace "-", ":"
$configFileContent = $configFileContent -replace "{{MACADDR}}", $mac
$target="$PSScriptRoot\config.yaml"
Set-Content -Path $target -Value $configFileContent
Set-Content -Path "$PSScriptRoot\run-server.ps1" -Value $srvTemplateContent

# Update the CLIENT_HOST in env.ps1
$envPath = "$PSScriptRoot\env.ps1.template"
if (Test-Path -Path $envPath) {
    $envContent = Get-Content -Path $envPath
    $envContent = $envContent -replace "{{CLIENT_HOST}}", $ipv4
    Set-Content -Path "$PSScriptRoot\env.ps1" -Value $envContent
}

Write-Host "This machine use the following ip: $ipv4."

Write-Host "Done generating config.yaml and run-server.ps1 files"