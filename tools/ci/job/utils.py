# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import time
from typing import List
import sqlite3

# ======================================================================================================================
# Global Variables
# ======================================================================================================================

COMMIT_HASH: str = ""
DB_FILE_PATH: str = ""
TABLE_NAME: str = ""
LIBOS = ""

# ======================================================================================================================
# Utilities
# ======================================================================================================================


def set_commit_hash(commit_hash: str):
    global COMMIT_HASH
    if COMMIT_HASH != "":
        raise Exception("Commit hash is already set.")
    COMMIT_HASH = commit_hash


def set_db_file_path(path: str) -> None:
    global DB_FILE_PATH
    if DB_FILE_PATH != "":
        raise Exception("db_file is already set.")
    DB_FILE_PATH = path

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


def get_db_file_path() -> str:
    global DB_FILE_PATH
    return DB_FILE_PATH


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
    commit_hash: str = get_commit_hash()

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
            entry["LibOS"] = get_libos()
            entry["JobName"] = job_name
            entry["Scope"] = scope
            entry["Syscall"] = syscall
            entry["TotalTime"] = float(total_time)
            entry["AverageCyclesPerSyscall"] = float(average_cycles)
            entry["AverageTimePerSyscall"] = float(average_time)

            write_to_db(entry)


def write_to_db(entry):
    db_file_path = get_db_file_path()
    table_name = get_table_name()
    if db_file_path == "" or table_name == "":
        return
    print(f"Writing to {db_file_path} table {table_name} entry {entry}")
    conn = sqlite3.connect(database=db_file_path)
    c = conn.cursor()
    c.execute(f'''CREATE TABLE IF NOT EXISTS {table_name}
                 (PartitionKey text, RowKey text, CommitHash text, LibOS text, 
                  JobName text, Scope text, Syscall text, TotalTime real, 
                  AverageCyclesPerSyscall real, AverageTimePerSyscall real)''')
    c.execute(f"INSERT OR REPLACE INTO {table_name} VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)", 
              (entry["PartitionKey"], entry["RowKey"], entry["CommitHash"], entry["LibOS"], 
               entry["JobName"], entry["Scope"], entry["Syscall"], entry["TotalTime"], 
               entry["AverageCyclesPerSyscall"], entry["AverageTimePerSyscall"]))
    conn.commit()
    conn.close()


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
