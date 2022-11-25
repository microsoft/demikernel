# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_compile(host: string, repository: string, target: string, is_debug: bool):
    debug_flag: string = "DEBUG=no"
    if is_debug:
        debug_flag = "DEBUG=yes"
    cmd = "cd {} && make {} {}".format(repository, debug_flag, target)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    p = subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p


def main():

    # Initialize parser
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", help="set remote host", required=True)
    parser.add_argument("--repository", help="set remote repository", required=True)
    parser.add_argument("--libos", help="set libos", required=True)
    parser.add_argument("--debug", help="set debug build", action='store_true')
    parser.add_argument("--verbose", help="set verbose mode", action="store_true", default=False)

    # Read arguments from command line
    args = parser.parse_args()

    host: string = args.host
    repository: string = args.repository
    libos: string = args.libos
    is_debug: bool = args.debug
    verbose: bool = args.verbose
    jobs: list[subprocess.Popen[str]] = []
    status: int = 0

    if is_debug:
        print("Compiling debug build...")
    else:
        print("Compiling release build...")

    jobs.append(job_compile(host, repository, "all LIBOS={}".format(libos), is_debug))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        status = status or j.returncode
        if verbose:
            print("{}".format(stderr))
            print("{}".format(stdout))
        else:
            print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
            job_name: string = str(j.pid) + "-compile"
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
