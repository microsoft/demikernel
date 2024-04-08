# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import subprocess
import time
import ci.task.linux as linux
from ci.task.linux import BaseLinuxTask, CheckoutOnLinux, CompileOnLinux, RunOnLinux, CleanupOnLinux
from ci.job.utils import wait_and_report
from ci.job.generic import BaseJob

# ======================================================================================================================
# Generic Jobs for Linux
# ======================================================================================================================


class BaseLinuxJob(BaseJob):
    def __init__(self, config, name):
        super().__init__(config, name)

    def execute(self, serverTask: BaseLinuxTask, clientTask: BaseLinuxTask = None, no_wait: bool = False) -> bool:
        return super().execute(serverTask, clientTask, no_wait)


class CheckoutJobOnLinux(BaseLinuxJob):

    def __init__(self, config: dict):
        super().__init__(config, "checkout")

    def execute(self) -> bool:
        serverTask: CheckoutOnLinux = CheckoutOnLinux(
            super().server(), super().repository(), super().branch())

        if not super().enable_nfs():
            clientTask: CheckoutOnLinux = CheckoutOnLinux(
                super().client(), super().repository(), super().branch())
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class CompileJobOnLinux(BaseLinuxJob):

    def __init__(self, config: dict):
        name = "compile-{}".format("debug" if config["is_debug"] else "release")
        super().__init__(config, name)

    def execute(self) -> bool:
        cmd: str = f"all LIBOS={super().libos()}"
        serverTask: CompileOnLinux = CompileOnLinux(
            super().server(), super().repository(), cmd, super().is_debug())

        if not super().enable_nfs():
            clientTask: CompileOnLinux = CompileOnLinux(
                super().client(), super().repository(), cmd, super().is_debug())
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class CleanupJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "cleanup")

    def execute(self) -> bool:
        default_branch: str = "dev"
        serverTask: CleanupOnLinux = CleanupOnLinux(
            super().server(), super().repository(), super().is_sudo(), default_branch)

        if not super().enable_nfs():
            clientTask: CleanupOnLinux = CleanupOnLinux(
                super().client(), super().repository(), super().is_sudo(), default_branch)
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class UnitTestJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict, name: str):
        super().__init__(config, name)

    def execute(self) -> bool:
        server_cmd: str = f"{super().name()} LIBOS={super().libos()}"
        serverTask: RunOnLinux = RunOnLinux(
            super().server(), super().repository(), server_cmd, super().is_debug(), super().is_sudo(), super().config_path())
        return super().execute(serverTask)


class UnitTestRustJobOnLinux(UnitTestJobOnLinux):
    def __init__(self, config: dict):
        super().__init__(config, "test-unit-rust")

    def execute(self) -> bool:
        return super().execute()


class UnitTestCJobOnLinux(UnitTestJobOnLinux):
    def __init__(self, config: dict):
        super().__init__(config, "test-unit-c")

    def execute(self) -> bool:
        return super().execute()


class EndToEndTestJobOnLinux(BaseLinuxJob):

    def __init__(self, config, job_name: str):
        self.job_name = job_name
        self.all_pass = config["all_pass"]
        super().__init__(config, job_name)

    def execute(self, server_cmd: str, client_cmd: str) -> bool:
        serverTask: RunOnLinux = RunOnLinux(
            super().server(), super().repository(), server_cmd, super().is_debug(), super().is_sudo(), super().config_path())
        jobs: dict[str, subprocess.Popen[str]] = {}
        jobs[self.job_name + "-server-" +
             super().server()] = serverTask.execute()
        time.sleep(super().delay())
        clientTask: RunOnLinux = RunOnLinux(
            super().client(), super().repository(), client_cmd, super().is_debug(), super().is_sudo(), super().config_path())
        jobs[self.job_name + "-client-" +
             super().client()] = clientTask.execute()
        return wait_and_report(self.job_name, super().log_directory(), jobs, self.all_pass)


