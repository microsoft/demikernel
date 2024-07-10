# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from io import StringIO
import argparse
import ci.git as git
import glob
import os
import pandas as pd
import subprocess


def main():
    args: argparse.Namespace = __read_args()
    __build_report(args)


def __read_args() -> argparse.Namespace:
    description: str = 'CI Utility for reporting performance statistics of Demikernel.'
    parser = argparse.ArgumentParser(prog='plot.py', description=description)
    parser.add_argument('--branch', required=True, help='Current branch name.')
    parser.add_argument('--libos', required=False, help='LibOS Name.')
    parser.add_argument('--log-dir', required=False, help='The directory where the logs are stored.')
    return parser.parse_args()


def __build_report(args):
    commit_id = git.get_head_commit(args.branch)

    # This information is printed to the console and will be used by the
    # workflow to post a comment on the PR.
    print('libos = ' + args.libos)
    print('commit id = ' + commit_id)

    perf_df = __get_perf_data(args.log_dir)

    __print_perf_data(perf_df)

    __create_flame_graph(args.libos, commit_id, perf_df)

    # Save the perf data to a CSV file. This file will be used by the workflow
    # to archive the perf data for future reference.
    perf_df.to_csv('perf_data.csv', index=False, header=True)


def __get_perf_data(log_dir):

    # Get all the log files that have the perf data. The log files are named as
    # system-test*stdout*. There are other log files, but we are interested in
    # the perf data for system tests only. Tune this pattern to match the log
    # files that have the perf data that you are interested in.
    files = glob.glob(os.path.join(log_dir, '**', 'system-test*stdout*'), recursive=True)

    collapsed_stacks = []

    # Read the perf data from the log files and populate the collapsed stacks
    for file in files:
        with open(file, 'r') as f:
            __populate_collapsed_stacks(collapsed_stacks, f)

    # Create a dataframe from the collapsed stacks. This makes it easier to work
    # with the data for transformations.
    perf_df = pd.read_csv(
        StringIO('\n'.join(collapsed_stacks)),
        names=['collapsed_stack', 'percent_time', 'cycles_per_call', 'nanoseconds_per_call'])

    # There will be multiple entries for each function in the perf data coming
    # from different files. So, we need to collapse them into a single entry
    # by taking the mean of the values.
    perf_df = perf_df.groupby(by='collapsed_stack').mean().round(2).reset_index()

    return perf_df


# This function reads the perf data from the log file and populates the
# collapsed stacks. The collapsed stacks are a list of strings where each string
# is a collapsed stack entry. A collapsed stack entry is a string that contains
# the collapsed stack, percent time, cycles per call, and nanoseconds per call.
# The collapsed stack is a string that contains the function names separated by
# a semicolon. The percent time, cycles per call, and nanoseconds per call are
# the respective columns in the log file.
def __populate_collapsed_stacks(collapsed_stacks, file):

    file_df = __get_file_df(file)

    thread_ids = file_df['thread_id'].unique()

    for thread_id in thread_ids:

        # The current stack is used to keep track of the current function call
        # stack. This sort of mimics the call stack of the program being profiled.
        current_stack = []

        for _index, row in file_df[file_df['thread_id'] == thread_id].iterrows():

            depth = row['call_depth']

            # The current stack is reset when the depth of the function call is
            # 1. This denotes the start of a new function call stack.
            current_stack = [] if depth == 1 else current_stack[:depth-1]
            current_stack.append(row['function_name'])

            # The collapsed stack is a string that contains the function names
            # separated by a semicolon.
            collapsed_stack = ";".join(current_stack)
            collapsed_stacks.append(f"{collapsed_stack},{row['percent_time']},{row['cycles_per_call']},{row['nanoseconds_per_call']}")


def __get_file_df(file):
    lines = __extract_perf_lines(file)

    # Create a dataframe from the perf data. This makes it easier to work with
    # multiple blocks of perf data. Each thread has its own block of perf data.
    file_df = pd.read_csv(
        StringIO('\n'.join(lines)),
        delimiter=',',
        names=['call_depth', 'thread_id', 'function_name', 'percent_time',
               'cycles_per_call', 'nanoseconds_per_call'])

    # Number of '+' characters in the call_depth column denotes the depth of
    # the function call.
    file_df['call_depth'] = file_df['call_depth'].apply(lambda x: x.count('+')).astype(int)

    return file_df


def __extract_perf_lines(file):
    lines = []

    for line in file:
        # Skip lines that don't start with a '+' character because they are not
        # part of the perf data.
        if not line.startswith('+'):
            continue
        lines.append(line)
    return lines


def __print_perf_data(perf_df):

    # Typically, time is the most important metric to sort by. However, you can
    # sort by any column.
    sort_by_columns = ['percent_time', 'cycles_per_call', 'nanoseconds_per_call']

    # The columns that we are interested in displaying in the table.
    # collapsed_stack is important because it denotes the complete function call
    # stack.
    columns_to_display = ['collapsed_stack', 'percent_time', 'cycles_per_call', 'nanoseconds_per_call']

    # We are interested in the aggregated perf data. We sort the data by the
    # important columns and display only the relevant columns.
    out_df = perf_df.sort_values(by=sort_by_columns, ascending=False)[columns_to_display]

    # Print the perf data in a tabular format. This will be used by the workflow
    # to post a comment on the PR.
    print(out_df.to_markdown(floatfmt='.2f', index=False))


# This function creates a flame graph from the perf data. The flame graph is a
# visualization of the perf data. It shows the function calls and the time spent
# in each function. The flame graph is saved as an SVG file.
def __create_flame_graph(libos, commit_id, perf_df) -> None:

    # Save folded stacks to file for consumption by flamegraph.pl
    perf_df[['collapsed_stack', 'percent_time']].to_csv(
        'flamegraph_input.txt', index=False, sep=' ', header=False)

    # Render flame graph
    subprocess.run(['/tmp/FlameGraph/flamegraph.pl', 'flamegraph_input.txt',
                    '--countname', 'percent_time',
                    '--title', "libos = " + libos,
                    '--subtitle', "commit id = " + commit_id],
                    check=True,
                    stdout=open('flamegraph.svg', 'w'))


if __name__ == '__main__':
    main()
