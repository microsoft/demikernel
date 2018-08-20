CC = g++

ZEUS_SRC_DIR=$(HOME)/datacenter-OS/

ZEUS_CFLAGS := -I$(ZEUS_SRC_DIR)
ZEUS_LDFLAGS := -L$(ZEUS_SRC_DIR)
DPDK_LDFLAGS := $(ZEUS_LDFLAGS)
DPDK_LDFLAGS += -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/lib

DPDK_HOME=$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/
DPDK_INC=$(DPDK_HOME)/include
DPDK_LIB=$(DPDK_HOME)/lib/
DPDK_MACHINE_FLAGS = $(shell cat $(HOME)/datacenter-OS/libos/libmtcp/mtcp/dpdk/include/cflags.txt)
DPDK_LIB_FLAGS = $(shell cat $(HOME)/datacenter-OS/libos/libmtcp/mtcp/dpdk/lib/ldflags.txt) -lgmp

FINAL_CFLAGS=$(STD) $(WARN) $(OPT) $(CFLAGS) -O3 -DNDEBUG
FINAL_LDFLAGS=$(LDFLAGS)
FINAL_LIBS=-lm

ZEUS_LIBS := -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_CFLAGS += $(ZEUS_CFLAGS)
FINAL_CFLAGS += $(DPDK_MACHINE_FLAGS) -I$(DPDK_INC) -include $(DPDK_INC)/rte_config.h
FINAL_LDFLAGS += $(DPDK_LDFLAGS) -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LIBS += $(ZEUS_LIBS)
FINAL_LIBS += -pthread -lrt -march=native -export-dynamic -L$(DPDK_HOME)/lib -lnuma -lpthread -lrt -ldl $(DPDK_LIB_FLAGS) -lstdc++

CFLAGS_CXX=-std=c++0x

all:
	${CC} -o udp_client dpdk_common.cpp dpdk_udp_echo_client.cpp ../libos/common/latency.cc ../libos/common/message.cc ../libos/common/time_resolution.cc ${FINAL_CFLAGS} ${CFLAGS_CXX} ${FINAL_LDFLAGS} ${FINAL_LIBS}