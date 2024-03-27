# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import datetime
import fnmatch
import subprocess
from typing import List
from azure.data.tables import TableServiceClient
import pandas
import matplotlib.pyplot as plt
import argparse
from azure.storage.blob import BlobClient

# =====================================================================================================================


# Drives the program.
def main():
    # Read arguments from command line and parse them.
    args: argparse.Namespace = __read_args()

    # Extract optionss.
    table_name: str = args.table
    container_name: str = args.container
    connection_str: str = args.connection
    key: str = args.key
    no_plot: bool = args.no_plot
    branch_name: str = args.branch
    libos: str = args.libos

    __plot_performance(table_name=table_name, container_name=container_name,
                       connection_str=connection_str, key=key, no_plot=no_plot, branch_name=branch_name, libos=libos)


# Reads and parses command line arguments.
def __read_args() -> argparse.Namespace:
    description: str = "CI Utility for reporting performance statistics of Demikernel."

    # Initialize parser.
    parser = argparse.ArgumentParser(prog="plot.py", description=description)

    # Options related to Storage account.
    parser.add_argument("--table", required=True, help="Set Azure Table to use.")
    parser.add_argument("--container", required=True, help="Set Azure Blob Container to use.")

    # Options related to credentials.
    parser.add_argument("--connection", required=True, help="Set connection string to access Azure Storage Account.")
    parser.add_argument("--key", required=True, help="Set connection key to access Azure Storage Account.")

    # Options related to repository.
    parser.add_argument("--branch", required=True, help="Set the branch to report performance statistics for.")
    parser.add_argument("--libos", required=False, help="Set the library OS to report performance statistics for.")

    # Output options.
    parser.add_argument("--no-plot", action="store_true", help="Do not plot the performance statistics.")

    # Read arguments from command line.
    return parser.parse_args()


# Get head commit on branch.
def get_head_commit(branch_name: str) -> str:
    cmd = f"git show --format=%H -s {branch_name}"
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    return git_stdout.replace("\n", "")


# Get first commit on branch.
def get_first_commit_on_branch(head_commit: str) -> str:
    cmd = f"git rev-list --max-parents=0 {head_commit}"
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    return git_stdout.replace("\n", "")


# Check if commit is a merge commit.
def check_if_merge_commit(commit_hash: str) -> bool:
    cmd = "git show --format=%P -s {}".format(commit_hash)
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    return len(git_stdout.split()) > 1


# Check if commit is a head commit.
def check_if_head_commit(head_commit: str, commit_hash: str) -> bool:
    return True if commit_hash == head_commit else False


# Get short commit hash.
def get_short_commit_hash(commit_hash: str) -> int:
    cmd = "git rev-parse --short {}".format(commit_hash)
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    return int(git_stdout.replace("\n", ""), 16)


# Compute distance of two commit hashes.
def get_distance_of_commits(base_commit: str, head_commit: str) -> int:
    cmd = "git rev-list --count {}..{}".format(base_commit, head_commit)
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    git_stdout = git_stdout.replace("\n", "")
    if git_stdout == "":
        git_stdout = "0"
    return int(git_stdout)


