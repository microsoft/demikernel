C = g++

ZEUS_SRC_DIR=/users/ajaustin/datacenter-OS/
JING_SRC_DIR=/users/jingliu/datacenter-OS/

LIBZEUS=zeus_mtcp

ZEUS_CFLAGS := -I$(ZEUS_SRC_DIR)
ZEUS_LDFLAGS := -L$(ZEUS_SRC_DIR) -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/lib -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/mtcp/lib -l$(LIBZEUS) -lmtcp -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)

DPDK_HOME=$(JING_SRC_DIR)/libos/libmtcp/mtcp/dpdk/
DPDK_INC=$(DPDK_HOME)/include
DPDK_LIB=$(DPDK_HOME)/lib/
DPDK_MACHINE_FLAGS = $(shell cat /users/jingliu/datacenter-OS/libos/libmtcp/mtcp/dpdk/include/cflags.txt)
DPDK_LIB_FLAGS = $(shell cat /users/jingliu/datacenter-OS/libos/libmtcp/mtcp/dpdk/lib/ldflags.txt) -lgmp

# MTCP
MTCP_FLD    =/users/jingliu/datacenter-OS/libos/libmtcp/mtcp/mtcp/
MTCP_INC    =-I$(MTCP_FLD)/include
MTCP_LIB    =-L$(MTCP_FLD)/lib
MTCP_TARGET = $(MTCP_LIB)/lib/libmtcp.a

FINAL_CFLAGS=$(STD) $(WARN) $(OPT) $(DEBUG) $(CFLAGS)
FINAL_LDFLAGS=$(LDFLAGS) $(DEBUG)
FINAL_LIBS=-lm
DEBUG=-g -ggdb

ZEUS_LIBS := -l$(LIBZEUS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_CFLAGS += $(ZEUS_CFLAGS)
FINAL_CFLAGS += $(DPDK_MACHINE_FLAGS) -I$(DPDK_INC) -include $(DPDK_INC)/rte_config.h
FINAL_LDFLAGS += $(ZEUS_LDFLAGS)
FINAL_LIBS += $(ZEUS_LIBS)
FINAL_LIBS += -pthread -lrt -march=native -export-dynamic $(MTCP_FLD)/lib/libmtcp.a -L$(DPDK_HOME)/lib -lnuma -lmtcp -lpthread -lrt -ldl $(DPDK_LIB_FLAGS) -lstdc++

CFLAGS_CXX=-std=c++0x

all:
	${CC} -o tests/tcp_server tests/tcp_server.cc ${FINAL_CFLAGS} ${CFLAGS_CXX} ${FINAL_LDFLAGS} ${FINAL_LIBS}
