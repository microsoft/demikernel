# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import subprocess
from ci.job.utils import wait_and_report
from ci.task.generic import BaseTask

# ======================================================================================================================
# Generic Jobs
# ======================================================================================================================


class BaseJob:
    def __init__(self, config, name):
        self.name = name
        self.config = config

    def execute(self, serverTask: BaseTask, clientTask: BaseTask = None) -> bool:
        jobs: dict[str, subprocess.Popen[str]] = {}

        jobs[self.name + "-server-" + self.server()] = serverTask.execute()

        if clientTask is not None:
            jobs[self.name + "-client-" +
                 self.client()] = clientTask.execute()
        return wait_and_report(self.name, self.log_directory(), jobs)

    def branch(self) -> str:
        return self.config["branch"]

    def server(self) -> str:
        return self.config["server"]

    def client(self) -> str:
        return self.config["client"]

    def repository(self) -> str:
        return self.config["repository"]

    def enable_nfs(self) -> bool:
        return self.config["enable_nfs"]

    def is_sudo(self) -> bool:
        return self.config["is_sudo"]

    def config_path(self) -> str:
        return self.config["config_path"]

    def is_debug(self) -> bool:
        return self.config["is_debug"]

    def libos(self) -> str:
        return self.config["libos"]

    def log_directory(self) -> str:
        return self.config["log_directory"]

    def delay(self) -> float:
        return self.config["delay"]

    def server_addr(self) -> str:
        return self.config["server_addr"]

    def client_addr(self) -> str:
        return self.config["client_addr"]
