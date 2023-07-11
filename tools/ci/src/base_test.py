# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

from abc import ABCMeta, abstractmethod


class BaseTest(metaclass=ABCMeta):
    '''The base class for all tests.'''

    has_test_passed = False

    def __init__(self, scaffolding, test_details, callback):
        self.scaffolding = scaffolding
        self.test_details = test_details
        self.callback = callback

    @abstractmethod
    def execute(self):
        raise NotImplementedError

    def job_test_system_rust(self, test_alias, test_name, repo, libos, is_debug, server, client, server_args,
                             client_args, is_sudo, all_pass, delay, config_path, log_directory):
        if self.callback:
            return self.callback(test_alias, test_name, repo, libos, is_debug, server, client, server_args,
                                 client_args, is_sudo, all_pass, delay, config_path, log_directory)
        return True
