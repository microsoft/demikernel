# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import subprocess

# ======================================================================================================================


class BaseTask:
    def __init__(self, cmd: str):
        self.cmd: str = cmd

    def execute(self) -> subprocess.Popen[str]:
        return subprocess.Popen(self.cmd, shell=True, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
