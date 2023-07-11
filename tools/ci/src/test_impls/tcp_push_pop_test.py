# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.src.base_test import BaseTest


class TcpPushPopTest(BaseTest):
    test_name = "tcp-push-pop"

    def execute(self):
        self.run_scenario()
        return self.has_test_passed

    def run_scenario(self):
        test_alias = "tcp-push-pop"
        s = self.scaffolding
        server_args = f"--server {s['server_ip']}:12345"
        client_args = f"--client {s['server_ip']}:12345" # This is deliberately the server_ip
        self.has_test_passed = self.job_test_system_rust(
            test_alias, self.test_name, s["repository"], s["libos"], s["is_debug"], s["server_name"],
            s["client_name"], server_args, client_args, s["is_sudo"], True, s["delay"], s["config_path"],
            s["log_directory"])
