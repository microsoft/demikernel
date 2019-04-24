# Python 3 script for logging into and running the echo server on multiple clients
# Written by Anna Kornfeld Simpson 2019
# To get a Python virtual environment with the correct packages, run "python3 -m venv venv",
# "source venv/bin/activate", and "pip3 install -r requirements.txt"
# Note: fabric currently uses some deprecated API from python/cryptography so you will see warnings
#       about that. However this does not current prevent correct execution.

from fabric import Connection
import argparse
import threading
import time

# Runs inside a thread that has set up an SSH connection to the client
def runSomeone(c, clientName, size, iterations):

    dirCmd = "cd /opt/demeter/src/build/apps/echo && "
    clientCmd = "{0} -s {1} -i {2}".format(clientName, str(size), str(iterations))
    print("Running {0} on {1}...\n".format(clientCmd, c.host))
    myResults = c.run(dirCmd + "taskset -c 2 " + clientCmd)
    return myResults

class myThread(threading.Thread):
    def __init__(self, host, clientName, size, iterations):
        threading.Thread.__init__(self)
        self.host = host
        self.clientName = clientName
        self.size = size
        self.iterations = iterations
        self.conn = Connection(host)
    def run(self):
        return runSomeone(self.conn, self.clientName, self.size, self.iterations)


def runMain(clientName="./posix-client", size=1024, iterations=1000000, totalConns=1):
    fullHosts = ["demeter2", "demeter4", "demeter7", "demeter5", "demeter6", "demeter1"]
    myHosts = fullHosts[0:totalConns]
    print("Using hosts: {}\n".format(myHosts))
    threads = []

    # Initial setup not part of the experiment
    for host in myHosts:
        conn = Connection(host)
        conn.run("sudo mount /opt/demeter")
        conn.run("sudo sysctl -w vm.nr_hugepages=512")
        conn.close()

    # Experiment runs parallel in threads
    for host in myHosts:
        newThread = myThread(host, clientName, size, iterations)
        newThread.start()
        threads.append(newThread)

    print("Done starting the runs!\n")

    for t in threads:
        t.join()

    print("Runs think they are done!\n")
    time.sleep(1)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("client",                           default="rdma", help="posix, rdma, dpdk, or raw")
    parser.add_argument("size",                   type=int, default=64,     help="packet size in bytes for the echo load")
    parser.add_argument("iterations",             type=int, default=100000, help="how many sends to call")
    parser.add_argument("-t", "--threadsPerConn", type=int, default=1,      help="how many client threads per machine")
    parser.add_argument("-c", "--connections",    type=int, default=1,      help="how many machines")
    args = parser.parse_args()

    clientName = "./dmtr-rdma-client"
    if args.client == "posix":
        clientName = "./dmtr-posix-client"
    elif args.client == "dpdk":
        clientName = "./dmtr-lwip-client"
    elif args.client == "raw":
        clientName = "./posix-client"

    runMain(clientName, args.size, args.iterations, args.threadsPerConn, args.connections)