class SystemTestJobOnLinux(EndToEndTestJobOnLinux):
    def __init__(self, config: dict):
        self.test_name = config["test_name"]
        self.server_args = config["server_args"]
        self.client_args = config["client_args"]
        super().__init__(config, f"system-test-{config['test_alias']}")

    def execute(self) -> bool:
        server_cmd: str = f"test-system-rust LIBOS={super().libos()} TEST={self.test_name} ARGS=\\\"{self.server_args}\\\""
        client_cmd: str = f"test-system-rust LIBOS={super().libos()} TEST={self.test_name} ARGS=\\\"{self.client_args}\\\""
        return super().execute(server_cmd, client_cmd)


class PipeOpenTest(SystemTestJobOnLinux):
    def __init__(self, config: dict, niterations: int):
        config["test_name"] = "pipe-open"
        config["test_alias"] = "pipe-open"
        config["all_pass"] = True
        pipe_name: str = "demikernel-test-pipe-open"
        config["server_args"] = f"--peer server --pipe-name {pipe_name} --niterations {niterations}"
        config["client_args"] = f"--peer client --pipe-name {pipe_name} --niterations {niterations}"
        super().__init__(config)


class PipePingPongTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "pipe-ping-pong"
        config["test_alias"] = "pipe-ping-pong"
        config["all_pass"] = True
        pipe_name: str = "demikernel-test-pipe-ping-pong"
        config["server_args"] = f"--server {pipe_name}"
        config["client_args"] = f"--client {pipe_name}"
        super().__init__(config)


class PipePushPopTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "pipe-push-pop"
        config["test_alias"] = "pipe-push-pop"
        config["all_pass"] = True
        pipe_name: str = "demikernel-test-pipe-push-pop"
        config["server_args"] = f"--server {pipe_name}"
        config["client_args"] = f"--client {pipe_name}"
        super().__init__(config)


class TcpCloseTest(SystemTestJobOnLinux):
    def __init__(self, config: dict, run_mode: str, who_closes: str, nclients: int):
        config["test_name"] = "tcp-close"
        config["test_alias"] = f"tcp-close-{run_mode}-{who_closes}-closes-sockets"
        config["all_pass"] = True
        config["server_args"] = f"--peer server --address {config['server_addr']}:12345 --nclients {nclients} --run-mode {run_mode} --whocloses {who_closes}"
        config["client_args"] = f"--peer client --address {config['server_addr']}:12345 --nclients {nclients} --run-mode {run_mode} --whocloses {who_closes}"
        super().__init__(config)


class TcpEchoTest(SystemTestJobOnLinux):
    def __init__(self, config: dict, run_mode: str, nclients: int, bufsize: int, nrequests: int, nthreads: int):
        config["test_name"] = "tcp-echo"
        config["test_alias"] = f"tcp-echo-{run_mode}-{nclients}-{bufsize}-{nrequests}-{nthreads}"
        config["all_pass"] = True
        config["server_args"] = f"--peer server --address {config['server_addr']}:12345 --nthreads {nthreads}"
        config["client_args"] = f"--peer client --address {config['server_addr']}:12345 --nclients {nclients} --nrequests {nrequests} --bufsize {bufsize} --run-mode {run_mode}"
        super().__init__(config)


class TcpPingPongTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "tcp-ping-pong"
        config["test_alias"] = "tcp-ping-pong"
        config["all_pass"] = True
        config["server_args"] = f"--server {config['server_addr']}:12345"
        config["client_args"] = f"--client {config['server_addr']}:12345"
        super().__init__(config)


class TcpPushPopTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "tcp-push-pop"
        config["test_alias"] = "tcp-push-pop"
        config["all_pass"] = True
        config["server_args"] = f"--server {config['server_addr']}:12345"
        config["client_args"] = f"--client {config['server_addr']}:12345"
        super().__init__(config)


