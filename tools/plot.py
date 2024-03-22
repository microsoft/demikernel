# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import subprocess
from typing import List
from azure.data.tables import TableServiceClient
from matplotlib.ticker import MaxNLocator
import pandas
import matplotlib.pyplot as plt
import sys
import argparse
from os import mkdir
from shutil import move, rmtree
from os.path import isdir
import yaml
from ci.job.utils import get_commit_hash
from azure.storage.blob import BlobClient
# ======================================================================================================================
# Global Variables
# ======================================================================================================================

COMMIT_HASH: str = ""

# =====================================================================================================================


def read_yaml():
    path = "tools/ci/config/benchmark.yaml"
    yaml_str = ""
    with open(path) as f:
        yaml_str = f.read()
    return yaml.safe_load(yaml_str)


# Reads and parses command line arguments.
def read_args() -> argparse.Namespace:
    description: str = ""

    # Initialize parser.
    parser = argparse.ArgumentParser(prog="plot.py", description=description)

<<<<<<< Updated upstream
    # Build options.
    parser.add_argument("--libos", required=True,
                        help="set target libos in remote hosts")

=======
>>>>>>> Stashed changes
    # Other options.
    parser.add_argument("--connection-string", required=True,
                        default="", help="connection string to access Azure tables")
    parser.add_argument("--table-name", required=True,
                        default="", help="Azure table to place results")
    parser.add_argument("--key", required=True,
                        default="", help="Azure table to place plots")

    # Read arguments from command line.
    return parser.parse_args()


# Drives the program.
def main():
    # Parse and read arguments from command line.
    args: argparse.Namespace = read_args()

    # Extract build options.
<<<<<<< Updated upstream
    libos: str = args.libos
=======
>>>>>>> Stashed changes
    key: str = args.key
    connection_string: str = args.connection_string
    table_name: str = args.table_name

    # Initialize glboal variables.
    get_commit_hash()

<<<<<<< Updated upstream
    extract_performance(connection_string, key, libos,
                        "demikernel", table_name, "benchmark-tcp-echo")
=======
    extract_performance(connection_string, key, "demikernel",
                        table_name, "benchmark-tcp-echo")
>>>>>>> Stashed changes


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


