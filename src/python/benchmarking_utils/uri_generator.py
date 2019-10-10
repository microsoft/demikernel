#!/usr/bin/env python3

import argparse
import numpy as np
import scipy.special
import random
import os.path
import pickle

webserver_root = '/media/wiki/'

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
def draw_elements(zalpha, array, n_elems):
    elements = []
    z = 1.0 / scipy.special.zeta(zalpha)
    print('z is {} (zalpha={})'.format(z, zalpha))
    p = [z / i**zalpha for i in range(1, len(array)+1)]
    print('first 20 probas: {}'.format(p[:20]))
    for n in range(0, n_elems):
        r_key = 0.0
        # random()'s range is [0.0, 1.0)
        while r_key == 0.0:
            r_key = random.random()

        # We look for the Zeta value closest from the number we drew
        l = 0
        r = len(array)
        while l < r:
            mid = int((l + r) / 2)
            if r_key > p[mid]:
                r = mid - 1
            elif r_key < p[mid]:
                l = mid + 1
            else:
                break
        elements.append(array[mid])
    return elements

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
        strengths = draw_elements(zalpha, regex_strengths_bins, n_req)

    # Generate URIS accordingly
    regex_uris = []
    for i in range(0, n_req):
        regex_str = '?regex=' + ''.join(['a' for i in range(0, strengths[i])]) + 'b'
        regex_uris.append(regex_str)

    return np.array(regex_uris)

def get_files(list_file, sort=False, ascending=False):
    filename = list_file + '.pkl'
    files_and_sizes = []
    if not os.path.isfile(filename):
        files = []
        with open(list_file, 'r') as f:
            files = np.array(f.read().splitlines())
        if len(files) == 0:
            log_fatal('{} was (likely) empty'.format(list_file))

        for f in files:
            try:
                size = os.path.getsize(webserver_root + f)
                if size > 0:
                    files_and_sizes.append((f, size))
            except OSError as e:
                print('Could not get size for {}: {}'.format(f, repr(e)))

        with open(filename, 'wb') as f:
            pickle.dump(files_and_sizes, f);
    else:
        with open(filename, 'rb') as f:
            files_and_sizes = pickle.load(f)

    if sort:
        files_and_sizes.sort(key=lambda f: f[1], reverse=(not ascending))
    else:#XXX untested
        random.shuffle(files_and_sizes)

    return [f[0] for f in files_and_sizes]

def gen_file_requests_tweak(n_req, zalpha, list_file):
    files = get_files(list_file, sort=False)
    sorted_files = get_files(list_file, sort=True)
    print('Generating 90% zipf popular + 10% unpopular (zalpha={})'.format(zalpha))

    popular_files = draw_elements(zalpha, files, int(n_req*0.9))
    unpopular_files = draw_elements(zalpha, sorted_files, int(n_req*0.1))
    print('Selected popular URIS:')
    print_file_uris_stats(popular_files)
    print('Selected unpopular URIS:')
    print_file_uris_stats(unpopular_files)

    return popular_files + unpopular_files

def gen_file_requests(n_req, zalpha, list_file):
    files = get_files(list_file)

    selected_files_uris = []
    if zalpha == -1:
        m = len(files) - 1
        idx = np.random.randint(0, m, size=n_req, dtype='i')
        selected_files_uris = [files[i][0] for i in idx]
    else:
        selected_files_uris = draw_elements(zalpha, [f[0] for f in files], n_req)
        print_file_uris_stats(selected_files_uris)

    return selected_files_uris

def print_file_uris_stats(uris):
    print('num URIs: {}'.format(len(uris)))
    print('num unique requests: {}'.format(len(set(uris))))
    sizes = []
    for uri in set(uris):
        try:
            sizes.append(os.path.getsize(webserver_root + uri))
        except OSError as e:
            print('Could not get size for {}: {}'.format(uri, repr(e)))

    print('total size for unique requests: {}'.format(sum(sizes)))
    print('min size for unique requests: {}'.format(min(sizes)))
    print('max size for unique requests: {}'.format(max(sizes)))

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
    parser.add_argument('--tweak-html', action='store_true',
                        help='Use _tweak function to generate html uris',
                        dest='html_tweak', default=False)

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
        log_fatal('Zipf alpha must be greater than 0 (regex zalpha: {})'.format(args.regex_zalpha))
    if args.html_zalpha > -1 and args.html_zalpha < 1:
        log_fatal('Zipf alpha must be greater than 0 (html zalpha: {})'.format(args.html_zalpha))

    # Generate regex requests
    if args.regex_ratio > 0:
        n_regex_requests = int(args.n_requests * args.regex_ratio)
        regex_requests = gen_regex_requests(n_regex_requests, args.regex_zalpha)
    else:
        regex_requests = np.array([])

    # Generate file requests
    if args.html_ratio > 0:
        n_file_requests = int(args.n_requests * args.html_ratio)
        if args.html_tweak:
            file_requests = gen_file_requests_tweak(n_file_requests, args.html_zalpha, args.html_files_list)
        else:
            file_requests = gen_file_requests(n_file_requests, args.html_zalpha, args.html_files_list)
    else:
        file_requests = np.array([])

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
