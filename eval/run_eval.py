import argparse
import os
import time
import math
import operator
import pyrem.host
# import pyrem.task
# from pyrem.host import RemoteHost
# import pyrem
import sys
import glob

import numpy as np
import matplotlib.pyplot as plt
import matplotlib.ticker as tick
from math import factorial, exp
from datetime import datetime
import signal
import atexit
from os.path import exists
import toml

import subprocess
from cycler import cycler

import datetime
import pty


from test_config import *

final_result = ''


def kill_procs():
    cmd = [f'sudo pkill -INT -e iokerneld ; \
            sudo pkill -INT -f synthetic ; \
            sudo pkill -INT -e {SERVER_APP} ']
    
    # cmd = [f'sudo pkill -INT -f Capybara && sleep 2 && sudo pkill -f Capybara && sudo pkill -f caladan']
    kill_tasks = []
    for node in ALL_NODES:
        host = pyrem.host.RemoteHost(node)
        task = host.run(cmd, quiet=False)
        # print(task)
        kill_tasks.append(task)
    
    pyrem.task.Parallel(kill_tasks, aggregate=True).start(wait=True)
    print('KILLED CAPYBARA PROCESSES')


def run_server():
    global experiment_id
    
    print('RUNNING SERVER')
    host = pyrem.host.RemoteHost(SERVER_NODE)

    server_tasks = []
    run_cmd = f'{AUTOKERNEL_PATH}/bin/examples/rust/{SERVER_APP}.elf {SERVER_IP}:{SERVER_PORT}'
    
    if SERVER_APP == 'http-server' :
        cmd = [f'cd {AUTOKERNEL_PATH} && \
            sudo -E \
            CAPY_LOG={CAPY_LOG} \
            LIBOS={LIBOS} \
            MTU=1500 \
            MSS=1500 \
            RUST_BACKTRACE=full \
            {f"CONFIG_PATH={SERVER_CONFIG}"} \
            LD_LIBRARY_PATH={HOME}/lib:{HOME}/lib/x86_64-linux-gnu \
            PKG_CONFIG_PATH={HOME}/lib/x86_64-linux-gnu/pkgconfig \
            numactl -m0 \
            {run_cmd} \
            > {DATA_PATH}/{experiment_id}.server 2>&1']
    else:
        print(f'Invalid server app: {SERVER_APP}')
        exit(1)
    task = host.run(cmd, quiet=False)
    server_tasks.append(task)
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'server is running')


def run_eval():
    global experiment_id
    global final_result
    
    for repeat in range(0, REPEAT_NUM):
        for pps in CLIENT_PPS:
            for conn in NUM_CONNECTIONS:
                kill_procs()
                experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                
                with open(f'{AUTOKERNEL_PATH}/eval/test_config.py', 'r') as file:
                    print(f'================ RUNNING TEST =================')
                    print(f'\n\nEXPTID: {experiment_id}')
                    with open(f'{DATA_PATH}/{experiment_id}.test_config', 'w') as output_file:
                        output_file.write(file.read())
                
                run_server()

                #exit(0)
                host = pyrem.host.RemoteHost(CLIENT_NODE)
                
                cmd = [f'cd {CALADAN_PATH} && sudo ./iokerneld ias nicpci 0000:31:00.1']
                task = host.run(cmd, quiet=True)
                pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                time.sleep(3)
                print('iokerneld is running')
                
                if SERVER_APP == 'http-server' :   
                    cmd = [f'sudo numactl -m0 {CALADAN_PATH}/apps/synthetic/target/release/synthetic \
                        {SERVER_IP}:{SERVER_PORT} \
                        --config {CALADAN_PATH}/client.config \
                        --mode runtime-client \
                        --protocol=http \
                        --transport=tcp \
                        --samples=1 \
                        --pps={pps} \
                        --threads={conn} \
                        --runtime={RUNTIME} \
                        --discard_pct=10 \
                        --output=trace \
                        --rampup=0 \
                        {f"--loadshift={LOADSHIFTS}" if LOADSHIFTS != "" else ""} \
                        {f"--zipf={ZIPF_ALPHA}" if ZIPF_ALPHA != "" else ""} \
                        {f"--onoff={ONOFF}" if ONOFF == "1" else ""} \
                        --exptid={DATA_PATH}/{experiment_id} \
                        > {DATA_PATH}/{experiment_id}.client']
                
                else:
                    print(f'Invalid server app: {SERVER_APP}')
                    exit(1)
                task = host.run(cmd, quiet=False)
                pyrem.task.Parallel([task], aggregate=True).start(wait=True)

                print('================ TEST COMPLETE =================\n')
                try:
                    cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]" | tail -1'
                    result = subprocess.run(
                        cmd,
                        shell=True,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT,
                        check=True,
                    ).stdout.decode()
                    if result == '':
                        result = '[RESULT] N/A\n'
                    print('[RESULT]' + f'{experiment_id}, {conn}, {result[len("[RESULT]"):]}' + '\n\n')
                    final_result = final_result + f'{experiment_id}, {conn},{result[len("[RESULT]"):]}'
                except subprocess.CalledProcessError as e:
                    # Handle the exception for a failed command execution
                    print("EXPERIMENT FAILED\n\n")

                except Exception as e:
                    # Handle any other unexpected exceptions
                    print("EXPERIMENT FAILED\n\n")
                
                kill_procs()
                   


def exiting():
    global final_result
    print('EXITING')
    result_header = "ID, #CONN, Distribution, RPS, Target, Actual, Dropped, Never Sent, Median, 90th, 99th, 99.9th, 99.99th, Start, StartTsc"
    if CLIENT_APP == 'wrk':
        result_header = "ID, #BE, #CONN, #THREAD, AVG, p50, p75, p90, p99"
        
    print(f'\n\n\n\n\n{result_header}')
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write(f'{result_header}')
        file.write(final_result)
    kill_procs()


def run_compile():

    features = '--features=' if len(FEATURES) > 0 else ''
    for feat in FEATURES:
        features += feat + ','

    os.system(f"cd {AUTOKERNEL_PATH} && EXAMPLE_FEATURES={features} make LIBOS={LIBOS} all-examples-rust")

if __name__ == '__main__':

    if len(sys.argv) > 1 and sys.argv[1] == 'build':
        exit(run_compile())
    
    atexit.register(exiting)
    run_eval()
    kill_procs()