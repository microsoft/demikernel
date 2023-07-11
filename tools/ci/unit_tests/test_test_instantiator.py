# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import pytest
import yaml
from ci_map import CIMap
from test_impls.pipe_open_test import PipeOpenTest

from test_impls.pipe_ping_pong import PipePingPongTest
from test_impls.pipe_push_pop import PipePushPopTest
from test_impls.tcp_close_test import TcpCloseTest
from test_impls.tcp_echo_test import TcpEchoTest
from test_impls.tcp_ping_pong_test import TcpPingPongTest
from test_impls.tcp_push_pop_test import TcpPushPopTest
from test_impls.tcp_wait_test import TcpWaitTest
from test_impls.udp_ping_pong_test import UdpPingPongTest
from test_impls.udp_push_pop_test import UdpPushPopTest
from test_instantiator import TestInstantiator


class TestTestInstantiator:
    @pytest.fixture()
    def scaffolding(self):
        path = "ci/unit_tests/test_data/scaffolding.yaml"
        yaml_str = ""
        with open(path, "r") as f:
            yaml_str = f.read()
        yield yaml.safe_load(yaml_str)

    @pytest.fixture()
    def ci_map(self):
        path = "ci/unit_tests/test_data/ci_map.yaml"
        yaml_str = ""
        with open(path, "r") as f:
            yaml_str = f.read()
        yield CIMap(yaml_str)

    @pytest.mark.parametrize("test_name, expected_test_type", [
        ("tcp_close", "TcpCloseTest"),
        ("tcp_echo", "TcpEchoTest"),
        ("tcp_ping_pong", "TcpPingPongTest"),
        ("tcp_push_pop", "TcpPushPopTest"),
        ("tcp_wait", "TcpWaitTest"),
        ("udp_ping_pong", "UdpPingPongTest"),
        ("udp_push_pop", "UdpPushPopTest"),
        ("pipe_open", "PipeOpenTest"),
        ("pipe_ping_pong", "PipePingPongTest"),
        ("pipe_push_pop", "PipePushPopTest"),
    ])
    def test_get_test_instance_creates_correct_test_type(self, scaffolding, ci_map, test_name, expected_test_type):
        # This is typically set in the input yaml file. But forcing it here so
        # that we don't need a new file because all the other details are same.
        if test_name.startswith("pipe_"):
            scaffolding["libos"] = "catmem"
        td = ci_map.get_test_details(scaffolding["libos"], test_name)
        ti = TestInstantiator(test_name, scaffolding, td)
        t = ti.get_test_instance(callback=None)
        actual_test_type = t.__class__.__name__
        assert expected_test_type == actual_test_type
