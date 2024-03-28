# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import argparse
from os import mkdir
from shutil import move, rmtree
from os.path import isdir
import yaml
from ci.job.linux import CheckoutJobOnLinux, CleanupJobOnLinux, CompileJobOnLinux, TcpEchoTest
from ci.job.utils import set_commit_hash, set_connection_string, set_libos, set_table_name
import ci.git as git

# =====================================================================================================================


# Runs the CI pipeline.
def run_pipeline(
        repository: str, branch: str, libos: str, is_debug: bool, server: str, client: str,
        server_addr: str, client_addr: str, delay: float, config_path: str,
        output_dir: str, enable_nfs: bool) -> int:
    is_sudo: bool = True if libos == "catnip" or libos == "catpowder" or libos == "catloop" else False
    status: dict[str, bool] = {}

    # Create folder for test logs
    log_directory: str = "{}/{}".format(output_dir, "{}-{}-{}".format(libos, branch,
                                                                      "debug" if is_debug else "release").replace("/", "_"))

    if isdir(log_directory):
        # Keep the last run
        old_dir: str = log_directory + ".old"
        if isdir(old_dir):
            rmtree(old_dir)
        move(log_directory, old_dir)
    mkdir(log_directory)

    config: dict = {
        "server": server,
        "server_name": server,
        "client": client,
        "client_name": client,
        "repository": repository,
        "branch": branch,
        "libos": libos,
        "is_debug": is_debug,
        "server_addr": server_addr,
        "server_ip": server_addr,
        "client_addr": client_addr,
        "client_ip": client_addr,
        "delay": delay,
        "config_path": config_path,
        "output_dir": output_dir,
        "enable_nfs": enable_nfs,
        "log_directory": log_directory,
        "is_sudo": is_sudo,
    }

    # STEP 1: Check out.
    status["checkout"] = CheckoutJobOnLinux(config).execute()

    # STEP 2: Compile debug.
    if status["checkout"]:
        status["compile"] = CompileJobOnLinux(config).execute()

    # STEP 4: Run system tests.
    if status["checkout"] and status["compile"]:
        ci_map = read_yaml()
        if 'tcp_echo' in ci_map[libos]:
            for scenario in ci_map[libos]['tcp_echo']:
                status["tcp_echo"] = TcpEchoTest(
                    config, scenario['run_mode'], scenario['nclients'], scenario['bufsize'], scenario['nrequests']).execute()

    # Setp 5: Clean up.
    status["cleanup"] = CleanupJobOnLinux(config).execute()

    return status


def read_yaml():
    path = "tools/ci/config/benchmark.yaml"
    yaml_str = ""
    with open(path) as f:
        yaml_str = f.read()
    return yaml.safe_load(yaml_str)


# Reads and parses command line arguments.
def read_args() -> argparse.Namespace:
    description: str = ""
    description += "Use this utility to run the performance regression system of Demikernel on a pair of remote host machines.\n"
    description += "Before using this utility, ensure that you have correctly setup the development environment on the remote machines.\n"
    description += "For more information, check out the README.md file of the project."

    # Initialize parser.
    parser = argparse.ArgumentParser(
        prog="benchmark.py", description=description)

    # Host options.
    parser.add_argument("--server", required=True, help="set server host name")
    parser.add_argument("--client", required=True, help="set client host name")

    # Build options.
    parser.add_argument("--repository", required=True,
                        help="set location of target repository in remote hosts")
    parser.add_argument("--branch", required=True,
                        help="set target branch in remote hosts")
    parser.add_argument("--libos", required=True,
                        help="set target libos in remote hosts")
    parser.add_argument("--debug", required=False,
                        action='store_true', help="sets debug build mode")
    parser.add_argument("--delay", default=1.0, type=float, required=False,
                        help="set delay between server and host for system-level tests")
    parser.add_argument("--enable-nfs", required=False, default=False,
                        action="store_true", help="enable building on nfs directories")

    # Test options.
    parser.add_argument("--server-addr", required=True,
                        help="sets server address in tests")
    parser.add_argument("--client-addr", required=True,
                        help="sets client address in tests")
    parser.add_argument("--config-path", required=False,
                        default="\$HOME/config.yaml", help="sets config path")

    # Other options.
    parser.add_argument("--output-dir", required=False,
                        default=".", help="output directory for logs")
    parser.add_argument("--connection-string", required=False,
                        default="", help="connection string to access Azure tables")
    parser.add_argument("--table-name", required=False,
                        default="", help="Azure table to place results")

    # Read arguments from command line.
    return parser.parse_args()


# Drives the program.
def main():
    # Parse and read arguments from command line.
    args: argparse.Namespace = read_args()

    # Extract host options.
    server: str = args.server
    client: str = args.client

    # Extract build options.
    repository: str = args.repository
    branch: str = args.branch
    libos: str = args.libos
    is_debug: bool = args.debug
    delay: float = args.delay
    config_path: str = args.config_path
    enable_nfs: bool = args.enable_nfs

    # Extract test options.
    server_addr: str = args.server_addr
    client_addr: str = args.client_addr

    # Output directory.
    output_dir: str = args.output_dir

    # Initialize glboal variables.
    head_commit: str = git.get_head_commit(branch)
    set_commit_hash(head_commit)
    set_connection_string(args.connection_string)
    set_table_name(args.table_name)
    set_libos(libos)

    run_pipeline(repository, branch, libos, is_debug, server,
                 client, server_addr,
                 client_addr, delay, config_path, output_dir, enable_nfs)


if __name__ == "__main__":
    main()
