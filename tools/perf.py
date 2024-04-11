# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import datetime
import fnmatch
from typing import List
from azure.data.tables import TableServiceClient
import pandas
import argparse
import ci.git as git

# =====================================================================================================================


# Drives the program.
def main():
    # Read arguments from command line and parse them.
    args: argparse.Namespace = __read_args()

    # Extract command line arguments.
    table_name: str = args.table
    connection_str: str = args.connection
    key: str = args.key
    branch_name: str = args.branch
    libos: str = args.libos
    ndays: int = 15

    __build_report(table_name=table_name, connection_str=connection_str,
                   key=key, branch_name=branch_name, libos=libos, ndays=ndays)


# =====================================================================================================================
# Private Functions
# =====================================================================================================================

# Reads and parses command line arguments.
def __read_args() -> argparse.Namespace:
    description: str = "CI Utility for reporting performance statistics of Demikernel."

    # Initialize parser.
    parser = argparse.ArgumentParser(prog="plot.py", description=description)

    # Options related to Storage account.
    parser.add_argument("--table", required=True, help="Set Azure Table to use.")

    # Options related to credentials.
    parser.add_argument("--connection", required=True, help="Set connection string to access Azure Storage Account.")
    parser.add_argument("--key", required=True, help="Set connection key to access Azure Storage Account.")

    # Options related to repository.
    parser.add_argument("--branch", required=True, help="Set the branch to report performance statistics for.")
    parser.add_argument("--libos", required=False, help="Set the library OS to report performance statistics for.")

    # Read arguments from command line.
    return parser.parse_args()


def __build_report(table_name: str, connection_str: str, key: str, branch_name: str, libos: str, ndays: int) -> None:

    # Sanity check inputs.
    if not table_name:
        raise ValueError("Invalid table name.")
    if not connection_str:
        raise ValueError("Invalid connection string.")
    if not key:
        raise ValueError("Invalid key.")
    if not branch_name:
        raise ValueError("Invalid branch name.")
    if ndays <= 0:
        raise ValueError("Invalid number of days.")

    # Connect to Azure table.
    table_service = TableServiceClient.from_connection_string(connection_str)
    table_client = table_service.get_table_client(table_name)

    # Query Azure table for performance statistics.
    base_date = datetime.datetime.now() - datetime.timedelta(days=ndays)
    # print(f"Querying Azure Table for performance statistics since {base_date}...")
    datetime_filter: str = f"(Timestamp gt datetime'{base_date.strftime('%Y-%m-%dT%H:%M:%S.%fZ')}')"
    scope_filter: str = f"(Scope eq 'NetworkLibOS')"
    libos_filter: str = f"(LibOS eq 'catnap' or LibOS eq 'catpowder' or LibOS eq 'catnip')"
    syscall_filter: str = f"(Syscall eq 'push' or Syscall eq 'pop')"
    query_filter: str = f"{datetime_filter} and {scope_filter} and {libos_filter} and {syscall_filter}"
    select: List[str] = ["LibOS", "JobName", "CommitHash", "Syscall", "AverageCyclesPerSyscall"]
    data = table_client.query_entities(query_filter=query_filter, select=select)

    job_types: list[str] = ["tcp-ping-pong-server", "tcp-ping-pong-client"]
    syscalls: list[str] = ["push", "pop"]
    libos_types: list[str] = ["catnap", "catpowder", "catnip"]

    head_commit: str = git.get_head_commit(branch_name)
    root_commit: str = git.get_root_commit(branch_name)

    print(f"Processing commits from {root_commit} to {head_commit} (in the last {ndays} days)")

    # Parse queried data.
    cooked_data = __process_data(data, job_types, syscalls, libos_types, root_commit, head_commit)

    # Build a report for each job type and syscall.
    for job_type in job_types:
        for syscall in syscalls:
            catpowder_df = pandas.DataFrame(cooked_data[job_type][syscall]["catpowder"])
            catpowder_df.sort_values(by=['diff'], inplace=True)
            catnap_df = pandas.DataFrame(cooked_data[job_type][syscall]["catnap"])
            catnap_df.sort_values(by=['diff'], inplace=True)
            catnip_df = pandas.DataFrame(cooked_data[job_type][syscall]["catnip"])
            catnip_df.sort_values(by=['diff'], inplace=True)
            df = pandas.merge(catpowder_df, catnap_df, on=["commit", "diff"])
            df = pandas.merge(df, catnip_df, on=["commit", "diff"])
            df.columns = ["Diff", "Catpowder", "Commit", "Catnap", "Catnip"]
            new_order = ["Diff", "Commit", "Catnap", "Catpowder", "Catnip"]
            df = df.reindex(columns=new_order)
            if libos is not None and libos in libos_types:
                df = df[["Diff", "Commit", libos.capitalize()]]
            if not df.empty:
                print(f"\n**Performance for {syscall.capitalize()}() in {job_type}**")
                print(f"\nDiff since root commit.")
                print(df.to_markdown())


def __process_data(data, job_types, syscalls, libos_types, root_commit, head_commit):
    cooked_data = {
        job: {
            syscall: {
                libos: {"diff": [], "cycles": [], "commit": []} for libos in libos_types}
            for syscall in syscalls}
        for job in job_types
    }

    # Processed data to avoid duplicates.
    processed = {}

    for row in data:
        for job_type in job_types:
            if fnmatch.fnmatch(row["JobName"], f"*{job_type}*"):
                for syscall in syscalls:
                    if syscall in row["Syscall"]:
                        for libos_type in libos_types:
                            if libos_type in row["LibOS"]:
                                hash = row["CommitHash"]
                                # Skip duplicates.
                                if (job_type, libos_type, hash, syscall) in processed:
                                    continue
                                # Skip invalid commits.
                                if not git.check_if_commit_is_valid(hash):
                                    continue
                                # Skip all commits, except for merge commits and the head commit.
                                if not (git.check_if_merge_commit(hash) or head_commit == hash):
                                    continue

                                try:
                                    processed[(job_type, libos_type, hash, syscall)] = True
                                    cooked_data[job_type][syscall][libos_type]["diff"].append(
                                        git.compute_commit_distance(root_commit, hash))
                                    cooked_data[job_type][syscall][libos_type]["cycles"].append(
                                        row["AverageCyclesPerSyscall"])
                                    cooked_data[job_type][syscall][libos_type]["commit"].append(
                                        git.get_short_commit_hash(hash))
                                except:
                                    print(f"Error processing commit {hash}")
    return cooked_data


if __name__ == "__main__":
    main()
