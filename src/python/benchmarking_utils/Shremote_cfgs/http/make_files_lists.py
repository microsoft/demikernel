#!/usr/bin/env python3
import sys
import os
import random
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('srv_file')
parser.add_argument('client_file')
parser.add_argument('file_dir', type=str)
parser.add_argument('n_sources', type=int)
parser.add_argument('n_reqs', type=int)
args = parser.parse_args()

server_prefix = '/media/wiki/'

files = ['fixed_size/%s/%d' % (args.file_dir,i) for i in range(1, args.n_sources+1)]

prefix1 = 'POPULAR,'
prefix2 = 'UNPOPULAR,'

def write_server_files(filename):
    with open(filename, 'w') as f:
        f.write('\n'.join([os.path.join(server_prefix, file) for file in files]))

def write_client_files(filename, n):

    client_files = []

    for i, file in enumerate(files):
        if i < len(files) / 2:
            client_files.append(prefix1 + file)
        else:
            client_files.append(prefix2 + file)

    sampling = random.choices(client_files, k=n)

    with open(filename, 'w') as f:
        f.write('\n'.join(sampling))


if len(sys.argv) < 4:
    print("Usage: srv.txt clt.txt n")
    exit(1)

write_server_files(args.srv_file)
write_client_files(args.client_file, args.n_reqs)
