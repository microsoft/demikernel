# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import datetime
import subprocess
import time
from typing import List
from azure.data.tables import TableServiceClient

# ======================================================================================================================
# Global Variables
# ======================================================================================================================

COMMIT_HASH: str = ""
CONNECTION_STRING: str = ""
TABLE_NAME = "test"
LIBOS = ""

# ======================================================================================================================
# Utilities
# ======================================================================================================================


def get_commit_hash() -> str:
    cmd = "git rev-parse HEAD"
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(
        git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    git_stdout = git_stdout.replace("\n", "")

    global COMMIT_HASH
    COMMIT_HASH = git_stdout
    assert len(COMMIT_HASH) == 40


def set_connection_string(connection_string: str) -> None:
    global CONNECTION_STRING
    CONNECTION_STRING = connection_string


def set_table_name(table_name: str) -> None:
    global TABLE_NAME
    TABLE_NAME = table_name


def set_libos(libos: str) -> None:
    global LIBOS
    LIBOS = libos


def timing(f):
    def wrap(*args, **kwargs):
        time1 = time.time()
        ret = f(*args, **kwargs)
        time2 = time.time()
        duration: float = (time2-time1)*1000.0
        return (ret, duration)
    return wrap


def extract_performance(job_name, file):
    global COMMIT_HASH
    global CONNECTION_STRING
    global TABLE_NAME
    global LIBOS

    # Connect to Azure Tables.
    if not CONNECTION_STRING == "":
        table_service = TableServiceClient.from_connection_string(
            CONNECTION_STRING)
        table_client = table_service.get_table_client(TABLE_NAME)

        # Filter profiler lines.
        lines = [line for line in file if line.startswith("+")]

        # Parse statistics and upload them to azure tables.
        for line in lines:
            line = line.replace("::", ";")
            columns = line.split(";")
            # Workaround for LibOses which are miss behaving.
            if len(columns) == 6:
                syscall = columns[2]
                total_time = columns[3]
                average_cycles = columns[4]
                average_time = columns[5]

                partition_key: str = "-".join([COMMIT_HASH, LIBOS, job_name])
                row_key: str = syscall

                entry: dict[str, str, str, str, str,
                            str, float, float, float] = {}
                entry["PartitionKey"] = partition_key
                entry["RowKey"] = row_key
                entry["CommitHash"] = COMMIT_HASH
                entry["LibOS"] = LIBOS
                entry["JobName"] = job_name
                entry["Syscall"] = syscall
                entry["TotalTime"] = float(total_time)
                entry["AverageCyclesPerSyscall"] = float(average_cycles)
                entry["AverageTimePerSyscall"] = float(average_time)
                entry["DateTime"] = datetime.datetime.now()

                table_client.delete_entity(partition_key, row_key)
                table_client.create_entity(entry)


def wait_jobs(log_directory: str, jobs: dict):
    @timing
    def wait_jobs2(log_directory: str, jobs: dict) -> List:
        status: list[int] = []

        for job_name, j in jobs.items():
            stdout, stderr = j.communicate()
            status.append((j.pid, j.returncode))
            with open(log_directory + "/" + job_name + ".stdout.txt", "w") as file:
                file.write("{}".format(stdout))
            with open(log_directory + "/" + job_name + ".stdout.txt", "r") as file:
                extract_performance(job_name, file)

            with open(log_directory + "/" + job_name + ".stderr.txt", "w") as file:
                file.write("{}".format(stderr))

        # Cleanup list of jobs.
        jobs.clear()

        return status
    return wait_jobs2(log_directory, jobs)


def wait_and_report(name: str, log_directory: str, jobs: dict, all_pass=True):
    ret = wait_jobs(log_directory, jobs)
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
