# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.task.windows import BaseWindowsTask, CheckoutOnWindows, CompileOnWindows, RunOnWindows, CleanupOnWindows
from ci.job.utils import wait_and_report
from ci.job.generic import BaseJob


# ======================================================================================================================
# Generic Jobs for Windows
# ======================================================================================================================


class BaseWindowsJob(BaseJob):
    def __init__(self, config, name):
        super().__init__(config, name)

    def execute(self, serverTask: BaseWindowsTask, clientTask: BaseWindowsTask = None) -> bool:
        return super().execute(serverTask, clientTask)


class CheckoutJobOnWindows(BaseWindowsJob):

    def __init__(self, config: dict):
        super().__init__(config, "checkout")

    def execute(self) -> bool:
        serverTask: CheckoutOnWindows = CheckoutOnWindows(
            super().server(), super().repository(), super().branch())

        if not super().enable_nfs():
            clientTask: CheckoutOnWindows = CheckoutOnWindows(
                super().client(), super().repository(), super().branch())
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class CompileJobOnWindows(BaseWindowsJob):

    def __init__(self, config: dict):
        name = "compile-{}".format("debug" if config["is_debug"] else "release")
        super().__init__(config, name)

    def execute(self) -> bool:
        cmd: str = f"all LIBOS={super().libos()}"
        serverTask: CompileOnWindows = CompileOnWindows(
            super().server(), super().repository(), cmd, super().is_debug())

        if not super().enable_nfs():
            clientTask: CompileOnWindows = CompileOnWindows(
                super().client(), super().repository(), cmd, super().is_debug())
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class CleanupJobOnWindows(BaseWindowsJob):
    def __init__(self, config: dict):
        super().__init__(config, "cleanup")

    def execute(self) -> bool:
        default_branch: str = "dev"
        serverTask: CleanupOnWindows = CleanupOnWindows(
            super().server(), super().repository(), super().is_sudo(), default_branch)

        if not super().enable_nfs():
            clientTask: CleanupOnWindows = CleanupOnWindows(
                super().client(), super().repository(), super().is_sudo(), default_branch)
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class UnitTestJobOnWindows(BaseWindowsJob):
    def __init__(self, config: dict):
        super().__init__(config, "unit-test")

    def execute(self) -> bool:
        server_cmd: str = f"test-unit-rust LIBOS={super().libos()}"
        serverTask: RunOnWindows = RunOnWindows(
            super().server(), super().repository(), server_cmd, super().is_debug(), super().is_sudo(), super().config_path())
        return super().execute(serverTask)
