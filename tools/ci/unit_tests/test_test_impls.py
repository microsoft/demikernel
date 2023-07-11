# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

import pytest
import yaml
from ci_map import CIMap
from test_callback_example import dummy_job_test_system_rust
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


class TestTestImpls:
    @pytest.fixture()
    def ci_map(self):
        path = "ci/unit_tests/test_data/ci_map.yaml"
        yaml_str = ""
        with open(path, "r") as f:
            yaml_str = f.read()
        yield CIMap(yaml_str)

    @pytest.fixture()
    def scaffolding(self):
        path = "ci/unit_tests/test_data/scaffolding.yaml"
        yaml_str = ""
        with open(path, "r") as f:
            yaml_str = f.read()
        scaffolding = yaml.safe_load(yaml_str)
        self.add_hostnames(scaffolding)
        yield scaffolding

    @classmethod
    def add_hostnames(self, scaffolding):
        scaffolding["server_name"] = "server_fqdn"
        scaffolding["client_name"] = "client_fqdn"

    def test_tcp_close_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "tcp_close")
        t = TcpCloseTest(scaffolding, test_details=td, callback=dummy_job_test_system_rust)
        assert True == t.execute()

    def test_udp_ping_pong_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "udp_ping_pong")
        t = UdpPingPongTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_udp_push_pop_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "udp_push_pop")
        test = UdpPushPopTest(scaffolding, test_details=td, callback=None)
        assert True == test.execute()

    def test_tcp_ping_pong_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "tcp_ping_pong")
        t = TcpPingPongTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_tcp_push_pop_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "tcp_push_pop")
        t = TcpPushPopTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_tcp_echo_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "tcp_echo")
        t = TcpEchoTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_tcp_wait_test(self, ci_map, scaffolding):
        td = ci_map.get_test_details(scaffolding["libos"], "tcp_wait")
        t = TcpWaitTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_pipe_open_test(self, ci_map, scaffolding):
        # This is typically set in the input yaml file. But forcing it here so
        # that we don't need a new file because all the other details are same.
        scaffolding["libos"] = "catmem"
        td = ci_map.get_test_details(scaffolding["libos"], "pipe_open")
        t = PipeOpenTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_pipe_ping_pong_test(self, ci_map, scaffolding):
        # This is typically set in the input yaml file. But forcing it here so
        # that we don't need a new file because all the other details are same.
        scaffolding["libos"] = "catmem"
        td = ci_map.get_test_details(scaffolding["libos"], "pipe_ping_pong")
        t = PipePingPongTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()

    def test_pipe_push_pop_test(self, ci_map, scaffolding):
        # This is typically set in the input yaml file. But forcing it here so
        # that we don't need a new file because all the other details are same.
        scaffolding["libos"] = "catmem"
        td = ci_map.get_test_details(scaffolding["libos"], "pipe_push_pop")
        t = PipePushPopTest(scaffolding, test_details=td, callback=None)
        assert True == t.execute()
