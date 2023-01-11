# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
import string
import subprocess
import time
from typing import List

# ======================================================================================================================
# Utilities
# ======================================================================================================================


def timing(f):
    def wrap(*args, **kwargs):
        time1 = time.time()
        ret = f(*args, **kwargs)
        time2 = time.time()
        duration: float = (time2-time1)*1000.0
        return (ret, duration)
    return wrap


def wait_jobs(name: string, jobs: List):
    @timing
    def wait_jobs2(name: string, jobs: List) -> List:
        status: list[int] = []

        for j in jobs:
            stdout, stderr = j.communicate()
            status.append((j.pid, j.returncode))
            job_name: string = "{}-{}".format(name, str(j.pid))
            with open(job_name + ".stdout", "w") as file:
                file.write("{}".format(stdout))
            with open(job_name + ".stderr", "w") as file:
                file.write("{}".format(stderr))

        # Cleanup list of jobs.
        while len(jobs) > 0:
            jobs.pop(0)

        return status
    return wait_jobs2(name, jobs)


def wait_and_report(name: string, jobs: List, all_pass=True):
    ret = wait_jobs(name, jobs)
    passed: bool = False
    status: List = ret[0]
    duration: float = ret[1]
    if len(jobs) > 1:
        if all_pass:
            passed: bool = True if status[0][1] == 0 and status[1][1] == 0 else False
        else:
            passed: bool = True if status[0][1] == 0 or status[1][1] == 0 else False
    else:
        passed: bool = True if status[0][1] == 0 else False
    print("[{}] in {:9.2f} ms {}".format("PASSED" if passed else "FAILED", duration, name))

    return passed

# ======================================================================================================================
# Remote Commands
# ======================================================================================================================


# Executes a checkout command in a remote host.
def remote_checkout(host: string, repository: string, branch: string):
    cmd = "cd {} && git pull origin && git checkout {}".format(repository, branch)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    return subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


# Executes a compile command in a remote host.
def remote_compile(host: string, repository: string, target: string, is_debug: bool):
    debug_flag: string = "DEBUG=yes" if is_debug else "DEBUG=no"
    cmd = "cd {} && make {} {}".format(repository, debug_flag, target)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    return subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


# Executes a test in a remote host.
def remote_run(host: string, repository: string, is_debug: bool, target: string, is_sudo: bool, config_path: string):
    debug_flag: string = "DEBUG=yes" if is_debug else "DEBUG=no"
    sudo_cmd: string = "sudo -E" if is_sudo else ""
    cmd = "cd {} && {} make CONFIG_PATH={} {} {}".format(repository, sudo_cmd, config_path, debug_flag, target)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    return subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


# Executes a cleanup command in a remote host.
def remote_cleanup(host: string, workspace: string, default_branch: string = "dev"):
    cmd = "cd {} && make clean && git checkout {} && git clean -fdx".format(workspace, default_branch)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    return subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


# ======================================================================================================================
# Generic Jobs
# ======================================================================================================================


def job_checkout(repository: string, branch: string, server: string, client: string, enable_nfs: bool) -> bool:
    jobs: list[subprocess.Popen[str]] = []
    jobs.append(remote_checkout(server, repository, branch))
    if not enable_nfs:
        jobs.append(remote_checkout(client, repository, branch))
    return wait_and_report("checkout", jobs)


def job_compile(repository: string, libos: string, is_debug: bool, server: string, client: string, enable_nfs: bool) -> bool:
    jobs: list[subprocess.Popen[str]] = []
    jobs.append(remote_compile(server, repository, "all LIBOS={}".format(libos), is_debug))
    if not enable_nfs:
        jobs.append(remote_compile(client, repository, "all LIBOS={}".format(libos), is_debug))
    return wait_and_report("compile-{}".format("debug" if is_debug else "release"), jobs)


def job_test_system_rust(
        test_name: string, repo: string, libos: string, is_debug: bool, server: string, client: string,
        server_args: string, client_args: string, is_sudo: bool, all_pass: bool, delay: float, config_path: string) -> bool:
    server_cmd: string = "test-system-rust LIBOS={} TEST={} ARGS=\\\"{}\\\"".format(libos, test_name, server_args)
    client_cmd: string = "test-system-rust LIBOS={} TEST={} ARGS=\\\"{}\\\"".format(libos, test_name, client_args)
    jobs: list[subprocess.Popen[str]] = []
    jobs.append(remote_run(server, repo, is_debug, server_cmd, is_sudo, config_path))
    time.sleep(delay)
    jobs.append(remote_run(client, repo, is_debug, client_cmd, is_sudo, config_path))
    return wait_and_report(test_name, jobs, all_pass)


