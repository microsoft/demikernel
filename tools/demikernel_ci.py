# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import copy
import sys
import argparse
from os import mkdir
from shutil import move, rmtree
from os.path import isdir
import yaml
from ci.job.utils import set_commit_hash, set_connection_string, set_libos, set_table_name
from ci.job.factory import JobFactory
import ci.git as git

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
        "libos": libos if libos != "catnapw" else "catnap",
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
        "platform": "linux" if libos != "catnapw" else "windows"
    }

    factory: JobFactory = JobFactory(config)

    # STEP 1: Check out.
    status["checkout"] = factory.checkout().execute()

    # STEP 2: Compile debug.
    if status["checkout"]:
        status["compile"] = factory.compile().execute()

    # STEP 3: Run unit tests.
    if test_unit:
        if status["checkout"] and status["compile"]:
            status["unit_tests"] = True
            status["unit_tests"] &= factory.unit_test(test_name="test-unit-rust").execute()
            status["unit_tests"] &= factory.unit_test(test_name="test-unit-c").execute()
            if libos == "catnap" or libos == "catloop":
                status["integration_tests"] = factory.integration_test().execute()
            elif libos == "catmem":
                status["integration_tests"] = factory.integration_test("standalone").execute()
                status["integration_tests"] = factory.integration_test("push-wait").execute()
                status["integration_tests"] = factory.integration_test("pop-wait").execute()
                status["integration_tests"] = factory.integration_test("push-wait-async").execute()
                status["integration_tests"] = factory.integration_test("pop-wait-async").execute()

    # STEP 4: Run system tests.
    if test_system and config["platform"] == "linux":
        if status["checkout"] and status["compile"]:
            ci_map = read_yaml(libos)
            # Run pipe-open test.
            if __should_run(ci_map[libos], "pipe_open", test_system):
                scenario = ci_map[libos]['pipe_open']
                status["pipe-open"] = factory.system_test(test_name="open-close",
                                                          niterations=scenario['niterations']).execute()
            # Run pipe-ping-pong test.
            if __should_run(ci_map[libos], "pipe_ping_pong", test_system):
                status["pipe-ping-pong"] = factory.system_test(test_name="ping-pong").execute()
            # Run pipe-push-pop test.
            if __should_run(ci_map[libos], "pipe_push_pop", test_system):
                status["pipe-push-pop"] = factory.system_test(test_name="push-pop").execute()
            if __should_run(ci_map[libos], "tcp_echo", test_system):
                status["tcp_echo"] = True
                test_config = ci_map[libos]['tcp_echo']
                names = [p for p in test_config]
                scenarios = build_combinations(test_config, names, {})
                for scenario in scenarios:
                    # Skipt if scenario requires more threads then clients.
                    if scenario['nthreads'] > scenario['nclients']:
                        continue

                    if libos == "catnap":
                        status["tcp_echo"] &= factory.system_test(
                            test_name="tcp_echo", run_mode=scenario['run_mode'], nclients=scenario['nclients'], bufsize=scenario['bufsize'], nrequests=scenario['nrequests'], nthreads=scenario['nthreads']).execute()
                    else:
                        status["tcp_echo"] &= factory.system_test(
                            test_name="tcp_echo", run_mode=scenario['run_mode'], nclients=scenario['nclients'],
                            bufsize=scenario['bufsize'], nrequests=scenario['nrequests'], nthreads=1).execute()
            if __should_run(ci_map[libos], "tcp_close", test_system):
                status["tcp_close"] = True
                test_config = ci_map[libos]['tcp_close']
                names = [p for p in test_config]
                scenarios = build_combinations(test_config, names, {})
                for scenario in scenarios:
                    status["tcp_close"] &= factory.system_test(
                        test_name="tcp_close", run_mode=scenario['run_mode'], who_closes=scenario['who_closes'], nclients=scenario['nclients']).execute()
            if __should_run(ci_map[libos], "tcp_wait", test_system):
                status["tcp_wait"] = True
                test_config = ci_map[libos]['tcp_wait']
                names = [p for p in test_config]
                scenarios = build_combinations(test_config, names, {})
                for scenario in scenarios:
                    status["tcp_wait"] &= factory.system_test(test_name="tcp_wait",
                                                              scenario=scenario['scenario'], nclients=scenario['nclients']).execute()
            if __should_run(ci_map[libos], "tcp_ping_pong", test_system):
                status["tcp_ping_pong"] = factory.system_test(test_name="tcp_ping_pong").execute()
            if __should_run(ci_map[libos], "tcp_push_pop", test_system):
                status["tcp_push_pop"] = factory.system_test(test_name="tcp_push_pop").execute()
            if __should_run(ci_map[libos], "udp_ping_pong", test_system):
                status["udp_ping_pong"] = factory.system_test(test_name="udp_ping_pong").execute()
            if __should_run(ci_map[libos], "udp_push_pop", test_system):
                status["udp_push_pop"] = factory.system_test(test_name="udp_push_pop").execute()

    # Setp 5: Clean up.
    status["cleanup"] = factory.cleanup().execute()

    return status


# Recursively builds all combinations
def build_combinations(scenario: dict, names: list, params: dict) -> list:
    if len(names) == 0:
        l = [copy.deepcopy(params)]
        return l
    else:
        name = names[0]
        values = [v for v in scenario[name]]
        scenarios = []
        for value in values:
            params[name] = value
            scenarios += build_combinations(scenario, names[1:], params)
            del params[name]
        return scenarios


def __should_run(ci_map, test_name: str, test_system: str) -> bool:
    """Checks if we should run a given system test."""
    return test_name in ci_map and (test_system == "all" or test_system == test_name)


def read_yaml(libos: str):
    path: str = f"tools/ci/config/test/{libos}.yaml"
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
    head_commit: str = git.get_head_commit(branch)
    set_commit_hash(head_commit)
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
