# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from os import mkdir
from shutil import move, rmtree
from os.path import isdir
from typing import List

from ci.src.base_test import BaseTest
from ci.src.ci_map import CIMap
from ci.src.test_instantiator import TestInstantiator
from common import *

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

        # STEP 4: Run system tests.
        if test_system:
            if status["checkout"] and status["compile"]:
                scaffolding: dict = create_scaffolding(libos, server, server_addr, client, client_addr, is_debug, is_sudo,
                                                       repository, delay, config_path, log_directory)
                ci_map: CIMap = get_ci_map()
                test_names: List = get_tests_to_run(
                    scaffolding, ci_map) if test_system == "all" else [test_system]
                for test_name in test_names:
                    t: BaseTest = create_test_instance_windows(
                        scaffolding, ci_map, test_name)
                    status[test_name] = t.execute()

        # Setp 5: Clean up.
        status["cleanup"] = job_cleanup_windows(
            repository, server, client, is_sudo, enable_nfs, log_directory)

        return status

    # STEP 1: Check out.
    status["checkout"] = job_checkout(
        repository, branch, server, client, enable_nfs, log_directory)

    # STEP 2: Compile debug.
    if status["checkout"]:
        status["compile"] = job_compile(
            repository, libos, is_debug, server, client, enable_nfs, log_directory)

    # STEP 3: Run unit tests.
    if test_unit:
        if status["checkout"] and status["compile"]:
            status["unit_tests"] = job_test_unit_rust(repository, libos, is_debug, server, client,
                                                      is_sudo, config_path, log_directory)
            if libos == "catnap" or libos == "catloop":
                status["integration_tests"] = job_test_integration_tcp_rust(
                    repository, libos, is_debug, server, client, server_addr, client_addr, is_sudo, config_path, log_directory)
            elif libos == "catmem":
                status["integration_tests"] = job_test_integration_pipe_rust(
                    repository, libos, is_debug, "standalone", server, client, server_addr, delay, is_sudo,
                    config_path, log_directory)
                status["integration_tests"] = job_test_integration_pipe_rust(
                    repository, libos, is_debug, "push-wait", server, client, server_addr, delay, is_sudo,
                    config_path, log_directory)
                status["integration_tests"] = job_test_integration_pipe_rust(
                    repository, libos, is_debug, "pop-wait", server, client, server_addr, delay, is_sudo,
                    config_path, log_directory)
                status["integration_tests"] = job_test_integration_pipe_rust(
                    repository, libos, is_debug, "push-wait-async", server, client, server_addr, delay, is_sudo,
                    config_path, log_directory)
                status["integration_tests"] = job_test_integration_pipe_rust(
                    repository, libos, is_debug, "pop-wait-async", server, client, server_addr, delay, is_sudo,
                    config_path, log_directory)

    # STEP 4: Run system tests.
    if test_system:
        if status["checkout"] and status["compile"]:
            scaffolding: dict = create_scaffolding(libos, server, server_addr, client, client_addr, is_debug, is_sudo,
                                                   repository, delay, config_path, log_directory)
            ci_map: CIMap = get_ci_map()
            test_names: List = get_tests_to_run(
                scaffolding, ci_map) if test_system == "all" else [test_system]
            for test_name in test_names:
                # Skip this tests for now
                if is_debug and (test_name == "tcp_ping_pong" or test_name == "tcp_push_pop") and (libos == "catnip" or libos == "catpowder"):
                    continue
                else:
                    t: BaseTest = create_test_instance(
                        scaffolding, ci_map, test_name)
                    status[test_name] = t.execute()

    # Setp 5: Clean up.
    status["cleanup"] = job_cleanup(
        repository, server, client, is_sudo, enable_nfs, log_directory)

    return status


def create_scaffolding(libos: str, server_name: str, server_addr: str, client_name: str, client_addr: str,
                       is_debug: bool, is_sudo: bool, repository: str, delay: float, config_path: str,
                       log_directory: str) -> dict:
    return {
        "libos": libos,
        "server_name": server_name,
        "server_ip": server_addr,
        "client_name": client_name,
        "client_ip": client_addr,
        "is_debug": is_debug,
        "is_sudo": is_sudo,
        "repository": repository,
        "delay": delay,
        "config_path": config_path,
        "log_directory": log_directory
    }


def get_ci_map() -> CIMap:
    path = "tools/ci/config/ci_map.yaml"
    yaml_str = ""
    with open(path, "r") as f:
        yaml_str = f.read()
    return CIMap(yaml_str)


def get_tests_to_run(scaffolding: dict, ci_map: CIMap) -> List:
    td: dict = ci_map.get_test_details(scaffolding["libos"], test_name="all")
    return td.keys()


def create_test_instance(scaffolding: dict, ci_map: CIMap, test_name: str) -> BaseTest:
    td: dict = ci_map.get_test_details(scaffolding["libos"], test_name)
    ti: TestInstantiator = TestInstantiator(test_name, scaffolding, td)
    t: BaseTest = ti.get_test_instance(job_test_system_rust)
    return t


def create_test_instance_windows(scaffolding: dict, ci_map: CIMap, test_name: str) -> BaseTest:
    td: dict = ci_map.get_test_details(scaffolding["libos"], test_name)
    ti: TestInstantiator = TestInstantiator(test_name, scaffolding, td)
    t: BaseTest = ti.get_test_instance(job_test_system_rust_windows)
    return t


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
    global CONNECTION_STRING
    CONNECTION_STRING = args.connection_string if args.connection_string != "" else CONNECTION_STRING
    global TABLE_NAME
    TABLE_NAME = args.table_name if args.table_name != "" else TABLE_NAME
    global LIBOS
    LIBOS = libos

    status: dict = run_pipeline(repository, branch, libos, is_debug, server,
                                client, test_unit, test_system, server_addr,
                                client_addr, delay, config_path, output_dir, enable_nfs)
    if False in status.values():
        sys.exit(-1)
    else:
        sys.exit(0)


if __name__ == "__main__":
    main()