def job_test_unit_rust(repo: string, libos: string, is_debug: bool, server: string, client: string,
                       is_sudo: bool, config_path: string) -> bool:
    server_cmd: string = "test-unit-rust LIBOS={}".format(libos)
    client_cmd: string = "test-unit-rust LIBOS={}".format(libos)
    jobs: list[subprocess.Popen[str]] = []
    jobs.append(remote_run(server, repo, is_debug, server_cmd, is_sudo, config_path))
    jobs.append(remote_run(client, repo, is_debug, client_cmd, is_sudo, config_path))
    return wait_and_report("unit-tests", jobs, True)


def job_cleanup(repository: string, server: string, client: string, enable_nfs: bool) -> bool:
    jobs: list[subprocess.Popen[str]] = []
    jobs.append(remote_cleanup(server, repository))
    if not enable_nfs:
        jobs.append(remote_cleanup(client, repository))
    return wait_and_report("cleanup", jobs)


# ======================================================================================================================
# System Test Jobs
# ======================================================================================================================

def test_udp_ping_pong(
        server: string, client: string, libos: string, is_debug: bool, is_sudo: bool, repository: string,
        server_addr: string, client_addr: string, delay: float, config_path: string) -> bool:
    test_name: string = "udp-ping-pong"
    server_args: string = "--server {}:12345 {}:23456".format(server_addr, client_addr)
    client_args: string = "--client {}:23456 {}:12345".format(client_addr, server_addr)
    return job_test_system_rust(
        test_name, repository, libos, is_debug, server, client, server_args, client_args, is_sudo, False, delay,
        config_path)


def test_udp_push_pop(
        server: string, client: string, libos: string, is_debug: bool, is_sudo: bool, repository: string,
        server_addr: string, client_addr: string, delay: float, config_path: string) -> bool:
    test_name: string = "udp-push-pop"
    server_args: string = "--server {}:12345 {}:23456".format(server_addr, client_addr)
    client_args: string = "--client {}:23456 {}:12345".format(client_addr, server_addr)
    return job_test_system_rust(
        test_name, repository, libos, is_debug, server, client, server_args, client_args, is_sudo, True, delay,
        config_path)


def test_tcp_ping_pong(
        server: string, client: string, libos: string, is_debug: bool, is_sudo: bool, repository: string,
        server_addr: string, delay: float, config_path: string) -> bool:
    test_name: string = "tcp-ping-pong"
    server_args: string = "--server {}:12345".format(server_addr)
    client_args: string = "--client {}:12345".format(server_addr)
    return job_test_system_rust(
        test_name, repository, libos, is_debug, server, client, server_args, client_args, is_sudo, True, delay,
        config_path)


def test_tcp_push_pop(
        server: string, client: string, libos: string, is_debug: bool, is_sudo: bool, repository: string,
        server_addr: string, delay: float, config_path: string) -> bool:
    test_name: string = "tcp-push-pop"
    server_args: string = "--server {}:12345".format(server_addr)
    client_args: string = "--client {}:12345".format(server_addr)
    return job_test_system_rust(
        test_name, repository, libos, is_debug, server, client, server_args, client_args, is_sudo, True, delay,
        config_path)


def test_pipe_ping_pong(
        server: string, client: string, is_debug: bool, repository: string, delay: float, config_path: string) -> bool:
    test_name: string = "pipe-ping-pong"
    pipe_name: string = "demikernel-test-pipe-ping-pong"
    server_args: string = "--server demikernel-test-pipe-ping-pong".format(pipe_name)
    client_args: string = "--client demikernel-test-pipe-ping-pong".format(pipe_name)
    return job_test_system_rust(
        test_name, repository, "catmem", is_debug, server, client, server_args, client_args, False, True, delay,
        config_path)


def test_pipe_push_pop(server: string, client: string, is_debug: bool, repository: string, delay: float,
                       config_path: string) -> bool:
    test_name: string = "pipe-push-pop"
    pipe_name: string = "demikernel-test-pipe-push-pop"
    server_args: string = "--server demikernel-test-pipe-push-pop".format(pipe_name)
    client_args: string = "--client demikernel-test-pipe-push-pop".format(pipe_name)
    return job_test_system_rust(
        test_name, repository, "catmem", is_debug, server, client, server_args, client_args, False, True, delay,
        config_path)

# =====================================================================================================================


