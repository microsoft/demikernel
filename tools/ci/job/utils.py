# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import time
from typing import List

# ======================================================================================================================
# Global Variables
# ======================================================================================================================

COMMIT_HASH: str = ""
LIBOS = ""

# ======================================================================================================================
# Utilities
# ======================================================================================================================


def set_commit_hash(commit_hash: str):
    global COMMIT_HASH
    if COMMIT_HASH != "":
        raise Exception("Commit hash is already set.")
    COMMIT_HASH = commit_hash


def set_libos(libos: str) -> None:
    global LIBOS
    if LIBOS != "":
        raise Exception("LibOS is already set.")
    LIBOS = libos


def get_commit_hash() -> str:
    global COMMIT_HASH
    if COMMIT_HASH == "":
        raise Exception("Commit hash is not set.")
    return COMMIT_HASH


def get_libos() -> str:
    global LIBOS
    if LIBOS == "":
        raise Exception("LibOS is not set.")
    return LIBOS


def timing(f):
    def wrap(*args, **kwargs):
        time1 = time.time()
        ret = f(*args, **kwargs)
        time2 = time.time()
        duration: float = (time2-time1)*1000.0
        return (ret, duration)
    return wrap


def wait_jobs(log_directory: str, jobs: dict, no_wait: bool = False):
    @timing
    def wait_jobs2(log_directory: str, jobs: dict) -> List:
        status: list[int] = []

        if not no_wait:
            for job_name, j in jobs.items():
                stdout, stderr = j.communicate()
                status.append((j.pid, j.returncode))
                with open(log_directory + "/" + job_name + ".stdout.txt", "w") as file:
                    file.write("{}".format(stdout))
                with open(log_directory + "/" + job_name + ".stderr.txt", "w") as file:
                    file.write("{}".format(stderr))

        # Cleanup list of jobs.
        jobs.clear()

        return status
    return wait_jobs2(log_directory, jobs)


def wait_and_report(name: str, log_directory: str, jobs: dict, all_pass=True, no_wait: bool = False):
    if no_wait:
        print("[NO WAIT] in {:9.2f} ms {}".format(0, name))
        return True

    ret = wait_jobs(log_directory, jobs, no_wait)
    passed: bool = False
    status: List = ret[0]
    duration: float = ret[1]
    if len(status) > 1:
        if all_pass:
            passed: bool = True if status[0][1] == 0 and status[1][1] == 0 else False
        else:
            passed: bool = True if status[0][1] == 0 or status[1][1] == 0 else False
    else:
        passed: bool = True if status[0][1] == 0 else False
    print("[{}] in {:9.2f} ms {}".format(
        "PASSED" if passed else "FAILED", duration, name))

    return passed
