# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.task.generic import BaseTask

# ======================================================================================================================


class BaseWindowsTask(BaseTask):

    def __init__(self, host: str, cmd: str):
        ssh_cmd: str = f"ssh {host} \"{cmd}\""
        super().__init__(ssh_cmd)

    @staticmethod
    def _build_env_cmd() -> str:
        rust_path = "\$RustPath = Join-Path \$Env:HOME \\.cargo\\bin"
        git_path = "\$GitPath = Join-Path \$Env:ProgramFiles \\Git\\cmd"
        env_path_git = "\$Env:Path += \$GitPath + \';\'"
        env_path_rust = "\$Env:Path += \$RustPath + \';\'"
        vs_install_path = "\$VsInstallPath = &(Join-Path \${Env:ProgramFiles(x86)} '\\Microsoft Visual Studio\\Installer\\vswhere.exe') -latest -property installationPath"
        import_module = "Import-Module (Join-Path \$VsInstallPath 'Common7\\Tools\\Microsoft.VisualStudio.DevShell.dll')"
        enter_vsdevshell = "Enter-VsDevShell -VsInstallPath \$VsInstallPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'"

        env_cmd = " ; ".join([rust_path, git_path, env_path_git, env_path_rust, vs_install_path,
                              import_module, enter_vsdevshell])
        return env_cmd


class CheckoutOnWindows(BaseWindowsTask):
    def __init__(self, host: str, repository: str, branch: str):
        env_cmd = BaseWindowsTask._build_env_cmd()
        cmd: str = f"cd {repository} ; {env_cmd} ; git pull origin ; git checkout {branch}"
        super().__init__(host, cmd)


class CompileOnWindows(BaseWindowsTask):
    def __init__(self, host: str, repository: str, target: str, is_debug: bool):
        env_cmd = BaseWindowsTask._build_env_cmd()
        debug_flag: str = "DEBUG=yes" if is_debug else "DEBUG=no"
        profiler_flag: str = "PROFILER=yes" if not is_debug else "PROFILER=no"
        cmd: str = f"cd {repository} ; {env_cmd} ; nmake {profiler_flag} {debug_flag} {target}"
        super().__init__(host, cmd)


class RunOnWindows(BaseWindowsTask):
    def __init__(self, host: str, repository: str, target: str, is_debug: bool, is_sudo: bool, config_path: str):
        env_cmd = BaseWindowsTask._build_env_cmd()
        debug_flag: str = "DEBUG=yes" if is_debug else "DEBUG=no"
        cmd: str = f"cd {repository} ; {env_cmd} ; nmake CONFIG_PATH={config_path} {debug_flag} {target}"
        super().__init__(host, cmd)


class CleanupOnWindows(BaseWindowsTask):
    def __init__(self, host: str, repository: str, is_sudo: bool, branch: str):
        env_cmd = BaseWindowsTask._build_env_cmd()
        cmd: str = f"cd {repository} ; {env_cmd} ; nmake clean ; git checkout ; git clean -fdx"
        super().__init__(host, cmd)