# Runs the CI pipeline.
def run_pipeline(
        repository: string, branch: string, libos: string, is_debug: bool, server: string, client: string,
        test_unit: bool, test_system: bool, server_addr: string, client_addr: string, delay: float, config_path: string,
        enable_nfs: bool) -> int:
    is_sudo: bool = True if libos == "catnip" or libos == "catpowder" else False
    passed: bool = False
    step: int = 0
    status: int = 0

    # STEP 1: Check out.
    passed = job_checkout(repository, branch, server, client, enable_nfs)
    status |= (0 if passed else 1) << step
    step += 1

    # STEP 2: Compile debug.
    if passed:
        passed = job_compile(repository, libos, is_debug, server, client, enable_nfs)
        status |= (0 if passed else 1) << step
        step += 1

    # STEP 3: Run unit tests.
    if test_unit | test_system:
        if passed:
            passed = job_test_unit_rust(repository, libos, is_debug, server, client, is_sudo, config_path)
            status |= (0 if passed else 1) << step
            step += 1

    # STEP 4: Run system tests.
    if test_system:
        if passed:
            if libos != "catmem":
                passed = test_udp_ping_pong(server, client, libos, is_debug, is_sudo,
                                            repository, server_addr, client_addr, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1
                passed = test_udp_push_pop(server, client, libos, is_debug, is_sudo,
                                           repository, server_addr, client_addr, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1
                passed = test_tcp_ping_pong(server, client, libos, is_debug, is_sudo,
                                            repository, server_addr, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1
                passed = test_tcp_push_pop(server, client, libos, is_debug, is_sudo,
                                           repository, server_addr, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1
            else:
                passed = test_pipe_ping_pong(server, server, is_debug, repository, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1
                passed = test_pipe_push_pop(client, client, is_debug, repository, delay, config_path)
                status |= (0 if passed else 1) << step
                step += 1

    # Setp 5: Clean up.
    passed = job_cleanup(repository, server, client, enable_nfs)
    status |= (0 if passed else 1) << step
    step += 1

    return status


# Reads and parses command line arguments.
def read_args() -> argparse.Namespace:
    description: string = ""
    description += "Use this utility to run the regression system of Demikernel on a pair of remote host machines.\n"
    description += "Before using this utility, ensure that you have correctly setup the development environment on the remote machines.\n"
    description += "For more information, check out the README.md file of the project."

    # Initialize parser.
    parser = argparse.ArgumentParser(prog="demikernel-ci.py", description=description)

    # Host options.
    parser.add_argument("--server", required=True, help="set server host name")
    parser.add_argument("--client", required=True, help="set client host name")

    # Build options.
    parser.add_argument("--repository", required=True, help="set location of target repository in remote hosts")
    parser.add_argument("--branch", required=True, help="set target branch in remote hosts")
    parser.add_argument("--libos", required=True, help="set target libos in remote hosts")
    parser.add_argument("--debug", required=False, action='store_true', help="sets debug build mode")
    parser.add_argument("--delay", default=1, required=False,
                        help="set delay between server and host for system-level tests")
    parser.add_argument("--enable-nfs", required=False, default=False,
                        action="store_true", help="enable building on nfs directories")

    # Test options.
    parser.add_argument("--test-unit", action='store_true', required=False, help="run unit tests only")
    parser.add_argument("--test-system", action='store_true', required=False, help="run unit and system tests")
    parser.add_argument("--server-addr", required="--test-system" in sys.argv, help="sets server address in tests")
    parser.add_argument("--client-addr", required="--test-system" in sys.argv, help="sets client address in tests")
    parser.add_argument("--config-path", required=False, default="\$HOME/config.yaml", help="sets config path")

    # Read arguments from command line.
    return parser.parse_args()


# Drives the program.
def main():
    # Parse and read arguments from command line.
    args: argparse.Namespace = read_args()

    # Extract host options.
    server: string = args.server
    client: string = args.client

    # Extract build options.
    repository: string = args.repository
    branch: string = args.branch
    libos: string = args.libos
    is_debug: bool = args.debug
    delay: float = args.delay
    config_path: string = args.config_path
    enable_nfs: bool = args.enable_nfs

    # Extract test options.
    test_unit: bool = args.test_unit
    test_system: bool = args.test_system
    server_addr: string = args.server_addr if test_system else ""
    client_addr: string = args.client_addr if test_system else ""

    status: int = run_pipeline(repository, branch, libos, is_debug, server,
                               client, test_unit, test_system, server_addr, client_addr, delay, config_path, enable_nfs)
    sys.exit(status)


if __name__ == "__main__":
    main()
