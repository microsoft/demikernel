# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.src.base_test import BaseTest


class PipeOpenTest(BaseTest):
    test_name = "pipe-open"

    def execute(self):
        self.run_scenario()
        return self.has_test_passed

    def run_scenario(self):
        test_alias = "pipe-open"
        pipe_name: str = "demikernel-test-pipe-open"
        niterations: int = 128
        s = self.scaffolding
        server_args = f"--peer server --pipe-name {pipe_name} --niterations {niterations}"
        client_args = f"--peer client --pipe-name {pipe_name} --niterations {niterations}"
        self.has_test_passed = self.job_test_system_rust(
            test_alias, self.test_name, s["repository"], s["libos"], s["is_debug"], s["server_name"],
            s["client_name"], server_args, client_args, s["is_sudo"], True, s["delay"], s["config_path"],
            s["log_directory"])
