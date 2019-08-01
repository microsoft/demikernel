#!/usr/bin/env python3

import argparse
import numpy as np
import scipy.special
import random

COLORS = dict(
    END='\033[0m',
    WARNING = '\033[93m',
    ERROR = '\033[31m',
    INFO = '\033[0;32m'
)

LOG_DEBUG = True
fatality = False

def log_(s, **print_kwargs):
    if (LOGFILE is not None):
        LOGFILE.write(s + '\n')
    if LOG_DEBUG:
        print(s, **print_kwargs)

def log(*args, **kwargs):
    s = "DEBUG {:.1f}: ".format(time.time() - start_time)
    log_(s + " ".join([str(x) for x in args]), **kwargs)

def log_info(*args, **kwargs):
    s = COLORS['INFO'] + 'INFO {:.1f}: '.format(time.time() - start_time)
    log_(s + " ".join([str(x) for x in args]) + COLORS['END'], **kwargs)

def log_warn(*args, **kwargs):
    s = COLORS["WARNING"] + "WARNING: " + ' '.join([str(x) for x in args]) + COLORS['END']
    log_(s, **kwargs)

def log_error(*args, **kwargs):
    s = COLORS['ERROR'] + "\n________________________________________________\n"
    s += "ERROR: " + " ".join([str(x) for x in args])
    s += "\n________________________________________________" + COLORS['END'] + "\n"
    log_(s, **kwargs)

def log_fatal(*args, **kwargs):
    global fatality
    if fatality:
        print("DOUBLE FATALITY: ", *args, **kwargs)
        exit(-1)

    print("\n________________________________________________")
    print("------- FATAL ERROR: ", *args, **kwargs)
    print("________________________________________________\n")
    fatality = True
    print("Exiting")
    exit(-1)


#Regex CPU time will increase exponentially from one bin to the next
regex_strengths_bins = [1,2,3,4,5,6,7,8,9,10]

#FIXME: Is it right to "uniformely draw a Zeta probability"?
def gen_regex_requests(n_req, zalpha):
    if (n_req <= 0):
        return []

    # First generate the strengths distribution
    strengths = []
    if zalpha == -1:
        mini = min(regex_strengths_bins) - 1
        maxi = max(regex_strengths_bins) - 1
        strengths = np.random.randint(mini, maxi, size=n_req, dtype='i')
    else:
        z = 1.0 / scipy.special.zeta(zalpha)
        strengths_p = [z / i**zalpha for i in range(1, len(regex_strengths_bins)+1)]
        for req in range(0, n_req):
            r_key = 0.0
            # random()'s range is [0.0, 1.0)
            while r_key == 0.0:
                r_key = random.random()

            # We look for the Zeta value closest from the number we drew
            l = 0
            r = len(regex_strengths_bins)
            while l < r:
                mid = int((l + r) / 2)
                if r_key > strengths_p[mid]:
                    r = mid - 1
                elif r_key < strengths_p[mid]:
                    l = mid + 1
                else:
                    break
            strengths.append(regex_strengths_bins[mid])

    # Generate URIS accordingly
    regex_uris = []
    for i in range(0, n_req):
        regex_str = '?regex=' + ''.join(['a' for i in range(0, strengths[i])]) + 'b'
        regex_uris.append(regex_str)

    return np.array(regex_uris)


def gen_file_requests(n_req, zalpha, list_file):
    files_uris = None
    with open(list_file, 'r') as f:
        files_uris = np.array(f.read().splitlines())
    if files_uris is None:
        log_fatal('{} was (likely) empty'.format(list_file))

    selected_files_uris = []
    if zalpha == -1:
        m = len(files_uris) - 1
        selected_files_uris = files_uris[np.random.randint(0, m, size=n_req, dtype='i')]
    else:
        log_fatal('Zipf file selection not implemented yet')

    return selected_files_uris

if __name__ == '__main__':
    parser = argparse.ArgumentParser(
        description='Generate a list of URI for Demeter html server experiments'
    )
    parser.add_argument('output_file', type=str, help='Output file name')
    parser.add_argument('n_requests', type=int, help='Number of requests to generate')
    parser.add_argument('--html-ratio', type=float, help='Ratio of html requests',
                        dest='html_ratio', default=1)
    parser.add_argument('--regex-ratio', type=float, help='Ratio of RegEX requests',
                        dest='regex_ratio', default=0)
    parser.add_argument('--html-zalpha', type=float, help='html file size Zipf alpha (-1 to disable)',
                        dest='html_zalpha', default=-1)
    parser.add_argument('--regex-zalpha', type=float, help='RegEX strength Zipf alpha (-1 to disable)',
                        dest='regex_zalpha', default=-1)
    parser.add_argument('--html-files-list', type=str, help='File holding html pages to request',
                        dest='html_files_list', default='./')

    args = parser.parse_args()

    # Check arguments
    if args.html_ratio + args.regex_ratio > 1:
        log_fatal('Given ratios exceed 1 (html: {}, regex: {})'.format(
            args.html_ratio, args.regex_ratio
        ))
    if args.html_ratio < 0 or args.regex_ratio < 0:
        log_fatal('Ratios are between 0 and 1 (html: {}, regex: {})'.format(
            args.html_ratio, args.regex_ratio
        ))
    if args.regex_zalpha > -1 and args.regex_zalpha < 1:
        log_fatal('Zipf alpha must be greater than 1 (regex zalpha: {})'.format(args.regex_zalpha))
    if args.html_zalpha > -1 and args.html_zalpha < 1:
        log_fatal('Zipf alpha must be greater than 1 (html zalpha: {})'.format(args.html_zalpha))

    # Generate regex requests
    if args.regex_ratio > 0:
        n_regex_requests = int(args.n_requests * args.regex_ratio)
        regex_requests = gen_regex_requests(n_regex_requests, args.regex_zalpha)
    else:
        regex_requests = np.array([])

    # Generate file requests
    if args.html_ratio > 0:
        n_file_requests = int(args.n_requests * args.html_ratio)
        file_requests = gen_file_requests(n_file_requests, args.html_zalpha, args.html_files_list)
    else:
        file_requests = np.array([])

    # Generate file requests
    if args.html_ratio > 0:
        n_file_requests = int(args.n_requests * args.html_ratio)
        file_requests = gen_file_requests(n_file_requests, args.html_zalpha, args.html_files_list)

    all_requests = np.concatenate([regex_requests, file_requests])
    random.shuffle(all_requests)

    # interleave requests (of each type) in the output file
    with open(args.output_file, 'w+') as f:
        f.writelines(['{}\n'.format(request) for request in all_requests])

    all_requests = np.concatenate([regex_requests, file_requests])
    random.shuffle(all_requests)

    # interleave requests (of each type) in the output file
    with open(args.output_file, 'w+') as f:
        f.writelines(['{}\n'.format(request) for request in all_requests])
