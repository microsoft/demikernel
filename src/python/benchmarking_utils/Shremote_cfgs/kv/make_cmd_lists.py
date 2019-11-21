#!/usr/bin/env python3
import sys
import os
import random
import string
from argparse import ArgumentParser

parser = ArgumentParser()
parser.add_argument('srv_file')
parser.add_argument('client_file')
parser.add_argument('value_size', type=int)
parser.add_argument('n_keys', type=int)
parser.add_argument('n_reqs', type=int)
parser.add_argument('reqtype', type=str)
args = parser.parse_args()

if args.reqtype not in ("SZOF", "GET", "NNZ"):
    raise Exception("args.reqtype must be one of SZOF GET NNZ")

keys = [('%d' % i) for i in range(args.n_keys)]

def write_server_files(filename):
    values = [''.join(random.choices(string.ascii_letters, k=args.value_size)) for _ in keys]
    sets = ['PUT %s %s' % (k, v) for k, v in zip(keys, values)]
    with open(filename, 'w') as f:
        f.write('\n'.join(sets))

def write_client_files(filename, n):
    sampling = random.choices(keys, k=n)
    gets = ['%s %s' % (args.reqtype, s) for s in sampling]

    with open(filename, 'w') as f:
        f.write('\n'.join(gets))

write_server_files(args.srv_file)
write_client_files(args.client_file, args.n_reqs)
