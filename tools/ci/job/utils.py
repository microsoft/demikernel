# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import time
from typing import List
from azure.data.tables import TableServiceClient

# ======================================================================================================================
# Global Variables
# ======================================================================================================================

COMMIT_HASH: str = ""
CONNECTION_STRING: str = ""
TABLE_NAME = ""
LIBOS = ""

# ======================================================================================================================
# Utilities
# ======================================================================================================================


def set_commit_hash(commit_hash: str):
    global COMMIT_HASH
    if COMMIT_HASH != "":
        raise Exception("Commit hash is already set.")
    COMMIT_HASH = commit_hash


def set_connection_string(connection_string: str) -> None:
    global CONNECTION_STRING
    if CONNECTION_STRING != "":
        raise Exception("Connection string is already set.")
    CONNECTION_STRING = connection_string


def set_table_name(table_name: str) -> None:
    global TABLE_NAME
    if TABLE_NAME != "":
        raise Exception("Table name is already set.")
    TABLE_NAME = table_name


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


def get_connection_string() -> str:
    global CONNECTION_STRING
    return CONNECTION_STRING


def get_table_name() -> str:
    global TABLE_NAME
    return TABLE_NAME


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


def extract_performance(job_name, file):
    connection_string: str = get_connection_string()
    table_name: str = get_table_name()
    libos: str = get_libos()
    commit_hash: str = get_commit_hash()

    # Connect to Azure Tables.
    if not connection_string == "":
        table_service = TableServiceClient.from_connection_string(connection_string)
        table_client = table_service.get_table_client(table_name)

        # Filter profiler lines.
        lines = [line for line in file if line.startswith("+")]

        # Parse statistics and upload them to azure tables.
        for line in lines:
            line = line.replace("::", ";")
            columns = line.split(";")
            # Workaround for LibOses which are miss behaving.
            if len(columns) == 6:
                scope = columns[1]
                syscall = columns[2]
                total_time = columns[3]
                average_cycles = columns[4]
                average_time = columns[5]

                partition_key: str = f"{commit_hash}-{get_libos()}-{job_name}"
                row_key: str = syscall

                entry: dict[str, str, str, str, str, str, float, float, float] = {}
                entry["PartitionKey"] = partition_key
                entry["RowKey"] = row_key
                entry["CommitHash"] = commit_hash
                entry["LibOS"] = libos
                entry["JobName"] = job_name
                entry["Scope"] = scope
                entry["Syscall"] = syscall
                entry["TotalTime"] = float(total_time)
                entry["AverageCyclesPerSyscall"] = float(average_cycles)
                entry["AverageTimePerSyscall"] = float(average_time)

                table_client.delete_entity(partition_key, row_key)
                table_client.create_entity(entry)


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
                with open(log_directory + "/" + job_name + ".stdout.txt", "r") as file:
                    extract_performance(job_name, file)

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