class TcpWaitTest(SystemTestJobOnLinux):
    def __init__(self, config: dict, scenario: str, nclients: int):
        config["test_name"] = "tcp-wait"
        config["test_alias"] = f"tcp-wait-scenario-{scenario}"
        config["all_pass"] = True
        config["server_args"] = f"--peer server --address {config['server_addr']}:12345 --nclients {nclients} --scenario {scenario}"
        config["client_args"] = f"--peer client --address {config['server_addr']}:12345 --nclients {nclients} --scenario {scenario}"
        super().__init__(config)


class UdpPingPongTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "udp-ping-pong"
        config["test_alias"] = "udp-ping-pong"
        config["all_pass"] = False
        config["server_args"] = f"--server {config['server_addr']}:12345 {config['client_addr']}:23456"
        config["client_args"] = f"--client {config['client_addr']}:23456 {config['server_addr']}:12345"
        super().__init__(config)


class UdpPushPopTest(SystemTestJobOnLinux):
    def __init__(self, config: dict):
        config["test_name"] = "udp-push-pop"
        config["test_alias"] = "udp-push-pop"
        config["all_pass"] = True
        config["server_args"] = f"--server {config['server_addr']}:12345 {config['client_addr']}:23456"
        config["client_args"] = f"--client {config['client_addr']}:23456 {config['server_addr']}:12345"
        super().__init__(config)


class IntegrationTestJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict, name: str):
        super().__init__(config, name)

    def execute(self, server_cmd: str) -> bool:
        serverTask: RunOnLinux = RunOnLinux(
            super().server(), super().repository(), server_cmd, super().is_debug(), super().is_sudo(), super().config_path())
        return super().execute(serverTask)


class TcpIntegrationTestJobOnLinux(IntegrationTestJobOnLinux):

    def __init__(self, config: dict):
        config["all_pass"] = True
        super().__init__(config, "integration-test")
        self.server_args: str = f"--local-address {super().server_addr()}:12345 --remote-address {super().client_addr()}:23456"

    def execute(self) -> bool:
        server_cmd: str = f"test-integration-rust TEST_INTEGRATION=tcp-test LIBOS={super().libos()} ARGS=\\\"{self.server_args}\\\""
        return super().execute(server_cmd)


class PipeIntegrationTestJobOnLinux(BaseLinuxJob):

    def __init__(self, config: dict, run_mode: str):
        config["all_pass"] = True
        super().__init__(config, f"integration-test-{run_mode}")
        self.server_args: str = f"--pipe-name {super().server_addr()}:12345 --run-mode {run_mode} --peer server"
        self.client_args: str = f"--pipe-name {super().client_addr()}:12345 --run-mode {run_mode} --peer client"
        self.run_mode: str = run_mode

    def execute(self) -> bool:
        server_cmd: str = f"test-integration-rust TEST_INTEGRATION=pipe-test LIBOS={super().libos()} ARGS=\\\"{self.server_args}\\\""
        client_cmd: str = f"test-integration-rust TEST_INTEGRATION=pipe-test LIBOS={super().libos()} ARGS=\\\"{self.client_args}\\\""
        jobs: dict[str, subprocess.Popen[str]] = {}
        jobs[self.name + "-server-" +
             super().server()] = RunOnLinux(super().server(), super().repository(), server_cmd, super().is_debug(), super().is_sudo(), super().config_path()).execute()
        if self.run_mode != "standalone":
            time.sleep(super().delay())
            jobs[self.name + "-client-" + super().client()] = RunOnLinux(
                super().client(), super().repository(), client_cmd, super().is_debug(), super().is_sudo(), super().config_path()).execute()
        return wait_and_report(self.name, super().log_directory(), jobs, True)


