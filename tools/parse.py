import sys
import pandas as pd
import re
import glob
import os

# Check if directory is provided
if len(sys.argv) < 2:
    print("Please provide a directory as a command line argument.")
    sys.exit()

# Get all .stdout files in the directory
directory = sys.argv[1]
files = glob.glob(f"{directory}/tcp-echo-*client*.stdout.txt")


libos = []
branch = []
kernel = []
numClients = []
numRequests = []
dataSize = []
rps = []
p50 = []
p99 = []

# Parse each file
for filename in files:
    with open(filename, 'r') as file:
        lines = file.readlines()

    # Initialize lists to store data
    statsLibOS = ""
    statsBranch = ""
    statsKernel = ""
    statsNumClients = ""
    statsNumRequests = ""
    statsDataSize = ""
    statsRPS = []
    statsP50 = []
    statsP99 = []

    # Get libos from directory name.
    d = os.path.basename(os.path.dirname(filename))
    statsLibOS = d.split('-')[0]
    statsBranch = d.split('-')[1]

    # Strip the directory from the filename
    f = os.path.basename(filename)
    # Convert tcp-echo to tcp_echo
    f = f.replace("tcp-echo", "tcp_echo")

    # Extract kernel, nclients, nrequests, and datasize from file name.
    statsKernel = f.split('-')[0]
    statsNumClients = f.split('-')[2]
    statsDataSize = f.split('-')[3]
    statsNumRequests = f.split('-')[4]

    print(f"Processing {filename}...")
    for line in lines:
        if "INFO:" in line:
            info = re.search(
                r'INFO: (\d+.\d) requests, (\d+.\d) rps, p50 (\d+) ns, p99 (\d+) ns', line)
            if info:
                statsRPS.append(float(info.group(2)))
                statsP50.append(int(info.group(3)))
                statsP99.append(int(info.group(4)))

    # Create a DataFrame
    df = pd.DataFrame({
        'rps': statsRPS,
        'p50': statsP50,
        'p99': statsP99
    })

    # Compute and print statistics
    stats = df.describe(include='all')

    libos.append(statsLibOS)
    branch.append(statsBranch)
    kernel.append(statsKernel)
    numClients.append(int(statsNumClients))
    numRequests.append(int(statsNumRequests))
    dataSize.append(int(statsDataSize))
    rps.append(float(stats['rps']['mean']))
    p50.append(int(stats['p50']['mean']))
    p99.append(int(stats['p99']['mean']))

df = pd.DataFrame({
    'kernel': kernel,
    'branch': branch,
    'libos': libos,
    'nclients': numClients,
    'nrequests': numRequests,
    'datasize': dataSize,
    'rps': rps,
    'p50': p50,
    'p99': p99
})

# Sort data frame by kernel, libos, datasize and nclients.
df = df.sort_values(by=['kernel', 'libos', 'datasize', 'nclients'])

print(df)
