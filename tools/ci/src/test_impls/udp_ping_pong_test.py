# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.src.base_test import BaseTest


class UdpPingPongTest(BaseTest):
    test_name = "udp-ping-pong"

    def execute(self):
        self.run_scenario()
        return self.has_test_passed

    def run_scenario(self):
        test_alias = "udp-ping-pong"
        s = self.scaffolding
        server_args = f"--server {s['server_ip']}:12345 {s['client_ip']}:23456"
        client_args = f"--client {s['client_ip']}:23456 {s['server_ip']}:12345"
        self.has_test_passed = self.job_test_system_rust(
            test_alias, self.test_name, s["repository"], s["libos"], s["is_debug"], s["server_name"], s["client_name"],
            server_args, client_args, s["is_sudo"], False, s["delay"], s["config_path"], s["log_directory"])
