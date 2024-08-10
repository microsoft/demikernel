This document contains instructions on how to run Autokernel experiments.
We use PyREM package (https://pypi.org/project/PyREM/) to run processes on remote nodes. 

## Assumptions
- This repository (autokernel branch) has cloned and built on a machine (for server).
- Caladan repository (https://github.com/ihchoi12/caladan.git) has cloned and built on a machine (for client).
- Please follow the corresponding document (e.g., README.md) for building

## Running Experiment 
- Edit `eval/test_config.py` according to your environment and the experimental setup you want to run. You may want to setup ssh alias for simplicity. 
- run `python3 -u eval/run_eval.py` 