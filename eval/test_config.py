import os
import pandas as pd
import numpy as np

################## PATHS #####################
HOME = os.path.expanduser("~")
CUR_PATH = os.getcwd()
AUTOKERNEL_PATH = f'{CUR_PATH}'
CALADAN_PATH = f'{HOME}/Capybara/caladan'
DATA_PATH = f'{HOME}/autokernel-data' # location to store data

################## CLUSTER CONFIG #####################
ALL_NODES = ['node7', 'node8']
CLIENT_NODE = 'node7'
SERVER_NODE = 'node8'

SERVER_IP = '10.0.1.8'
SERVER_MAC = '08:c0:eb:b6:e8:05'
SERVER_PORT = '10008'
SERVER_CONFIG = f'{AUTOKERNEL_PATH}/scripts/config/node8_config.yaml'

################## BUILD CONFIG #####################
LIBOS = 'catnip'#'catnap', 'catnip'
FEATURES = [
    # 'capy-log',
]

################## TEST CONFIG #####################
SERVER_APP = 'http-server'
CLIENT_APP = 'caladan' 
REPEAT_NUM = 1


################## ENV VARS #####################
CAPY_LOG = 'no' # 'all', 'mig'
REDIS_LOG = 0


### CALADAN ###
CLIENT_PPS = [i for i in range(40000, 40000 + 1, 30000)]#[i for i in range(100000, 1_300_001, 100000)]
LOADSHIFTS = ''
# LOADSHIFTS = '90000:10000,270000:10000,450000:10000,630000:10000,810000:10000/90000:50000/90000:50000/90000:50000'
# LOADSHIFTS = ''#'10000:10000/10000:10000/10000:10000/10000:10000'
ZIPF_ALPHA = '' # 0.9
ONOFF = '0' # '0', '1'
NUM_CONNECTIONS = [100]
RUNTIME = 1

#####################
# build command: run_eval.py [build [clean]]
# builds based on SERVER_APP
# cleans redis before building if clean