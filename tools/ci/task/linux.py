# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.task.generic import BaseTask

# ======================================================================================================================


class BaseLinuxTask(BaseTask):
    def __init__(self, host: str, cmd: str):
        ssh_cmd: str = f"ssh -C {host} \"bash -l -c \'{cmd}\'\""
        super().__init__(ssh_cmd)


class CheckoutOnLinux(BaseLinuxTask):
    def __init__(self, host: str, repository: str, branch: str):
        cmd: str = f"cd {repository} && git pull origin && git checkout {branch}"
        super().__init__(host, cmd)


class CompileOnLinux(BaseLinuxTask):
    def __init__(self, host: str, repository: str, target: str, is_debug: bool):
        debug_flag: str = "DEBUG=yes" if is_debug else "DEBUG=no"
        profiler_flag: str = "PROFILER=yes" if not is_debug else "PROFILER=no"
        cmd: str = f"cd {repository} && make {profiler_flag} {debug_flag} {target}"
        super().__init__(host, cmd)


class RunOnLinux(BaseLinuxTask):
    def __init__(self, host: str, repository: str, target: str, is_debug: bool, is_sudo: bool, config_path: str):
        debug_flag: str = "DEBUG=yes" if is_debug else "DEBUG=no"
        sudo_cmd: str = "sudo -E" if is_sudo else ""
        profiler_flag: str = "PROFILER=yes" if not is_debug else "PROFILER=no"
        cmd: str = f"cd {repository} && {sudo_cmd} make -j 1 CONFIG_PATH={config_path} {profiler_flag} {debug_flag} {target} 2> out.stderr && cat out.stderr >&2 || ( cat out.stderr >&2 ; exit 1 )"
        super().__init__(host, cmd)


class CleanupOnLinux(BaseLinuxTask):
    def __init__(self, host: str, repository: str, is_sudo: bool, branch: str):
        sudo_cmd: str = "sudo -E" if is_sudo else ""
        cmd: str = f"cd {repository} && {sudo_cmd} make clean && git checkout {branch} && git clean -fdx ; sudo -E rm -rf /dev/shm/demikernel* ; sudo pkill -f demikernel*"
        super().__init__(host, cmd)
