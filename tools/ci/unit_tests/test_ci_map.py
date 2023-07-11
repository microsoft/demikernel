# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import pytest

from ci.src.ci_map import CIMap


class TestCIMap:
    @pytest.fixture()
    def ci_map(self):
        yaml_str = ""
        path = "ci/unit_tests/test_data/ci_map.yaml"
        with open(path, "r") as f:
            yaml_str = f.read()
        yield CIMap(yaml_str)

    @pytest.mark.parametrize("yaml_str", [
        None,
        "",
    ])
    def test_constructor_with_invalid_yaml_string_raises_exception(self, yaml_str):
        with pytest.raises(Exception):
            _ = CIMap(yaml_str)

    @pytest.mark.parametrize("libos_name, test_name", [
        ("", ""),
        (None, ""),
        ("", None),
        (None, None),
        ("catnap", ""),
        ("catnap", None),
        ("", "tcp_wait"),
        (None, "tcp_wait"),
        ("libos_non_existent", "tcp_wait"),
        ("catnip", "test_non_existent"),
        ("libos_non_existent", "test_non_existent"),
    ])
    def test_get_test_details_with_non_existent_query_raises_exception(self, libos_name, test_name):
        yaml_str = """
            catnip:
              tcp_ping_pong: {}
              tcp_push_pop: {}
              udp_ping_pong: {}
              udp_push_pop: {}
        """
        ci_map = CIMap(yaml_str)
        with pytest.raises(KeyError):
            _ = ci_map.get_test_details(libos_name, test_name)

    def test_get_test_details_with_valid_query_returns_correct_dictionary(self, ci_map):
        # simple case
        expected = {}
        actual = ci_map.get_test_details("catmem", "pipe_open")
        assert expected == actual

        # nested case
        expected = {
            "scenario0": {
                "nclients": 32,
                "scenario": "push_close_wait"
            },
            "scenario1": {
                "nclients": 32,
                "scenario": "push_async_close_wait"
            },
            "scenario2": {
                "nclients": 32,
                "scenario": "push_async_close_pending_wait"
            },
            "scenario3": {
                "nclients": 32,
                "scenario": "pop_close_wait"
            },
            "scenario4": {
                "nclients": 32,
                "scenario": "pop_async_close_wait"
            },
            "scenario5": {
                "nclients": 32,
                "scenario": "pop_async_close_pending_wait"
            },
        }
        actual = ci_map.get_test_details("catnap", "tcp_wait")
        assert expected == actual

    def test_get_test_details_with_all_test_name_returns_correct_dictionary(self, ci_map):
        expected = {
            "pipe_open": {},
            "pipe_ping_pong": {},
            "pipe_push_pop": {},
        }
        actual = ci_map.get_test_details("catmem", "all")
        assert expected == actual