def job_test_system_rust(
        test_alias: str, test_name: str, repo: str, libos: str, is_debug: bool, server: str, client: str,
        server_args: str, client_args: str, is_sudo: bool, all_pass: bool, delay: float, config_path: str,
        log_directory: str) -> bool:
    config: dict = {
        "test_alias": test_alias,
        "test_name": test_name,
        "repository": repo,
        "libos": libos,
        "is_debug": is_debug,
        "server": server,
        "client": client,
        "server_args": server_args,
        "client_args": client_args,
        "is_sudo": is_sudo,
        "all_pass": all_pass,
        "delay": delay,
        "config_path": config_path,
        "log_directory": log_directory
    }
    job: SystemTestJobOnLinux = SystemTestJobOnLinux(config)
    return job.execute()


class InstallJobOnLinux(BaseLinuxJob):

    def __init__(self, config: dict):
        name = "install-{}".format("debug" if config["is_debug"] else "release")
        super().__init__(config, name)

    def execute(self) -> bool:
        cmd: str = f"LIBOS={super().libos()} INSTALL_PREFIX={super().install_prefix()} install"
        serverTask: linux.CompileOnLinux = linux.CompileOnLinux(
            super().server(), super().repository(), cmd, super().is_debug())

        if not super().enable_nfs():
            clientTask: linux.CompileOnLinux = linux.CompileOnLinux(
                super().client(), super().repository(), cmd, super().is_debug())
            return super().execute(serverTask, clientTask)
        return super().execute(serverTask)


class CloneRedisJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "clone-redis")

    def execute(self) -> bool:
        serverTask: linux.CloneOnLinux = linux.CloneOnLinux(
            super().server(), super().path(), super().repository(), super().branch())
        clientTask: linux.CloneOnLinux = linux.CloneOnLinux(
            super().client(), super().path(), super().repository(), super().branch())
        return super().execute(serverTask, clientTask)


class MakeRedisJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "make-redis")

    def execute(self) -> bool:
        serverTask: linux.MakeRedisOnLinux = linux.MakeRedisOnLinux(super().server(), super().path())
        clientTask: linux.MakeRedisOnLinux = linux.MakeRedisOnLinux(super().client(), super().path())
        return super().execute(serverTask, clientTask)


class RunRedisServerJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "run-redis-server")

    def execute(self) -> bool:
        serverTask: linux.RunredisServerOnLinux = linux.RunredisServerOnLinux(
            host=super().server(), redis_path=f"{super().path()}/redis",
            env=f"CONFIG_PATH={super().config_path()} LD_LIBRARY_PATH={super().ld_library_path()} LD_PRELOAD={super().libshim_path()} LIBOS={super().libos()}",
            params=f"--bind {super().server_addr()} --protected-mode no --save \\\"\\\" ")

        passed: bool = super().execute(serverTask, no_wait=True)

        # Give some time to Redis server to start.
        time.sleep(super().delay())

        return passed


class RunRedisBenchmarkJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "run-redis-benchmark")

    def execute(self) -> bool:
        clientTask: linux.RunRedisBenchmarkOnLinux = linux.RunRedisBenchmarkOnLinux(
            super().client(), f"{super().path()}/redis",
            f"-h {super().server_addr()} -d 64 -t set,get -c 1 -n 100")
        return super().execute(clientTask)


class StopRedisServerJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "stop-redis-server")

    def execute(self) -> bool:
        clientTask: linux.StopRedisServerOnLinux = linux.StopRedisServerOnLinux(
            super().client(), f"{super().path()}/redis",
            f"-h {super().server_addr()}")
        return super().execute(clientTask)


class CleanupRedisJobOnLinux(BaseLinuxJob):
    def __init__(self, config: dict):
        super().__init__(config, "cleanup-redis")

    def execute(self) -> bool:
        serverTask: linux.CleanupRedisOnLinux = linux.CleanupRedisOnLinux(
            super().server(), "redis-server", f"{super().path()}/redis")
        clientTask: linux.CleanupRedisOnLinux = linux.CleanupRedisOnLinux(
            super().client(), "redis-benchmark", f"{super().path()}/redis")
        return super().execute(serverTask, clientTask)
