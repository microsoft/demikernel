# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from azure.data.tables import TableServiceClient
import demikernel_ci

def read_args() ->argparse.Namespace:
    description: str = ""
    description += "Use this utility automate regression testing of Demikernel on a pair of remote host machines and report the results to Azure Tables.\n"
    description += "Before using this utility, ensure that you have correctly setup the development environment on the remote machines and created the Azure table for storing the results.\n"
    description += "For more information, check out the README.md file of the project."

    # Initialize parser.
    parser = argparse.ArgumentParser(prog="azure_tables.py", description=description)

    # Host options.
    parser.add_argument("--server", required=True, help="set server host name")
    parser.add_argument("--client", required=True, help="set client host name")

    # Build options.
    parser.add_argument("--repository", required=True, help="set location of target repository in remote hosts")
    parser.add_argument("--branch", required=False, help="set target branch in remote hosts")
    parser.add_argument("--libos", required=False, help="set target libos in remote hosts")
    parser.add_argument("--debug", required=False, action='store_true', help="sets debug build mode")
    parser.add_argument("--release", required=False, action='store_true', help="sets release build mode")
    parser.add_argument("--delay", default=1.0, type=float, required=False,
                        help="set delay between server and host for system-level tests")
    parser.add_argument("--enable-nfs", required=False, default=False,
                        action="store_true", help="enable building on nfs directories")

    # Test options.
    parser.add_argument("--server-addr", required="--test-system" in sys.argv, help="sets server address in tests")
    parser.add_argument("--client-addr", required="--test-system" in sys.argv, help="sets client address in tests")
    parser.add_argument("--config-path", required=False, default="\$HOME/config.yaml", help="sets config path")
    parser.add_argument("--test-unit", action='store_true', required=False, help="run unit tests only")
    parser.add_argument("--test-system", action='store_true', required=False, help="run unit and system tests")
    parser.add_argument("--connection-string", required=True, help="connection string to access Azure tables")
    parser.add_argument("--table-name", required=True, help="Azure table to place results")

    return parser.parse_args()

def main():
    args: argparse.Namespace = read_args()

    # Extract host options.
    server: str = args.server
    client: str = args.client

    # Extract build options.
    repository: str = args.repository
    delay: float = args.delay
    config_path: str = args.config_path
    enable_nfs: bool = args.enable_nfs

    # Set up list of tests to run.
    debug_modes: List = []
    liboses: List = []
    branches: List = []

    # If args ask for debug or no make flags given, then run debug build.
    if args.debug or not args.release:
        debug_modes.append(True)
    # If args ask for release or no make flags given, then run release build.
    if args.release or not args.debug:
        debug_modes.append(False)

    if args.libos is None:
        liboses = ["catnap", "catcollar", "catmem", "catpowder", "catnip"]
    else:
        liboses = [args.libos]

    if args.branch is None:
        branches = ["dev"]
    else:
        branches = [args.branch
]
    # Extract test options.
    test_unit: bool = args.test_unit
    test_system: bool = args.test_system
    server_addr: str = args.server_addr if test_system else ""
    client_addr: str = args.client_addr if test_system else ""
    conn_string: str = args.connection_string
    table_name: str = args.table_name

    # Connect to Azure Tables.
    table_service = TableServiceClient.from_connection_string(conn_string)
    table_client = table_service.get_table_client(table_name)


    # Run tests.
    for is_debug in debug_modes:
        for libos in liboses:
            for branch in branches:
                row_key = libos
                partition_key = branch + "-{}".format("debug" if is_debug else "release")
                print("Running test for " + partition_key + " in branch and mode: "+row_key)
                status = demikernel_ci.run_pipeline(repository, branch, libos, is_debug, server,
                                                    client, test_unit, test_system, server_addr,
                                                    client_addr, delay, config_path, enable_nfs)
                status["PartitionKey"] = partition_key
                status["RowKey"] = row_key
                # Clear out the entity for a new clean result
                table_client.delete_entity(partition_key,row_key)
                table_client.create_entity(status)

    sys.exit(0)

if __name__ == "__main__":
    main()
