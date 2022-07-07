# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_cleanup(host: string, workspace: string, project: string, default_branch: string = "master"):
    cmd = "cd {}/{} && make clean && git checkout {} && git clean -fdx".format(workspace, project, default_branch)
    ssh_cmd = "ssh {} \"bash -l -c \'{}\'\"".format(host, cmd)
    p = subprocess.Popen(ssh_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return p


def main():

    # Initialize parser
    parser = argparse.ArgumentParser()
    parser.add_argument("-c", "--client", help="Set client host", required=True)
    parser.add_argument("-s", "--server", help="set server host", required=True)
    parser.add_argument("-w", "--workspace", help="Set workspace", required=True)
    parser.add_argument("-p", "--project", help="Set project", required=True)

    # Read arguments from command line
    args = parser.parse_args()

    host0: string = args.server
    host1: string = args.client
    workspace: string = args.workspace
    project: string = args.project
    jobs: list[subprocess.Popen[str]] = []

    print("  Cleanup...")

    jobs.append(job_cleanup(host0, workspace, project))
    jobs.append(job_cleanup(host1, workspace, project))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
        job_name: string = str(j.pid) + "-cleanup"
        with open(job_name + ".stdout", "w") as file:
            file.write("{}".format(stdout))
        with open(job_name + ".stderr", "w") as file:
            file.write("{}".format(stderr))

    # Cleanup list of jobs.
    while len(jobs) > 0:
        jobs.pop(0)


if __name__ == "__main__":
    main()
