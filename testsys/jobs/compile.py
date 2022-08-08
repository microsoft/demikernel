# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_compile(host: string, workspace: string, project: string, target: string, is_debug: bool):
    debug_flag: string = "DEBUG=no"
    if is_debug:
        debug_flag = "DEBUG=yes"
    cmd = "cd {}/{} && make {} {}".format(workspace, project, debug_flag, target)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    p = subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p


def main():

    # Initialize parser
    parser = argparse.ArgumentParser()
    parser.add_argument("-d", "--debug", help="Set debug build", action='store_true')
    parser.add_argument("-c", "--client", help="Set client host", required=True)
    parser.add_argument("-s", "--server", help="set server host", required=True)
    parser.add_argument("-w", "--workspace", help="Set workspace", required=True)
    parser.add_argument("-p", "--project", help="Set project", required=True)
    parser.add_argument("-l", "--libos", help="Set libos", required=True)

    # Read arguments from command line
    args = parser.parse_args()

    host0: string = args.server
    host1: string = args.client
    workspace: string = args.workspace
    project: string = args.project
    libos: string = args.libos
    jobs: list[subprocess.Popen[str]] = []
    is_debug: bool = args.debug

    if is_debug:
        print("  Compile debug build...")
    else:
        print("  Compile release build...")

    jobs.append(job_compile(host0, workspace, project, "all LIBOS={}".format(libos), is_debug))
    jobs.append(job_compile(host1, workspace, project, "all LIBOS={}".format(libos), is_debug))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
        job_name: string = str(j.pid) + "-compile"
        with open(job_name + ".stdout", "w") as file:
            file.write("{}".format(stdout))
        with open(job_name + ".stderr", "w") as file:
            file.write("{}".format(stderr))

    # Cleanup list of jobs.
    while len(jobs) > 0:
        jobs.pop(0)


if __name__ == "__main__":
    main()
