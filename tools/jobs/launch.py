# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_launch(host: string, repository: string, target: string, is_sudo: bool):
    sudo_cmd = ""
    if is_sudo:
        sudo_cmd = "sudo -E"
    cmd = "cd {} && {} make {}".format(repository, sudo_cmd, target)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    p = subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p


def main():

    # Initialize parser
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", help="set remote host", required=True)
    parser.add_argument("--repository", help="set remote repository", required=True)
    parser.add_argument("--libos", help="set libos", required=True)
    parser.add_argument("--test", help="set test", required=True)
    parser.add_argument("--args", help="set args", required=True)
    parser.add_argument("--verbose", help="set verbose mode", action="store_true", default=False)

    # Read arguments from command line
    args = parser.parse_args()

    host: string = args.host
    repository: string = args.repository
    libos: string = args.libos
    test_name: string = args.test
    prog_args: string = args.args
    is_sudo: bool = False
    verbose: bool = args.verbose
    jobs: list[subprocess.Popen[str]] = []
    status: int = 0

    # Check for sudo requirements.
    if libos == "catnip" or libos == "catpowder":
        is_sudo = True

    print("Launching... {}".format(prog_args))

    cmd = "test-system-rust LIBOS={} TEST={} ARGS=\\\"{}\\\"".format(libos, test_name, prog_args)
    jobs.append(job_launch(host, repository, cmd, is_sudo))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        status = status or j.returncode
        if verbose:
            print("{}".format(stderr))
            print("{}".format(stdout))
        else:
            print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
            job_name: string = str(j.pid) + "-launch"
            with open(job_name + ".stdout", "w") as file:
                file.write("{}".format(stdout))
            with open(job_name + ".stderr", "w") as file:
                file.write("{}".format(stderr))

    # Cleanup list of jobs.
    while len(jobs) > 0:
        jobs.pop(0)

    sys.exit(status)


if __name__ == "__main__":
    main()
