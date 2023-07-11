# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import yaml


class CIMap:
    '''The mapping of LibOS to tests and scenario tree.'''

    def __init__(self, ci_map_yaml_string):
        self.__validate_input(ci_map_yaml_string)
        self.test_map = yaml.safe_load(ci_map_yaml_string)

    def __validate_input(self, ci_map_yaml_string):
        if ci_map_yaml_string is None or ci_map_yaml_string == "":
            raise Exception("yaml_file is null or empty!")

    def get_test_details(self, libos_name, test_name):
        if test_name == "all":
            all_test_details = self.test_map[libos_name]
            return all_test_details
        else:
            specific_test_details = self.test_map[libos_name][test_name]
            return specific_test_details
