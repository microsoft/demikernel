# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from ci.src.base_test import BaseTest


class TcpEchoTest(BaseTest):
    test_name = "tcp-echo"

    def execute(self):
        for _, params in self.test_details.items():
            self.run_scenario(params)
        return self.has_test_passed

    def run_scenario(self, params):
        test_alias = self.__make_test_alias(params)
        server_args = self.__make_server_args()
        client_args = self.__make_client_args(params)
        s = self.scaffolding
        self.has_test_passed = self.job_test_system_rust(
            test_alias, self.test_name, s["repository"], s["libos"], s["is_debug"], s["server_name"],
            s["client_name"], server_args, client_args, s["is_sudo"], True, s["delay"], s["config_path"],
            s["log_directory"])

    def __make_test_alias(self, params):
        return f"tcp-echo-{params['run_mode']}-{params['nclients']}-{params['bufsize']}-{params['nrequests']}"

    def __make_server_args(self):
        return f"--peer server --address {self.scaffolding['server_ip']}:12345"

    def __make_client_args(self, params):
        return f"--peer client --address {self.scaffolding['server_ip']}:12345 --nclients {params['nclients']} " \
            f"--nrequests {params['nrequests']} --bufsize {params['bufsize']} --run-mode {params['run_mode']}"
