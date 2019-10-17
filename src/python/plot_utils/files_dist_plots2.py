import seaborn as sns
import pandas as pd
import argparse
import matplotlib.pyplot as plt
import os.path
import numpy as np

webserver_root = '/media/memfs/wiki/'

def plot_hist(files_list, trim_flags=False):
    files = []
    with open(files_list, 'r') as f:
        files = np.array(f.read().splitlines())
    if len(files) == 0:
        print('{} was likely empty?'.format(files_list))
        return

    sizes = []
    for f in files:
        try:
            if trim_flags:
                filepath = os.path.join(webserver_root, f.split(',')[1])
            else:
                filepath = os.path.join(webserver_root, f)

            size = os.path.getsize(filepath)
            if size > 0:
                sizes.append(size)
        except OSError as e:
            print('Could not get size for {}'.format(filepath))

    '''
    hplot = sns.distplot(sizes, bins=1000)
    hplot.set(xlabel='Bytes', ylabel='#files')
    hplot.xaxis.set_major_locator(plt.MaxNLocator(18))
    for item in hplot.get_xticklabels():
        item.set_rotation(45)

    hplot.figure.suptitle('Size distribution of files served')
    filename = os.path.basename(files_list) + '-size-hist.pdf'
    hplot.figure.savefig(filename, format='pdf')
    '''
    fig = plt.figure()
    ax = fig.add_subplot(1,1,1)
    ax.plot(np.log2(np.sort(sizes)), np.linspace(0, 1, len(sizes), endpoint=False))
    filename = os.path.basename(files_list) + '-size-hist.pdf'
    fig.savefig(filename, format='pdf')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('files', nargs='+', help='list of URIs')
    parser.add_argument('--trim-flags', action='store_true', dest='trim_flags', default=False,
                        help='remove request type flag from URI list')
    args = parser.parse_args()

    for f in args.files:
        plot_hist(f, trim_flags=args.trim_flags)
