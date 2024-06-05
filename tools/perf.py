# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import os
import argparse
import ci.git as git
import pandas as pd


def main():
    args: argparse.Namespace = __read_args()
    __build_report(args)


def __read_args() -> argparse.Namespace:
    description: str = "CI Utility for reporting performance statistics of Demikernel."
    parser = argparse.ArgumentParser(prog="plot.py", description=description)
    parser.add_argument("--branch", required=True, help="Current branch name.")
    parser.add_argument("--libos", required=False, help="LibOS Name.")
    parser.add_argument("--log-dir", required=False, help="The directory where the logs are stored.")
    return parser.parse_args()


def __build_report(args) -> None:
    print("libos = " + args.libos)
    print("commit id = " + git.get_head_commit(args.branch))

    lines = []
    out_path = os.path.join(args.log_dir, "perf_data.csv")

    for root, _dirs, files in os.walk(args.log_dir):
            for filename in files:
                if "system-test" in filename and "stdout" in filename:
                    with open(os.path.join(root, filename), "r") as file:
                        for line in file.readlines():
                            if line.startswith("+"):
                                line = line.lstrip("+;")
                                line = line.replace(";", ",")
                                lines.append(line)

    with open(out_path, "w") as f:
        f.writelines(lines)

    perf_df = pd.read_csv(
        out_path,
        delimiter=",",
        names=["fn", "percent_time", "mean_cycles_per_call", "mean_ns_per_call"])
    perf_df = perf_df.groupby("fn").mean().round(2)
    perf_df = perf_df.sort_values(
        ["percent_time", "mean_cycles_per_call", "mean_ns_per_call"], ascending=False)

    print(perf_df.to_markdown(floatfmt='.2f'))


if __name__ == "__main__":
    main()
