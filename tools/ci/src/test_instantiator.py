# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.


from ci.src.test_impls.pipe_open_test import PipeOpenTest
from ci.src.test_impls.pipe_ping_pong import PipePingPongTest
from ci.src.test_impls.pipe_push_pop import PipePushPopTest
from ci.src.test_impls.tcp_close_test import TcpCloseTest
from ci.src.test_impls.tcp_echo_test import TcpEchoTest
from ci.src.test_impls.tcp_ping_pong_test import TcpPingPongTest
from ci.src.test_impls.tcp_push_pop_test import TcpPushPopTest
from ci.src.test_impls.tcp_wait_test import TcpWaitTest
from ci.src.test_impls.udp_ping_pong_test import UdpPingPongTest
from ci.src.test_impls.udp_push_pop_test import UdpPushPopTest


class TestInstantiator:
    test_name_to_type_name_map = {
        "tcp_close": TcpCloseTest,
        "tcp_echo": TcpEchoTest,
        "tcp_ping_pong": TcpPingPongTest,
        "tcp_push_pop": TcpPushPopTest,
        "tcp_wait": TcpWaitTest,
        "udp_ping_pong": UdpPingPongTest,
        "udp_push_pop": UdpPushPopTest,
        "pipe_open": PipeOpenTest,
        "pipe_ping_pong": PipePingPongTest,
        "pipe_push_pop": PipePushPopTest,
    }

    def __init__(self, test_class_name, scaffolding, test_details):
        self.test_name = test_class_name
        self.scaffolding = scaffolding
        self.test_details = test_details

    def get_test_instance(self, callback):
        return self.test_name_to_type_name_map[self.test_name](self.scaffolding, self.test_details, callback)
