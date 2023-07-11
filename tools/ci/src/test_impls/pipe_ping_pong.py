# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.src.base_test import BaseTest


class PipePingPongTest(BaseTest):
    test_name = "pipe-ping-pong"

    def execute(self):
        self.run_scenario()
        return self.has_test_passed

    def run_scenario(self):
        test_alias = "pipe-ping-pong"
        pipe_name: str = "demikernel-test-pipe-ping-pong"
        server_args: str = "--server demikernel-test-pipe-ping-pong".format(pipe_name)
        client_args: str = "--client demikernel-test-pipe-ping-pong".format(pipe_name)
        s = self.scaffolding
        self.has_test_passed = self.job_test_system_rust(
            test_alias, self.test_name, s["repository"], s["libos"], s["is_debug"], s["server_name"],
            s["client_name"], server_args, client_args, s["is_sudo"], True, s["delay"], s["config_path"],
            s["log_directory"])
