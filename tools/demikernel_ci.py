# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from os import mkdir
from shutil import move, rmtree
from os.path import isdir
import yaml

from ci.job.linux import CheckoutJobOnLinux, CompileJobOnLinux, UnitTestJobOnLinux, IntegrationTestJobOnLinux, CleanupJobOnLinux, PipeIntegrationTestJobOnLinux, TcpCloseTest, TcpEchoTest, TcpPingPongTest, TcpPushPopTest, TcpWaitTest, UdpPingPongTest, UdpPushPopTest, PipeOpenTest, PipePingPongTest, PipePushPopTest
from ci.job.windows import job_checkout_windows, job_compile_windows, job_test_unit_rust_windows, job_test_integration_tcp_rust_windows, job_cleanup_windows
from ci.job.utils import get_commit_hash, set_connection_string, set_libos, set_table_name

# =====================================================================================================================


# Runs the CI pipeline.
def run_pipeline(
        repository: str, branch: str, libos: str, is_debug: bool, server: str, client: str,
        test_unit: bool, test_system: str, server_addr: str, client_addr: str, delay: float, config_path: str,
        output_dir: str, enable_nfs: bool) -> int:
    is_sudo: bool = True if libos == "catnip" or libos == "catpowder" or libos == "catloop" else False
    step: int = 0
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
        "test_unit": test_unit,
        "test_system": test_system,
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

    if libos == "catnapw":
        libos = "catnap"
        status["checkout"] = job_checkout_windows(
            repository, branch, server, client, enable_nfs, log_directory)

        # STEP 2: Compile debug.
        if status["checkout"]:
            status["compile"] = job_compile_windows(
                repository, libos, is_debug, server, client, enable_nfs, log_directory)

        # STEP 3: Run unit tests.
        if test_unit:
            if status["checkout"] and status["compile"]:
                status["unit_tests"] = job_test_unit_rust_windows(repository, libos, is_debug, server, client,
                                                                  is_sudo, config_path, log_directory)
                # FIXME: https://github.com/microsoft/demikernel/issues/1030
                if False:
                    status["integration_tests"] = job_test_integration_tcp_rust_windows(
                        repository, libos, is_debug, server, client, server_addr, client_addr, is_sudo, config_path, log_directory)

        # Setp 5: Clean up.
        status["cleanup"] = job_cleanup_windows(
            repository, server, client, is_sudo, enable_nfs, log_directory)

        return status

    # STEP 1: Check out.
    status["checkout"] = CheckoutJobOnLinux(config).execute()

    # STEP 2: Compile debug.
    if status["checkout"]:
        status["compile"] = CompileJobOnLinux(config).execute()

    # STEP 3: Run unit tests.
    if test_unit:
        if status["checkout"] and status["compile"]:
            status["unit_tests"] = UnitTestJobOnLinux(config).execute()
            if libos == "catnap" or libos == "catloop":
                status["integration_tests"] = IntegrationTestJobOnLinux(
                    config).execute()
            elif libos == "catmem":
                status["integration_tests"] = PipeIntegrationTestJobOnLinux(
                    config, "standalone").execute()
                status["integration_tests"] = PipeIntegrationTestJobOnLinux(
                    config, "push-wait").execute()
                status["integration_tests"] = PipeIntegrationTestJobOnLinux(
                    config, "pop-wait").execute()
                status["integration_tests"] = PipeIntegrationTestJobOnLinux(
                    config, "push-wait-async").execute()
                status["integration_tests"] = PipeIntegrationTestJobOnLinux(
                    config, "pop-wait-async").execute()

    # STEP 4: Run system tests.
    if test_system:
        if status["checkout"] and status["compile"]:
            ci_map = read_yaml()
            if 'pipe-open' in ci_map[libos] and (test_system == "pipe-open" or test_system == "all"):
                scenario = ci_map[libos]['pipe-open']
                status["pipe-open"] = PipeOpenTest(config,
                                                   scenario['niterations']).execute()
            if 'pipe-ping-pong' in ci_map[libos] and (test_system == "pipe-ping-pong" or test_system == "all"):
                status["pipe-ping-pong"] = PipePingPongTest(config).execute()
            if 'pipe-push-pop' in ci_map[libos] and (test_system == "pipe-push-pop" or test_system == "all"):
                status["pipe-push-pop"] = PipePushPopTest(config).execute()
            if 'tcp_echo' in ci_map[libos] and (test_system == "tcp-echo" or test_system == "all"):
                for scenario in ci_map[libos]['tcp_echo']:
                    status["tcp_echo"] = TcpEchoTest(
                        config, scenario['run_mode'], scenario['nclients'], scenario['bufsize'], scenario['nrequests']).execute()
            if 'tcp_close' in ci_map[libos] and (test_system == "tcp-close" or test_system == "all"):
                for scenario in ci_map[libos]['tcp_close']:
                    status["tcp_close"] = TcpCloseTest(
                        config, scenario['run_mode'], scenario['who_closes'], scenario['nclients']).execute()
            if 'tcp_wait' in ci_map[libos] and (test_system == "tcp-wait" or test_system == "all"):
                for scenario in ci_map[libos]['tcp_wait']:
                    status["tcp_wait"] = TcpWaitTest(
                        config, scenario['scenario'], scenario['nclients']).execute()
            if 'tcp_ping_pong' in ci_map[libos] and (test_system == "tcp-ping-pong" or test_system == "all"):
                status["tcp_ping_pong"] = TcpPingPongTest(config).execute()
            if 'tcp_push_pop' in ci_map[libos] and (test_system == "tcp-push-pop" or test_system == "all"):
                status["tcp_push_pop"] = TcpPushPopTest(config).execute()
            if 'udp_ping_pong' in ci_map[libos] and (test_system == "udp-ping-pong" or test_system == "all"):
                status["udp_ping_pong"] = UdpPingPongTest(config).execute()
            if 'udp_push_pop' in ci_map[libos] and (test_system == "udp-push-pop" or test_system == "all"):
                status["udp_push_pop"] = UdpPushPopTest(config).execute()

    # Setp 5: Clean up.
    status["cleanup"] = CleanupJobOnLinux(config).execute()

    return status


