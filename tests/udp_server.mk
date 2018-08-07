CC = g++

ZEUS_SRC_DIR=/home/jingliu/datacenter-OS/

LIBZEUS=zeus_lwip

ZEUS_CFLAGS := -I$(ZEUS_SRC_DIR)
ZEUS_LDFLAGS := -L$(ZEUS_SRC_DIR) -l$(LIBZEUS)
DPDK_LDFLAGS := $(ZEUS_LDFLAGS)
DPDK_LDFLAGS += -L$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/lib

DPDK_HOME=$(ZEUS_SRC_DIR)/libos/libmtcp/mtcp/dpdk/
DPDK_INC=$(DPDK_HOME)/include
DPDK_LIB=$(DPDK_HOME)/lib/
DPDK_MACHINE_FLAGS = $(shell cat /home/jingliu/datacenter-OS/libos/libmtcp/mtcp/dpdk/include/cflags.txt)
DPDK_LIB_FLAGS = $(shell cat /home/jingliu/datacenter-OS/libos/libmtcp/mtcp/dpdk/lib/ldflags.txt) -lgmp

FINAL_CFLAGS=$(STD) $(WARN) $(OPT) $(DEBUG) $(CFLAGS)
FINAL_LDFLAGS=$(LDFLAGS) $(DEBUG)
FINAL_LIBS=-lm
DEBUG=-g -ggdb

ZEUS_LIBS := -l$(LIBZEUS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_CFLAGS += $(ZEUS_CFLAGS)
FINAL_CFLAGS += $(DPDK_MACHINE_FLAGS) -I$(DPDK_INC) -include $(DPDK_INC)/rte_config.h
#FINAL_LDFLAGS += $(ZEUS_LDFLAGS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LDFLAGS += $(DPDK_LDFLAGS) -lhoard -Wl,-rpath,$(ZEUS_SRC_DIR)
FINAL_LIBS += $(ZEUS_LIBS)
#FINAL_LIBS += -pthread -lrt -march=native -export-dynamic -lnuma -lpthread -lrt -ldl -lstdc++
FINAL_LIBS += -pthread -lrt -march=native -export-dynamic -L$(DPDK_HOME)/lib -lnuma -lpthread -lrt -ldl $(DPDK_LIB_FLAGS) -lstdc++

CFLAGS_CXX=-std=c++0x

all:
	${CC} -o udp_server udp_server.cc ${FINAL_CFLAGS} ${CFLAGS_CXX} ${FINAL_LDFLAGS} ${FINAL_LIBS}