def __plot_performance(table_name: str, container_name: str, connection_str: str, key: str, no_plot: bool, branch_name: str, libos: str) -> None:
    # Connect to Azure table.
    table_service = TableServiceClient.from_connection_string(connection_str)
    table_client = table_service.get_table_client(table_name)

    # Query Azure table for statistics on the past 30 days.
    ndays: int = 15
    base_date = datetime.datetime.now() - datetime.timedelta(days=ndays)
    # print(f"Querying Azure Table for performance statistics since {base_date}...")
    query_filter: str = f"Timestamp gt datetime'{base_date.strftime('%Y-%m-%dT%H:%M:%S.%fZ')}' and" + \
        "(LibOS eq 'catnap' or LibOS eq 'catpowder' or LibOS eq 'catnip') and (Syscall eq 'push' or Syscall eq 'pop')"
    select: List[str] = ["LibOS", "JobName", "CommitHash", "Syscall", "AverageCyclesPerSyscall"]
    data = table_client.query_entities(query_filter=query_filter, select=select)

    cooked_data = {
        "tcp-ping-pong-server": {
            "push": {
                "catnap": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catnip": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
            },
            "pop": {
                "catnap": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catnip": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
            },
        },
        "tcp-ping-pong-client": {
            "push": {
                "catnap": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catnip": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
            },
            "pop": {
                "catnap": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
                "catnip": {
                    "diff": [],
                    "cycles": [],
                    "commit": []
                },
            },
        }
    }

    job_types = ["tcp-ping-pong-server", "tcp-ping-pong-client"]
    syscalls = ["push", "pop"]
    libos_types = ["catnap", "catpowder", "catnip"]

    # Hashtable of commits.
    commits = {}
    head_commit: str = get_head_commit(branch_name)
    root_commit: str = get_first_commit_on_branch(head_commit)

    print(f"Processing commits from {root_commit} to {head_commit} (in the last {ndays} days)")

    # Parse queried data.
    for row in data:
        for job_type in job_types:
            if fnmatch.fnmatch(row["JobName"], f"*{job_type}*"):
                for syscall in syscalls:
                    if syscall in row["Syscall"]:
                        for libos_type in libos_types:
                            if libos_type in row["LibOS"]:
                                hash = row["CommitHash"]
                                if check_if_head_commit(head_commit, hash) or check_if_merge_commit(hash):
                                    if not (job_type, libos_type, hash, syscall) in commits:
                                        commits[(job_type, libos_type, hash, syscall)] = True
                                        cooked_data[job_type][syscall][libos_type]["diff"].append(
                                            get_distance_of_commits(root_commit, hash))
                                        cooked_data[job_type][syscall][libos_type]["cycles"].append(
                                            row["AverageCyclesPerSyscall"])
                                        cooked_data[job_type][syscall][libos_type]["commit"].append(
                                            get_short_commit_hash(hash))

    # print(json.dumps(cooked_data, indent=4))
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
            # Set number format for columns.
            df["Diff"] = df["Diff"].astype(int)
            df["Catpowder"] = df["Catpowder"].astype(str)
            df["Catnap"] = df["Catnap"].astype(str)
            df["Catnip"] = df["Catnip"].astype(str)
            df["Commit"] = df["Commit"].apply(lambda x: f"{x:08x}")
            if libos is not None and libos in ["catpowder", "catnap", "catnip"]:
                df = df[["Diff", "Commit", libos.capitalize()]]
            if not df.empty:
                if not no_plot:
                    df.plot(x="Diff", y=["Catpowder", "Catnap", "Catnip"],
                            kind="line", marker='o',
                            title=f"Performance for {syscall.capitalize()}()",
                            xlabel="Commit Hash",
                            ylabel="Average Cycles Spent in Syscall",
                            legend=True, ylim=(0, 5000))
                    plt.xticks(rotation=90, ticks=df["Diff"], labels=df["Commit"])
                    plt.savefig(f"{job_type}-{syscall}.png", bbox_inches='tight', dpi=300)
                    upload_image_to_blob("demikernel", key, container_name,
                                         f"{head_commit}-{job_type}-{syscall}.png", f"{job_type}-{syscall}.png")
                else:
                    print(f"\n**Performance for {syscall.capitalize()}() in {job_type}**")
                    print(f"\nDiff since root commit.")
                    print(df.to_markdown())


def upload_image_to_blob(account_name, account_key, container_name, blob_name, image_path):
    blob_client = BlobClient(account_url=f"https://{account_name}.blob.core.windows.net",
                             container_name=container_name,
                             blob_name=blob_name,
                             credential=account_key)

    with open(image_path, 'rb') as f:
        blob_client.upload_blob(f, overwrite=True)


if __name__ == "__main__":
    main()
