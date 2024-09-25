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

result_header = ''
final_result = ''


def kill_procs():
    cmd = [f'sudo pkill -INT -f tcp-generator ; \
            sudo pkill -INT -f tcp-echo']
    # print(cmd)
    
    # cmd = [f'sudo pkill -INT -f Capybara && sleep 2 && sudo pkill -f Capybara && sudo pkill -f caladan']
    
    # print(cmd)
    for node in ALL_NODES:
        # kill_tasks = []
        # print(node)
        host = pyrem.host.RemoteHost(node)
        task = host.run(cmd, quiet=False)
        # print(task)
        # kill_tasks.append(task)
        pyrem.task.Parallel([task], aggregate=True).start(wait=True)
    
    # print(kill_tasks)
    print('KILLED AUTOKERNEL PROCESSES')


def run_server(test_values):
    global experiment_id
    print('RUNNING SERVER')
    host = pyrem.host.RemoteHost(SERVER_NODE)
    
    # print(test_values)
    ENV = ''
    for param_name, value in test_values.items():
        ENV += f"{param_name}={value} "
    # print(ENV)
    
    server_tasks = []
    cmd = [f'cd {AUTOKERNEL_PATH}/demikernel && \
            {ENV} \
            make tcp-echo-server9 \
            > {DATA_PATH}/{experiment_id}.server 2>&1']
    # print(cmd)
    task = host.run(cmd, quiet=False)
    server_tasks.append(task)    
    pyrem.task.Parallel(server_tasks, aggregate=True).start(wait=False)    
    time.sleep(2)
    print(f'Server is running')
    
def run_eval():
    global experiment_id
    global result_header
    global final_result
    
    # Create a dictionary that stores each parameter and its values
    parameters = {
        "RECEIVE_BATCH_SIZE": RECEIVE_BATCH_SIZE,
        "TIMER_RESOLUTION": TIMER_RESOLUTION,
        "TIMER_FINER_RESOLUTION": TIMER_FINER_RESOLUTION,
        "RTO_ALPHA": RTO_ALPHA,
        "RTO_BETA": RTO_BETA,
        "RTO_GRANULARITY": RTO_GRANULARITY,
        "RTO_LOWER_BOUND_SEC": RTO_LOWER_BOUND_SEC,
        "RTO_UPPER_BOUND_SEC": RTO_UPPER_BOUND_SEC,
        "DEFAULT_BODY_POOL_SIZE": DEFAULT_BODY_POOL_SIZE,
        "DEFAULT_CACHE_SIZE": DEFAULT_CACHE_SIZE
    }

    # Define the default values (the first element from each array)
    default_values = {k: v[0] for k, v in parameters.items()}

    result_header = "EXPT_ID," + ",".join(parameters.keys())
    result_header += ',#flows,frame_size,#queues,Rate,Throughput,Median,p90,p95,p99,p99.9'
                
    for pps in CLIENT_PPS:
        for conn in NUM_CONNECTIONS:
            for param_name, values in parameters.items():
                for value in values:
                    # Create a copy of the default values
                    test_values = default_values.copy()
                    # Update the current parameter being tested
                    test_values[param_name] = value
                    # Run the test with the updated values
                    if test_values['RTO_UPPER_BOUND_SEC'] < test_values['RTO_LOWER_BOUND_SEC']:
                        continue
                    kill_procs()
                    experiment_id = datetime.datetime.now().strftime('%Y%m%d-%H%M%S.%f')
                    with open(f'{AUTOKERNEL_PATH}/demikernel/eval/test_config.py', 'r') as file:
                        print(f'================ RUNNING TEST =================')
                        print(f'\n\nEXPTID: {experiment_id}')
                        with open(f'{DATA_PATH}/{experiment_id}.test_config', 'w') as output_file:
                            output_file.write(file.read())
                    run_server(test_values)
                    
                    host = pyrem.host.RemoteHost(CLIENT_NODE)
                    cmd = [f'cd {AUTOKERNEL_PATH}/tcp_generator && \
                            sudo ./build/tcp-generator \
                            -a 31:00.1 \
                            -n 4 \
                            -c 0xffff -- \
                            -d exponential \
                            -r {pps} \
                            -f {conn} \
                            -s 128 \
                            -t {RUNTIME} \
                            -q 1 \
                            -c addr.cfg \
                            -o {DATA_PATH}/{experiment_id}.lat \
                            > {DATA_PATH}/{experiment_id}.client 2>&1']
                    task = host.run(cmd, quiet=False)
                    print('Running client\n')
                    pyrem.task.Parallel([task], aggregate=True).start(wait=False)
                    time.sleep(RUNTIME + 10)
                    print('================ TEST COMPLETE =================\n')
                    
                    try:
                        cmd = f'cat {DATA_PATH}/{experiment_id}.client | grep "\[RESULT\]" | tail -1'
                        client_result = subprocess.run(
                            cmd,
                            shell=True,
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT,
                            check=True,
                        ).stdout.decode()
                        if client_result == '':
                            client_result = '[RESULT] N/A\n'
                        
                        
                        # Generate the result string with the values
                        result = f'{experiment_id},' + ",".join(map(str, test_values.values()))
                        result += f',{client_result[len("[RESULT]"):]}'
                        
                        print('\n\n' + "***TEST RESULT***\n" + result_header + '\n' + result)
                        
                        final_result += result + '\n'
                        
                    except subprocess.CalledProcessError as e:
                        # Handle the exception for a failed command execution
                        print("EXPERIMENT FAILED\n\n")

                    except Exception as e:
                        # Handle any other unexpected exceptions
                        print("EXPERIMENT FAILED\n\n")
                                    
def exiting():
    
    global result_header
    global final_result
    
    print('EXITING')
   
    print(f'\n\n\n\n\n{result_header}')
    print(final_result)
    with open(f'{DATA_PATH}/result.txt', "w") as file:
        file.write(f'{result_header}')
        file.write(final_result)
    kill_procs()


def run_compile():
    # only for redis-server
    mig = ''
    if 'manual-tcp-migration' in FEATURES:
        mig = '-mig-manual'
    elif 'tcp-migration' in FEATURES:
        mig = '-mig'

    features = '--features=' if len(FEATURES) > 0 else ''
    for feat in FEATURES:
        features += feat + ','

    # if SERVER_APP == 'http-server':
    
    if SERVER_APP == 'redis-server':
        os.system(f"cd {CAPYBARA_PATH} && EXAMPLE_FEATURES={features} make LIBOS={LIBOS} all-libs")
        clean = 'make clean-redis &&' if len(sys.argv) > 2 and sys.argv[2] == 'clean' else ''
        return os.system(f'cd {CAPYBARA_PATH} && {clean} EXAMPLE_FEATURES={features} REDIS_LOG={REDIS_LOG} make redis-server{mig}')
    else :
        return os.system(f"cd {CAPYBARA_PATH} && EXAMPLE_FEATURES={features} make LIBOS={LIBOS} all-examples-rust")
    
    # else:
    #     print(f'Invalid server app: {SERVER_APP}')
    #     exit(1)


if __name__ == '__main__':

    if len(sys.argv) > 1 and sys.argv[1] == 'build':
        exit(run_compile())
    
    atexit.register(exiting)
    # cleaning()
    run_eval()
    # parse_result()
    kill_procs()