. $PSScriptRoot\env.ps1

$BUFFER_SIZE="1024"
$NCLIENTS="8"

if ($args.Count -ne 1) {
    Write-Host "Please input the server address."
    Write-Host "Usage: run-cli.ps1 {server IP address}"
    exit
}

$SERVER_HOST = $args[0]

Write-Host " *******        Client Mode         ********************* "
Write-Host " LIBOS:           $env:LIBOS"
Write-Host " CONFIG:          $env:CONFIG_PATH"
Write-Host " Host:            $SERVER_HOST"
Write-Host " Client:          $CLIENT_HOST"
Write-Host " Concurrent:      $NCLIENTS"
Write-Host " Buffersize:      $BUFFER_SIZE"
Write-Host "**********************************************************"
Write-Host " "

# 1. tcp-echo client
Write-Host "Starting TCP echo client..."
.\bin\examples\rust\tcp-echo.exe --peer client --address $SERVER_HOST":"$TCP_SERVER_PORT --nclients $NCLIENTS --bufsize $BUFFER_SIZE --run-mode concurrent

# 2. udp-pktgen client
# Write-Host "Starting UDP pktgen client..."
# .\bin\examples\rust\udp-pktgen.exe --local $CLIENT_HOST":"$UDP_CLIENT_PORT --remote $SERVER_HOST":"$UDP_SERVER_PORT --bufsize $BUFFER_SIZE --injection_rate 10000