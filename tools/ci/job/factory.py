
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.job.utils import wait_and_report
import ci.job.windows as windows
import ci.job.linux as linux
from ci.job.generic import BaseJob

# ======================================================================================================================
# Generic Jobs
# ======================================================================================================================


class JobFactory:

    def __init__(self, config: dict):
        self.config = config

    def checkout(self) -> BaseJob:
        if self.config["platform"] == "windows":
            return windows.CheckoutJobOnWindows(self.config)
        else:
            return linux.CheckoutJobOnLinux(self.config)

    def compile(self) -> BaseJob:
        if self.config["platform"] == "windows":
            return windows.CompileJobOnWindows(self.config)
        else:
            return linux.CompileJobOnLinux(self.config)

    def install(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Install is not supported on Windows")
        else:
            return linux.InstallJobOnLinux(self.config)

    def cleanup(self) -> BaseJob:
        if self.config["platform"] == "windows":
            return windows.CleanupJobOnWindows(self.config)
        else:
            return linux.CleanupJobOnLinux(self.config)

    def unit_test(self, test_name: str) -> BaseJob:
        if self.config["platform"] == "windows":
            if test_name == "test-unit-rust":
                return windows.UnitTestRustJobOnWindows(self.config)
            elif test_name == "test-unit-c":
                return windows.UnitTestCJobOnWindows(self.config)
            else:
                raise Exception("Invalid test name")
        else:
            if test_name == "test-unit-rust":
                return linux.UnitTestRustJobOnLinux(self.config)
            elif test_name == "test-unit-c":
                return linux.UnitTestCJobOnLinux(self.config)
            else:
                raise Exception("Invalid test name")

    def integration_test(self, run_mode="") -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Integration tests are not supported on Windows")
        else:
            if self.config["libos"] == "catmem":
                return linux.PipeIntegrationTestJobOnLinux(self.config, run_mode=run_mode)
            else:
                return linux.TcpIntegrationTestJobOnLinux(self.config)

    def system_test(self, test_name: str, niterations: int = 0, run_mode: str = "", nclients: int = 0, bufsize: int = 0, nrequests: int = 0, nthreads: int = 1, who_closes: str = "", scenario: str = "") -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("System tests are not supported on Windows")
        else:
            if self.config["libos"] == "catmem":
                if test_name == "open-close":
                    return linux.PipeOpenTest(self.config, niterations)
                elif test_name == "ping-pong":
                    return linux.PipePingPongTest(self.config)
                elif test_name == "push-pop":
                    return linux.PipePushPopTest(self.config)
                else:
                    raise Exception("Invalid test name")
            else:
                if test_name == "tcp_echo":
                    return linux.TcpEchoTest(self.config, run_mode, nclients, bufsize, nrequests, nthreads)
                elif test_name == "tcp_close":
                    return linux.TcpCloseTest(self.config, run_mode=run_mode, who_closes=who_closes, nclients=nclients)
                elif test_name == "tcp_wait":
                    return linux.TcpWaitTest(self.config, scenario=scenario, nclients=nclients)
                elif test_name == "tcp_ping_pong":
                    return linux.TcpPingPongTest(self.config)
                elif test_name == "tcp_push_pop":
                    return linux.TcpPushPopTest(self.config)
                elif test_name == "udp_ping_pong":
                    return linux.UdpPingPongTest(self.config)
                elif test_name == "udp_push_pop":
                    return linux.UdpPushPopTest(self.config)
                else:
                    raise Exception("Invalid test name")

    def clone_redis(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Clone is not supported on Windows")
        else:
            return linux.CloneRedisJobOnLinux(self.config)

    def make_redis(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Make Redis is not supported on Windows")
        else:
            return linux.MakeRedisJobOnLinux(self.config)

    def run_redis_server(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Run Redis is not supported on Windows")
        else:
            return linux.RunRedisServerJobOnLinux(self.config)

    def run_redis_benchmark(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Run Redis is not supported on Windows")
        else:
            return linux.RunRedisBenchmarkJobOnLinux(self.config)

    def stop_redis_server(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Stop Redis is not supported on Windows")
        else:
            return linux.StopRedisServerJobOnLinux(self.config)

    def cleanup_redis(self) -> BaseJob:
        if self.config["platform"] == "windows":
            raise Exception("Cleanup Redis is not supported on Windows")
        else:
            return linux.CleanupRedisJobOnLinux(self.config)
