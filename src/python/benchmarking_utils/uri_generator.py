#!/usr/bin/env python3

import argparse
import numpy as np
import scipy.special
import random
import os.path
import pickle
import time

webserver_root = '/media/memfs/wiki/'

COLORS = dict(
    END='\033[0m',
    WARNING = '\033[93m',
    ERROR = '\033[31m',
    INFO = '\033[0;32m'
)

LOG_DEBUG = True
LOGFILE = None
fatality = False
start_time = time.time()

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
    if len(array) == 0:
        log_fatal('Cannot draw elements from an empty array')
    if n_elems <= 0:
        log_fatal('Cannot draw {} elements from an array'. format(n_elems))

    elements = []
    z = 1.0 / scipy.special.zeta(zalpha)
    #print('z is {} (zalpha={})'.format(z, zalpha))
    p = [z / i**zalpha for i in range(1, len(array)+1)]
    #print('first 20 probas: {}'.format(p[:20]))
    for n in range(0, n_elems):
        r_key = 0.0
        # random()'s range is [0.0, 1.0)
        while r_key == 0.0:
            r_key = random.random()

        # We look for the Zeta value closest from the number we drew
        l = 0
        r = len(array)
        mid = 0
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

def gen_regex_requests(n_req, zalpha, flag_type=False):
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
        if flag_type:
            regex_str = 'REGEX,' + regex_str
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
                print('Could not get size for {}: {}'.format(webserver_root+f, repr(e)))

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

'''
 Generates a set of "popular files", randomly selected from a non size-sorted, zipf distributed list of files
 Generates a set of "unpopular files, randomly selected from a size-sorted, zipf distributed list of files
 To clarify: because the zipf probabilities are applied to the elements of the list in order, in the second case,
 the largest files will have higher probability to get selected.
 Generates a set of "cache no use" requests (regex)
'''
def gen_exp_series(n_req, cir_ratio, files_zalpha, list_file, regex_zalpha, flag_type=False):
    print('Generating {} CIN and {:.2} CIN/CN (Zipf: files zalpha={}, regex zalpha={})'.format(
        cir_ratio, 1-cir_ratio, files_zalpha, regex_zalpha
    ))
    files = get_files(list_file)
    sorted_files = get_files(list_file, sort=True)

    drawn_uris = draw_elements(files_zalpha, files, int(n_req*cir_ratio))
    print('Selected CIR URIS:')
    print_file_uris_stats(drawn_uris)
    if flag_type:
        popular_files = ['POPULAR,'+uri for uri in drawn_uris]
    else:
        popular_files = drawn_uris

    drawn_uris = draw_elements(files_zalpha, sorted_files, int(n_req*(1-cir_ratio)))
    print('Selected CIN URIS:')
    print_file_uris_stats(drawn_uris)
    if flag_type:
        unpopular_files = ['UNPOPULAR,'+uri for uri in drawn_uris]
    else:
        unpopular_files = drawn_uris

    regex_requests = gen_regex_requests(int(n_req*(1-cir_ratio)), regex_zalpha, flag_type=flag_type)

    return popular_files, unpopular_files, regex_requests

def gen_file_requests(n_req, zalpha, list_file, flag_type=False):
    files = get_files(list_file)

    selected_files_uris = []
    if zalpha == -1:
        m = len(files) - 1
        idx = np.random.randint(0, m, size=n_req, dtype='i')
        if flag_type:
            selected_files_uris = ['PAGE,'+files[i] for i in idx]
        else:
            selected_files_uris = [files[i] for i in idx]
    else:
        drawn_uris = draw_elements(zalpha, files, n_req)
        if flag_type:
            selected_files_uris = ['PAGE,'+uri for uri in drawn_uris]
        else:
            selected_files_uris = drawn_uris
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
                        dest='html_files_list', default=None)
    parser.add_argument('--exp-series', action='store_true',
                        help='Generate a two files: CIN and CN, CIN and CIR, with the same CIN base',
                        dest='exp_series', default=False)
    parser.add_argument('--cir-ratio', type=float, help='Ratio of CIN requests',
                        dest='cir_ratio', default=0.9)
    parser.add_argument('--flag-type', action='store_true',
                        help='Append the type of request to the URI',
                        dest='flag_type', default=False)

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

    if args.exp_series:
        log_info('Attempting to generate CIR/CIN/CN experimental dataset.')

        if args.html_zalpha == -1:
            log_fatal('Need html zalpha > 0')
        if args.regex_zalpha == -1:
            log_fatal('Need regex zalpha > 0')
        if args.html_files_list is None:
            log_fatal('Need a list of HTML files')

        cir, cin, cn = gen_exp_series(
            args.n_requests, args.cir_ratio,
            args.html_zalpha, args.html_files_list,
            args.regex_zalpha, flag_type=args.flag_type
        )

        # Concatenate and shuffle CIN, CIR and CN in two datasets
        cir_cn = np.concatenate([cir, cn])
        random.shuffle(cir_cn)

        cir_cin = np.concatenate([cir, cin])
        random.shuffle(cir_cin)

        # Now create two files: cir_cn, cir_cin
        filename = 'CIR{:.2}-CN{:.2}-{}req-{}zf-{}zr_'.format(
            args.cir_ratio, 1-args.cir_ratio,
            args.n_requests, args.html_zalpha, args.regex_zalpha
        ) + args.output_file
        with open(filename, 'w+') as f:
            f.writelines(['{}\n'.format(request) for request in cir_cn])

        filename = 'CIR{:.2}-CIN{:.2}-{}req-{}zf-{}zr_'.format(
            args.cir_ratio, 1-args.cir_ratio,
            args.n_requests, args.html_zalpha, args.regex_zalpha
        ) + args.output_file
        with open(filename, 'w+') as f:
            f.writelines(['{}\n'.format(request) for request in cir_cin])
    else:
        # Generate regex requests
        if args.regex_ratio > 0:
            n_regex_requests = int(args.n_requests * args.regex_ratio)
            regex_requests = gen_regex_requests(n_regex_requests, args.regex_zalpha, args.flag_type)
        else:
            regex_requests = np.array([])

        # Generate file requests
        if args.html_ratio > 0:
            n_file_requests = int(args.n_requests * args.html_ratio)
            file_requests = gen_file_requests(
                n_file_requests, args.html_zalpha, args.html_files_list, args.flag_type
            )
        else:
            file_requests = np.array([])

        all_requests = np.concatenate([regex_requests, file_requests])
        random.shuffle(all_requests)

        # interleave requests (of each type) in the output file
        with open(args.output_file, 'w+') as f:
            f.writelines(['{}\n'.format(request) for request in all_requests])
