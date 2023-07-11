# Copyright (c) Microsoft Corporation.
# Licensed under the MIT license.

def dummy_job_test_system_rust(test_alias, test_name, repo, libos, is_debug, server, client, server_args, client_args,
                         is_sudo, all_pass, delay, config_path, log_directory):
    print(f"\n\n====>> test_name = {test_name}")
    print(f"test_alias = {test_alias}")
    print(f"repo = {repo}")
    print(f"libos = {libos}")
    print(f"is_debug = {is_debug}")
    print(f"server = {server}")
    print(f"client = {client}")
    print(f"server_args = {server_args}")
    print(f"client_args = {client_args}")
    print(f"is_sudo = {is_sudo}")
    print(f"all_pass = {all_pass}")
    print(f"delay = {delay}")
    print(f"config_path = {config_path}")
    print(f"log_directory = {log_directory}")
    return True
