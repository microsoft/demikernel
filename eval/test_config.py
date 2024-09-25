import os
import pandas as pd
import numpy as np
import itertools

################## PATHS #####################
HOME = os.path.expanduser("~")
LOCAL = HOME.replace("/homes", "/local")
AUTOKERNEL_PATH = f'{HOME}/Autokernel'
DATA_PATH = f'{HOME}/autokernel-data'

################## CLUSTER CONFIG #####################
ALL_NODES = ['node7', 'node9']
CLIENT_NODE = 'node7'
SERVER_NODE = 'node9'
NODE9_IP = '10.0.1.9'
NODE9_MAC = '08:c0:eb:b6:c5:ad'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    'tcp-migration',
    # 'manual-tcp-migration',
    'capy-log',
    # 'capy-profile',
    # 'capy-time-log',
    # 'server-reply-analysis',
]

################## TEST CONFIG #####################
SERVER_APP = 'tcp-echo' 
CLIENT_APP = 'tcp_generator'
REPEAT_NUM = 1


################## VARS #####################
### CLIENT ###
CLIENT_PPS = [i for i in range(90000, 90000 + 1, 30000)]#[i for i in range(100000, 1_300_001, 100000)]
NUM_CONNECTIONS = [32]
RUNTIME = 5

### SERVER ###
# CAPY_LOG = 'all' # 'all', 'mig'
RECEIVE_BATCH_SIZE = [4, 4 * 8, 4 * 32, 4 * 64] # Default: 4
TIMER_RESOLUTION = [64, 64 * 64, 64 * 1024] # 64
TIMER_FINER_RESOLUTION = [2, 2 * 32, 2 * 128] # 2
RTO_ALPHA = [0.125, 0.125 * 2, 0.125 * 4, 0.125 * 8] # 0.125
RTO_BETA = [0.25, 0.25 * 2, 0.25 * 4] # 0.25 
RTO_GRANULARITY = [0.001, 0.001 * 16, 0.001 * 128, 0.001 * 512] # 0.001f64
RTO_LOWER_BOUND_SEC = [0.1, 2, 1024] # 0.100f64
RTO_UPPER_BOUND_SEC = [60, 1, 2048] # 60.0f64
DEFAULT_BODY_POOL_SIZE = [8192 - 1, 65536 - 1] # 8192 - 1
DEFAULT_CACHE_SIZE = [250, 250*2, 250*4] # 250





#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean

# Commands:
# $ python3 -u run_eval.py 2>&1 | tee -a /homes/inho/capybara-data/experiment_history.txt
# $ python3 -u run_eval.py build
