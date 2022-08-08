# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import sys
import argparse
from cmath import exp
from getopt import getopt
import string
import subprocess
from time import sleep


def job_launch(host: string, workspace: string, project: string, target: string, is_sudo: bool):
    sudo_cmd = ""
    if is_sudo:
        sudo_cmd = "sudo -E"
    cmd = "cd {}/{} && {} make {}".format(workspace, project, sudo_cmd, target)
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
    parser.add_argument("-l", "--libos", help="Set libos", required=True)
    parser.add_argument("-t", "--test", help="Set test", required=True)

    # Read arguments from command line
    args = parser.parse_args()

    host0: string = args.server
    host1: string = args.client
    workspace: string = args.workspace
    project: string = args.project
    libos: string = args.libos
    test_name: string = args.test
    is_sudo: bool = False
    jobs: list[subprocess.Popen[str]] = []
    status: int = 0

    # Check for sudo requirements.
    if libos == "catnip" or libos == "catpowder":
        is_sudo = True

    print("  Launch...")

    cmd = "test-system-rust LIBOS={} TEST={} PEER=server".format(libos, test_name)
    jobs.append(job_launch(host0, workspace, project, cmd, is_sudo))
    sleep(5)
    cmd = "test-system-rust LIBOS={} TEST={} PEER=client".format(libos, test_name)
    jobs.append(job_launch(host1, workspace, project, cmd, is_sudo))

    # Wait jobs to complete and print output.
    for j in jobs:
        stdout, stderr = j.communicate()
        print("    job " + str(j.pid) + " exited with status " + str(j.returncode))
        status = status or j.returncode
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
