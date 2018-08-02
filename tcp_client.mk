C = g++

ZEUS_SRC_DIR=/users/ajaustin/datacenter-OS/
JING_SRC_DIR=/users/jingliu/datacenter-OS/

LIBZEUS=zeus_mtcp

ZEUS_CFLAGS := -I$(ZEUS_SRC_DIR)
ZEUS_LDFLAGS := -L$(ZEUS_SRC_DIR) -l$(LIBZEUS)
MTCP_LDFLAGS := $(ZEUS_LDFLAGS)
MTCP_LDFLAGS += -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/lib -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/mtcp/lib -lmtcp

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
#FINAL_LDFLAGS += $(ZEUS_LDFLAGS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LDFLAGS += $(MTCP_LDFLAGS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LIBS += $(ZEUS_LIBS)
#FINAL_LIBS += -pthread -lrt -march=native -export-dynamic -lnuma -lpthread -lrt -ldl -lstdc++
FINAL_LIBS += -pthread -lrt -march=native -export-dynamic $(MTCP_FLD)/lib/libmtcp.a -L$(DPDK_HOME)/lib -lnuma -lmtcp -lpthread -lrt -ldl $(DPDK_LIB_FLAGS) -lstdc++

CFLAGS_CXX=-std=c++0x

all:
	${CC} -o tests/tcp_client tests/tcp_client.cc ${FINAL_CFLAGS} ${CFLAGS_CXX} ${FINAL_LDFLAGS} ${FINAL_LIBS}
