import seaborn as sns
import pandas as pd
import argparse
import matplotlib.pyplot as plt

def plot_hist(csvfile):
    s = []
    with open(csvfile, 'r') as f:
        for line in f:
            line = line.rstrip()
            if line.endswith('K'):
                s.append(float(line[:-1]) * 1e3)
            elif line.endswith('M'):
                s.append(float(line[:-1]) * 1e6)
    series = pd.Series(s)

    '''
    vplot = sns.violinplot(series)
    vplot.set(xlabel='Bytes')
    plt.show()
    vplot.figure.suptitle('Size distribution of files served')
    vplot.figure.savefig('files-size-violin.pdf', format='pdf')
    '''

    hplot = sns.distplot(series, bins=1000)
    hplot.set(xlabel='Bytes', ylabel='#files')
    hplot.xaxis.set_major_locator(plt.MaxNLocator(18))
    for item in hplot.get_xticklabels():
        item.set_rotation(45)
    plt.show()
    hplot.figure.suptitle('Size distribution of files served')
    hplot.figure.savefig('files-size-hist.pdf', format='pdf')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('files', nargs='+', help='file with sizes')
    args = parser.parse_args()

    for f in args.files:
        plot_hist(f)