# Get DateTime of a commit hash.
def get_datetime_of_commit(commit_hash: str) -> str:
    cmd = "git show -s --format=%ci {}".format(commit_hash)
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(
        git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    git_stdout = git_stdout.replace("\n", "")
    return git_stdout

# Compute distance of two commit hashes.


def get_distance_of_commits(commit_hash1: str) -> int:
<<<<<<< Updated upstream
    cmd = "git rev-list --count {}..HEAD".format(commit_hash1)
=======
    cmd = "git rev-list --count dev..{}".format(commit_hash1)
>>>>>>> Stashed changes
    git_cmd = "bash -l -c \'{}\'".format(cmd)
    git_process = subprocess.Popen(
        git_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_stdout, _ = git_process.communicate()
    git_stdout = git_stdout.replace("\n", "")
    return int(git_stdout)


<<<<<<< Updated upstream
def extract_performance(connection_string, key, libos, account_name, table_name, container_name):
=======
def extract_performance(connection_string, key, account_name, table_name, container_name):
>>>>>>> Stashed changes
    # Connect to Azure Tables.
    table_service = TableServiceClient.from_connection_string(
        connection_string)
    table_client = table_service.get_table_client(table_name)

    # Retrieve all entities from the table.
<<<<<<< Updated upstream
    query_filter = "LibOS eq @libos"
    parameters = {"libos": libos}
    data = table_client.query_entities(
        query_filter=query_filter, select=["DateTime", "JobName", "CommitHash", "Syscall", "AverageCyclesPerSyscall"], parameters=parameters
    )

    df = pandas.DataFrame(data)

    df['Diff'] = df['CommitHash'].apply(get_distance_of_commits).astype(int)

    pop_syscall = ["pop", 1000]
    push_syscall = ["push", 1000]
    syscall_names = [push_syscall, pop_syscall]

    for syscall in syscall_names:
        syscall_name = syscall[0]
        syscall_y_max = syscall[1]
        print(f"Plotting {syscall_name}...")
        df_syscall: pandas.DataFrame = df[df["Syscall"].str.contains(
            syscall_name)]

        # Use the function
        plot_and_upload(df_syscall, "server", syscall_name,
                        syscall_y_max, account_name, key, container_name)
        plot_and_upload(df_syscall, "client", syscall_name,
                        syscall_y_max, account_name, key, container_name)


def plot_and_upload(df: pandas.DataFrame, job_type, syscall_name, syscall_y_max, account_name, key, container_name):
    df = df[df["JobName"].str.contains(job_type)]
    df = df.sort_values(by="Diff")
    df['AverageCyclesPerSyscallInThousands'] = df['AverageCyclesPerSyscall'] / 1000
    filename: str = f"{syscall_name}-{job_type}.png"
    plot = df.plot(x="Diff", y="AverageCyclesPerSyscall",
                   kind="line", marker="o")
    plot.xaxis.set_major_locator(MaxNLocator(integer=True))
    plot.set_xlabel("Commit Distance")
    plot.set_title(
        f"Performance for {syscall_name}() in {job_type.capitalize()}")
    plot.legend(labels=[])
    plot.set_ylim(bottom=0, top=syscall_y_max)
    plot.set_ymargin(0.0)
    plot.set_ylabel("Average Cycles Spent in Syscall")
=======
    query_filter = ""
    parameters = {}
    data = table_client.query_entities(
        query_filter=query_filter, select=["LibOS", "JobName", "CommitHash", "Syscall", "AverageCyclesPerSyscall"], parameters=parameters
    )

    cooked_data = {
        "server": {
            "push": {
                "catnap": {
                    "diff": [],
                    "cycles": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": []
                },
            },
            "pop": {
                "catnap": {
                    "diff": [],
                    "cycles": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": []
                },
            },
        },
        "client": {
            "push": {
                "catnap": {
                    "diff": [],
                    "cycles": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": []
                },
            },
            "pop": {
                "catnap": {
                    "diff": [],
                    "cycles": []
                },
                "catpowder": {
                    "diff": [],
                    "cycles": []
                },
            },
        }
    }

    job_types = ["server", "client"]
    syscalls = ["push", "pop"]
    libos_types = ["catnap", "catpowder"]

    for row in data:
        for job_type in job_types:
            if job_type in row["JobName"]:
                for syscall in syscalls:
                    if syscall in row["Syscall"]:
                        for libos_type in libos_types:
                            if libos_type in row["LibOS"]:
                                cooked_data[job_type][syscall][libos_type]["diff"].append(
                                    get_distance_of_commits(row["CommitHash"]))
                                cooked_data[job_type][syscall][libos_type]["cycles"].append(
                                    row["AverageCyclesPerSyscall"])

    for job_type in job_types:
        for syscall in syscalls:
            catpowder_df = pandas.DataFrame(
                cooked_data[job_type][syscall]["catpowder"])
            catnap_df = pandas.DataFrame(
                cooked_data[job_type][syscall]["catnap"])
            df = pandas.merge(catpowder_df, catnap_df, on="diff")
            df.columns = ["Diff", "Catpowder", "Catnap"]
            print(f"Plotting {job_type} {syscall}...")
            print(df)
            df.plot(x="Diff", y=["Catpowder", "Catnap"], kind="line")
            plt.title(
                f"Performance for {syscall.capitalize()}() in {job_type.capitalize()}")
            plt.xlabel("Commit Distance")
            plt.ylabel("Average Cycles Spent in Syscall")
            plt.legend()
            plt.ylim(bottom=0)
            plt.savefig(f"{job_type}-{syscall}.png")

    # df['Diff'] = df['CommitHash'].apply(get_distance_of_commits).astype(int)

    # pop_syscall = ["pop", 1000]
    # push_syscall = ["push", 1000]
    # syscall_names = [push_syscall, pop_syscall]

    # for syscall in syscall_names:
    #     syscall_name = syscall[0]
    #     syscall_y_max = syscall[1]
    #     print(f"Plotting {syscall_name}...")
    #     df_syscall: pandas.DataFrame = df[df["Syscall"].str.contains(
    #         syscall_name)]

    # plot_and_upload(libos, df_syscall, "server", syscall_name,
    #                 syscall_y_max, account_name, key, container_name)
    # plot_and_upload(libos, df_syscall, "client", syscall_name,
    #                 syscall_y_max, account_name, key, container_name)


def plot_and_upload(libos: str, df: pandas.DataFrame, job_type, syscall_name, syscall_y_max, account_name, key, container_name):
    # filter out rows matching a libos.
    df = df[df["JobName"].str.contains(job_type)]
    df = df.sort_values(by="Diff")
    catnap_df = df[df["LibOS"].str.contains("catnap")]
    catpowder_df = df[df["LibOS"].str.contains("catpowder")]

    print(catnap_df.set_index("Diff"))
    # print(catpowder_df)

    filename: str = f"{syscall_name}-{job_type}.png"
    # plt.plot("Diff", "AverageCyclesPerSyscall",
    #          data=catnap_df, label='Catnap')
    # plt.plot("Diff", "AverageCyclesPerSyscall",
    #          data=catpowder_df, label='Catpowder')
    plt.plot(x="Diff", y=[""])
    plt.xlabel("Commit Distance")
    plt.title(f"Performance for {syscall_name}() in {job_type.capitalize()}")
    # plt.ylim(bottom=0)
    plt.ylabel("Average Cycles Spent in Syscall")
    plt.legend()

>>>>>>> Stashed changes
    plt.savefig(filename)
    upload_image_to_blob(account_name, key, container_name, filename, filename)


def upload_image_to_blob(account_name, account_key, container_name, blob_name, image_path):
    blob_client = BlobClient(account_url=f"https://{account_name}.blob.core.windows.net",
                             container_name=container_name,
                             blob_name=blob_name,
                             credential=account_key)

    # Upload the image
    # with open(image_path, 'rb') as f:
    #     blob_client.upload_blob(f, overwrite=True)


if __name__ == "__main__":
    main()
