import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt
import argparse

def read_csv(csvfile, reset_time=True):
    df = pd.read_csv(csvfile, delimiter='\t').sort_values('TIME')
    if reset_time:
        df.TIME -= min(df.TIME)
    return df

def plot_timeseries(df, second_ticks=True):
    if second_ticks:
        df.TIME /= 1e9
    lplot = sns.scatterplot(x=df.TIME, y='VALUE', data=df, palette='colorblind')
    lplot.set(xlabel='Timestamp', ylabel='latency (ns)')
    plt.gcf().suptitle('End to end latency')
    #plt.show()
    lplot.figure.savefig('end-to-end-latency.pdf', format='pdf')

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('log', nargs='+', help='CSV log files')
    args = parser.parse_args()

    for csv in args.log:
        plot_timeseries(read_csv(csv))
