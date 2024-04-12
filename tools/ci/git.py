# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

# ======================================================================================================================
# Imports
# ======================================================================================================================

import subprocess

# ======================================================================================================================
# Private constants
# ======================================================================================================================


# Length of a long git commit hash.
__LONG_COMMIT_HASH_LENGTH: int = 40

# ======================================================================================================================
# Private Functions
# ======================================================================================================================


def __validate_commit_hash(commit_hash: str) -> None:
    """Validates a commit hash by checking its length."""
    if commit_hash == "" or len(commit_hash) != __LONG_COMMIT_HASH_LENGTH:
        raise ValueError(f"Invalid commit hash: {commit_hash}")

# ======================================================================================================================
# Public Functions
# ======================================================================================================================


def check_if_commit_is_valid(commit_hash: str) -> bool:
    """Checks if a commit is valid."""

    # We don't validate commit hash here, as it will be validated later.

    # Spawn a bash command to check if a commit hash is valid.
    git_cmd: str = f"git rev-parse --quiet --verify {commit_hash}^{{commit}}"
    bash_cmd: str = f"bash -l -c \'{git_cmd}\'"
    process: subprocess.Popen = subprocess.Popen(
        bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = process.communicate()
    stdout: str = stdout.replace("\n", "")

    return False if stdout == "" or stderr != "" else True


def get_head_commit(branch_name: str) -> str:
    """Gets the head commit of a branch."""

    # Sanity check the input.
    if branch_name == "":
        raise ValueError("Expected non-empty branch name")

    # Spawn a bash command to get the head commit hash of a branch.
    git_cmd: str = f"git show --format=%H -s {branch_name}"
    bash_cmd: str = f"bash -l -c \'{git_cmd}\'"
    process = subprocess.Popen(bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = process.communicate()
    stdout: str = stdout.replace("\n", "")

    # Sanity check result.
    if stdout == "":
        raise ValueError(f"Expected non-empty output for {git_cmd}")
    if stderr != "":
        raise ValueError(f"Expected empty error for {git_cmd}, got {stderr}")

    return stdout


def get_root_commit(branch_name: str) -> str:
    """Gets the root commit of a branch."""

    # Sanity check the input.
    if branch_name == "":
        raise ValueError("Expected non-empty branch name")

    # Spawn a bash command to get the root commit of a branch.
    git_cmd: str = f"git rev-list --date-order --max-parents=0 {branch_name} | tail -n 1"
    bash_cmd: str = f"bash -l -c \'{git_cmd}\'"
    process: subprocess.Popen = subprocess.Popen(
        bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = process.communicate()
    stdout: str = stdout.replace("\n", "")

    if stdout == "":
        raise ValueError(f"Expected non-empty output for {git_cmd}")
    if stderr != "":
        raise ValueError(f"Expected empty error for {git_cmd}, got {stderr}")

    return stdout


def check_if_merge_commit(commit_hash: str) -> bool:
    """Checks if a commit is a merge commit."""

    # Sanity check the input.
    __validate_commit_hash(commit_hash)

    # Spawn a bash command count the number of parents of a commit.
    git_command: str = f"git show --format=%P -s {commit_hash}"
    bash_cmd: str = f"bash -l -c \'{git_command}\'"
    process: subprocess.Popen = subprocess.Popen(
        bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, _ = process.communicate()
    stdout: str = stdout.replace("\n", "")
    # We don't assert stdout and stderr, as for orphan commits are reported as errors.

    return len(stdout.split()) > 1


def get_short_commit_hash(commit_hash: str) -> str:
    """Gets the short hash of a commit."""

    # Sanity check the input.
    __validate_commit_hash(commit_hash)

    # Spawn a bash command to get the short hash of a commit.
    git_cmd: str = f"git rev-parse --short {commit_hash}"
    bash_cmd: str = f"bash -l -c \'{git_cmd}\'"
    process: subprocess.Popen = subprocess.Popen(
        bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = process.communicate()
    stdout: str = stdout.replace("\n", "")

    if stdout == "":
        raise ValueError(f"Expected non-empty output for {git_cmd}")
    if stderr != "":
        raise ValueError(f"Expected empty error for {git_cmd}, got {stderr}")

    return stdout


def compute_commit_distance(base_commit_hash: str, head_commit_hash: str) -> int:
    """Computes the distance between two commits."""

    # Sanity check the input.
    __validate_commit_hash(base_commit_hash)
    __validate_commit_hash(head_commit_hash)

    # Spawn a bash command to compute the distance between two commit hashes.
    git_cmd: str = f"git rev-list --count {base_commit_hash}..{head_commit_hash}"
    bash_cmd: str = f"bash -l -c \'{git_cmd}\'"
    process: subprocess.Popen = subprocess.Popen(
        bash_cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    stdout, stderr = process.communicate()
    stdout: str = stdout.replace("\n", "")

    if stdout == "":
        raise ValueError(f"Expected non-empty output for {git_cmd}")
    if stderr != "":
        raise ValueError(f"Expected empty error for {git_cmd}, got {stderr}")

    return int(stdout)
