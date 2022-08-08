# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_checkout(host: string, workspace: string, project: string, branch: string):
    cmd = "cd {}/{} && git pull origin && git checkout {}".format(workspace, project, branch)
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
    parser.add_argument("-b", "--branch", help="Set branch", required=True)

    # Read arguments from command line
    args = parser.parse_args()

    host0: string = args.server
    host1: string = args.client
    workspace: string = args.workspace
    project: string = args.project
    branch: string = args.branch
    jobs: list[subprocess.Popen[str]] = []

    print("  Check out...")

    jobs.append(job_checkout(host0, workspace, project, branch))
    jobs.append(job_checkout(host1, workspace, project, branch))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
        job_name: string = str(j.pid) + "-checkout"
        with open(job_name + ".stdout", "w") as file:
            file.write("{}".format(stdout))
        with open(job_name + ".stderr", "w") as file:
            file.write("{}".format(stderr))

    # Cleanup list of jobs.
    while len(jobs) > 0:
        jobs.pop(0)


if __name__ == "__main__":
    main()
