CC = g++

ZEUS_SRC_DIR=$(HOME)/datacenter-OS/

MTCP_LDFLAGS := -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/lib -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/mtcp/lib -lmtcp

DPDK_HOME=$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/
DPDK_INC=$(DPDK_HOME)/include
DPDK_LIB=$(DPDK_HOME)/lib/
DPDK_MACHINE_FLAGS = $(shell cat $(HOME)/datacenter-OS/libos/libmtcp/mtcp/dpdk/include/cflags.txt)
DPDK_LIB_FLAGS = $(shell cat $(HOME)/datacenter-OS/libos/libmtcp/mtcp/dpdk/lib/ldflags.txt) -lgmp

# MTCP
MTCP_FLD    =$(HOME)/datacenter-OS/libos/libmtcp/mtcp/mtcp/
MTCP_INC    =$(MTCP_FLD)/include
MTCP_LIB    =-L$(MTCP_FLD)/lib
MTCP_TARGET = $(MTCP_LIB)/lib/libmtcp.a

FINAL_CFLAGS=$(STD) $(WARN) $(OPT) $(CFLAGS) -O3 -DNNDEBUG
FINAL_LDFLAGS=$(LDFLAGS)
FINAL_LIBS=-lm

ZEUS_LIBS := -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_CFLAGS += $(DPDK_MACHINE_FLAGS) -I$(DPDK_INC) -include $(DPDK_INC)/rte_config.h -I$(MTCP_INC) -include $(MTCP_INC)/mtcp_api.h
FINAL_LDFLAGS += $(MTCP_LDFLAGS) -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LIBS += -pthread -lrt -march=native -export-dynamic $(MTCP_FLD)/lib/libmtcp.a -L$(DPDK_HOME)/lib -lnuma -lmtcp -lpthread -lrt -ldl $(DPDK_LIB_FLAGS) -lstdc++

CFLAGS_CXX=-std=c++0x

all:
	${CC} -o tcp_server mtcp_echo_server.cpp mtcp_common.cpp ../libos/common/latency.cc ../libos/common/message.cc ../libos/common/time_resolution.cc ${FINAL_CFLAGS} ${CFLAGS_CXX} ${FINAL_LDFLAGS} ${FINAL_LIBS}