def read_yaml():
    path = "tools/ci/config/ci_map.yaml"
    yaml_str = ""
    with open(path) as f:
        yaml_str = f.read()
    return yaml.safe_load(yaml_str)


# Reads and parses command line arguments.
def read_args() -> argparse.Namespace:
    description: str = ""
    description += "Use this utility to run the regression system of Demikernel on a pair of remote host machines.\n"
    description += "Before using this utility, ensure that you have correctly setup the development environment on the remote machines.\n"
    description += "For more information, check out the README.md file of the project."

    # Initialize parser.
    parser = argparse.ArgumentParser(
        prog="demikernel_ci.py", description=description)

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
    parser.add_argument("--test-unit", action='store_true',
                        required=False, help="run unit tests")
    parser.add_argument("--test-system", type=str,
                        required=False, help="run system tests")
    parser.add_argument("--server-addr", required="--test-system" in sys.argv,
                        help="sets server address in tests")
    parser.add_argument("--client-addr", required="--test-system" in sys.argv,
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
    test_unit: bool = args.test_unit
    test_system: str = args.test_system
    server_addr: str = args.server_addr
    client_addr: str = args.client_addr

    # Output directory.
    output_dir: str = args.output_dir

    # Initialize glboal variables.
    get_commit_hash()
    set_connection_string(args.connection_string)
    set_table_name(args.table_name)
    set_libos(libos)

    status: dict = run_pipeline(repository, branch, libos, is_debug, server,
                                client, test_unit, test_system, server_addr,
                                client_addr, delay, config_path, output_dir, enable_nfs)
    if False in status.values():
        sys.exit(-1)
    else:
        sys.exit(0)


if __name__ == "__main__":
    main()
