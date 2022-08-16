# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

all: all-tcp-proxy all-tcp-push-pop all-tcp-relay all-udp-push-pop

all-tcp-proxy:
	$(MAKE) -C tcp-proxy all

all-tcp-push-pop:
	$(MAKE) -C tcp-push-pop all

all-tcp-relay:
	$(MAKE) -C tcp-relay all

all-udp-push-pop:
	$(MAKE) -C udp-push-pop all

clean: clean-tcp-proxy clean-tcp-push-pop clean-tcp-relay clean-udp-push-pop

clean-tcp-proxy:
	$(MAKE) -C tcp-proxy clean

clean-tcp-push-pop:
	$(MAKE) -C tcp-push-pop clean

clean-tcp-relay:
	$(MAKE) -C tcp-relay clean

clean-udp-push-pop:
	$(MAKE) -C udp-push-pop clean
